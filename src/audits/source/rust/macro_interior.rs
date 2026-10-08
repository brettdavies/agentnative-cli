//! Recover genuine `.unwrap()` calls from inside a macro invocation's token tree.
//!
//! tree-sitter models macro arguments as flat tokens: `v.unwrap()` inside
//! `write!(..)` parses as `identifier . identifier token_tree`, never as the
//! `call_expression` the plain matcher in [`super::unwrap`] looks for. The
//! interior is a verbatim slice of the file, so re-parsing it recovers the call
//! and its position maps back to file coordinates.
//!
//! Reach here is bought against precision, and error recovery will invent a
//! call the source does not hold: the interior `"{}", v.unwrap()` parses as one
//! `call_expression` whose receiver glues the string literal, the comma and `v`
//! together. Its text is verbatim in the file, so no range comparison rejects
//! it. [`unwrap_calls_in_interior`] therefore discards a candidate whose own
//! subtree rests on error recovery or whose receiver is a macro fragment
//! reference.
//!
//! Two kinds of interior are never re-parsed at all, because a call in them is
//! not a call this crate runs: a `macro_rules!` transcriber, which is a token
//! template, and the arguments of a macro that never evaluates them.

use ast_grep_core::Node;
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use ast_grep_language::Rust;

use crate::types::SourceLocation;

use super::unwrap::unwrap_call_snippet;

/// Wraps an interior into an argument list so the parse starts in expression
/// position, which is the position a macro interior actually holds. Single-line
/// by construction, so the prefix shifts the column of nodes on the interior's
/// first line and nothing after the first newline.
const EXPR_WRAPPER_PREFIX: &str = "fn __anc_interior() {__anc_args(";
const EXPR_WRAPPER_SUFFIX: &str = ")}";

/// Matching delimiter pairs a `token_tree` can carry.
const DELIMITERS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}')];

/// Macros whose arguments are tokens rather than code this crate runs, so a
/// `.unwrap()` written in one never panics here.
///
/// `stringify!` and `concat!` expand to a literal, `cfg!` to a boolean, and a
/// `quote!` body is a template for the crate the macro generates, where that
/// crate's own audit is the one that should report it. Matched on the last path
/// segment, so `quote::quote!` and `::quote::quote!` are covered.
///
/// A denylist rather than an allowlist of evaluating macros: `write!`,
/// `format!`, `assert_eq!`, `log::info!`, `lazy_static!`, `thread_local!` and
/// every third-party macro that does evaluate its arguments have to keep
/// reporting, and they cannot be enumerated.
const NON_EVALUATING_MACROS: &[&str] = &["stringify", "concat", "cfg", "quote", "quote_spanned"];

/// Interior-relative, zero-based position of a recovered node.
#[derive(Clone, Copy, PartialEq, Eq)]
struct InteriorPos {
    line: usize,
    column: usize,
}

/// A call kept from one of the re-parses, still in interior coordinates.
struct Recovered {
    pos: InteriorPos,
    snippet: String,
}

/// Find `.unwrap()` calls the source genuinely holds inside `token_tree`.
///
/// `token_tree` is the node as the walker reached it, delimiters included.
/// Returned locations are in file coordinates with `file` attached, at most one
/// per position, and carry the same snippet shape the plain matcher reports.
/// An interior the audit cannot speak for yields nothing.
pub(super) fn unwrap_calls_in_interior(
    token_tree: &Node<'_, StrDoc<Rust>>,
    file: &str,
) -> Vec<SourceLocation> {
    let text = token_tree.text();
    let Some(interior) = strip_delimiters(text.as_ref()) else {
        return Vec::new();
    };

    // Verdict-preserving precondition: the matcher only reports a call whose
    // text ends with `.unwrap()`, and that text is always a substring of the
    // interior. Keeps the cost proportional to real unwraps rather than to how
    // many macros the audited project contains, and it runs before the ancestor
    // walk below so that walk is paid for only by an interior that could match.
    if !interior.contains(".unwrap()") {
        return Vec::new();
    }

    if inside_macro_definition(token_tree) || inside_non_evaluating_macro(token_tree) {
        return Vec::new();
    }

    let start = token_tree.start_pos();
    // Every delimiter is one character wide, so the interior begins one column
    // past the token tree's own start.
    let interior_line = start.line();
    let interior_column = start.column(token_tree) + 1;

    let mut found: Vec<Recovered> = Vec::new();
    collect_from_item_grammar(interior, &mut found);
    collect_from_expression_grammar(interior, &mut found);

    found
        .into_iter()
        .map(|call| SourceLocation {
            file: file.to_string(),
            line: interior_line + call.pos.line + 1,
            // The interior's start column applies only to its own first line;
            // after the first newline a recovered column is already the file's.
            column: if call.pos.line == 0 {
                interior_column + call.pos.column + 1
            } else {
                call.pos.column + 1
            },
            text: call.snippet,
        })
        .collect()
}

/// The bare interior, parsed from the grammar root, which is item position.
/// This is the path that reaches a `lazy_static!` or `thread_local!` body.
fn collect_from_item_grammar(interior: &str, found: &mut Vec<Recovered>) {
    let root = Rust.ast_grep(interior);
    collect_candidates(&root.root(), interior, found, Some);
}

/// The interior wrapped in a synthetic argument list, so each argument parses as
/// its own expression. The wrapper's prefix comes off before any position is
/// read.
fn collect_from_expression_grammar(interior: &str, found: &mut Vec<Recovered>) {
    let wrapped = format!("{EXPR_WRAPPER_PREFIX}{interior}{EXPR_WRAPPER_SUFFIX}");
    let prefix_columns = EXPR_WRAPPER_PREFIX.chars().count();
    let root = Rust.ast_grep(&wrapped);
    collect_candidates(&root.root(), interior, found, |pos| {
        if pos.line == 0 {
            // The interior starts mid-line, past the prefix. A column inside
            // the prefix belongs to the wrapper, not to the interior.
            Some(InteriorPos {
                line: 0,
                column: pos.column.checked_sub(prefix_columns)?,
            })
        } else {
            Some(pos)
        }
    });
}

/// Walk a re-parsed tree, keeping every candidate the interior itself holds.
///
/// `to_interior` maps a position in the parsed text back to interior
/// coordinates, which is where the expression wrapper's prefix comes off.
fn collect_candidates(
    root: &Node<'_, StrDoc<Rust>>,
    interior: &str,
    found: &mut Vec<Recovered>,
    to_interior: impl Fn(InteriorPos) -> Option<InteriorPos>,
) {
    for node in root.dfs() {
        let Some(snippet) = unwrap_call_snippet(&node) else {
            continue;
        };
        let pos = node.start_pos();
        let parsed = InteriorPos {
            line: pos.line(),
            column: pos.column(&node),
        };
        // Cheapest first: a position the other grammar already recorded needs no
        // structural judgement, and the two guards each walk a subtree.
        if let Some(rel) = to_interior(parsed)
            && !found.iter().any(|seen| seen.pos == rel)
            && interior_holds(interior, rel, &snippet)
            && !rests_on_error_recovery(&node)
            && !receiver_is_fragment(&node)
        {
            found.push(Recovered { pos: rel, snippet });
        }
    }
}

/// A `macro_rules!` body is a pattern, not code: `$x.unwrap()` in it re-parses
/// into a real `call_expression` whose text is verbatim in the file, so a range
/// comparison admits it and only a structural test rejects it.
///
/// Testing every ancestor rather than the nearest enclosing item is equivalent
/// for a definition at item position: its body is flat token trees all the way
/// down, so nothing re-parsable nests between. A definition written inside
/// another macro's arguments is not an item, so tree-sitter builds no
/// `macro_definition` node for it and the token pattern has to be read instead.
fn inside_macro_definition(node: &Node<'_, StrDoc<Rust>>) -> bool {
    if node.ancestors().any(|a| a.kind() == "macro_definition") {
        return true;
    }
    std::iter::once(node.clone())
        .chain(node.ancestors())
        .filter(|candidate| candidate.kind() == "token_tree")
        .any(|tree| follows_macro_rules_header(&tree))
}

/// Whether `tree` is the token tree of a `macro_rules! <name>` written as bare
/// tokens, which is how one nested in another macro's arguments appears.
///
/// The two tokens before the name are the discriminator. Testing only that some
/// earlier sibling says `macro_rules` would also skip a sibling macro written
/// after a definition in the same interior.
fn follows_macro_rules_header(tree: &Node<'_, StrDoc<Rust>>) -> bool {
    let Some(parent) = tree.parent() else {
        return false;
    };
    let mut preceding: Vec<String> = Vec::new();
    for child in parent.children() {
        if child.node_id() == tree.node_id() {
            break;
        }
        preceding.push(child.text().to_string());
        if preceding.len() > 3 {
            preceding.remove(0);
        }
    }
    matches!(
        preceding.as_slice(),
        [macro_rules, bang, _name] if macro_rules == "macro_rules" && bang == "!"
    )
}

/// Whether the macro that owns this token tree leaves its arguments as tokens.
///
/// The nearest enclosing `macro_invocation` is the owner: a macro nested inside
/// another macro's arguments is itself only tokens, so it builds no
/// `macro_invocation` node, and the outer macro's decision is the one that
/// governs the whole interior.
fn inside_non_evaluating_macro(node: &Node<'_, StrDoc<Rust>>) -> bool {
    let Some(invocation) = node.ancestors().find(|a| a.kind() == "macro_invocation") else {
        return false;
    };
    let Some(path) = invocation.children().next() else {
        return false;
    };
    let text = path.text();
    let name = text.rsplit("::").next().unwrap_or_default().trim();
    NON_EVALUATING_MACROS.contains(&name)
}

/// Whether the parser had to recover inside the candidate itself.
///
/// An error *above* a candidate is expected and admitted: a bare interior is
/// not a whole item, so a recovered call commonly sits under an ERROR root. An
/// error *within* one means the parser assembled the call out of tokens the
/// source does not join that way, which is the fabrication to reject.
fn rests_on_error_recovery(node: &Node<'_, StrDoc<Rust>>) -> bool {
    node.dfs().any(|d| d.is_error() || d.is_missing())
}

/// Whether the leftmost leaf of a candidate's receiver chain is a macro
/// fragment reference (`$x.unwrap()`, `$self.v.unwrap()`).
fn receiver_is_fragment(node: &Node<'_, StrDoc<Rust>>) -> bool {
    // The first leaf a pre-order walk reaches is the leftmost one.
    node.dfs()
        .find(Node::is_leaf)
        .is_some_and(|leaf| leaf.kind() == "metavariable")
}

/// Offset-sanity assertion: the interior's own text at `pos` starts with the
/// candidate's text. Every recovered node maps back to a real range by
/// construction, so this proves the arithmetic rather than the finding — it is
/// where an inverted column rule or a mis-sized wrapper prefix surfaces.
fn interior_holds(interior: &str, pos: InteriorPos, snippet: &str) -> bool {
    let Some(line) = interior.lines().nth(pos.line) else {
        return false;
    };
    let mut rest = line.chars().skip(pos.column);
    snippet.chars().all(|c| rest.next() == Some(c))
}

/// Strip a matching delimiter pair, yielding the interior text.
fn strip_delimiters(text: &str) -> Option<&str> {
    DELIMITERS.iter().find_map(|&(open, close)| {
        text.strip_prefix(open)
            .and_then(|rest| rest.strip_suffix(close))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Locations recovered from every token tree in `source`, in walk order.
    fn interior_locations(source: &str) -> Vec<SourceLocation> {
        let root = Rust.ast_grep(source);
        root.root()
            .dfs()
            .filter(|node| node.kind() == "token_tree")
            .flat_map(|node| unwrap_calls_in_interior(&node, "t.rs"))
            .collect()
    }

    #[test]
    fn recovers_a_call_from_an_expression_interior() {
        let found = interior_locations("fn f() {\n    write!(out, \"{}\", v.unwrap())?;\n}");
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].line, found[0].column), (2, 23));
        assert_eq!(found[0].text, "v.unwrap()");
    }

    #[test]
    fn recovers_a_call_from_an_item_interior() {
        let found =
            interior_locations("lazy_static! {\n    static ref RE: R = R::new(\"x\").unwrap();\n}");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, r#"R::new("x").unwrap()"#);
    }

    #[test]
    fn the_text_gate_is_not_what_rejects_a_string_literal() {
        // The gate admits this interior — its raw text does contain the
        // substring — so the matcher is what must reject it. Pins the gate as a
        // cost precondition rather than the thing making a literal pass.
        let interior = r#"("do not call .unwrap() in production")"#;
        assert!(strip_delimiters(interior).is_some_and(|text| text.contains(".unwrap()")));
        assert!(interior_locations(&format!("fn f() {{ eprintln!{interior}; }}")).is_empty());
    }

    #[test]
    fn a_macro_definition_body_is_never_re_parsed() {
        // The receiver is an ordinary identifier, so the fragment-receiver test
        // cannot reject this and the `macro_definition` skip is the only thing
        // that can. A `$x` receiver here would pass with the skip removed.
        let source = "macro_rules! m {\n    () => { GLOBAL.unwrap() };\n}";
        assert!(interior_locations(source).is_empty());
    }

    #[test]
    fn a_macro_definition_nested_in_another_macro_is_never_re_parsed() {
        // Nested, the definition is bare tokens rather than a `macro_definition`
        // node, and the transcriber's receiver is an ordinary identifier, so
        // neither the ancestor-kind test nor the fragment-receiver test sees it.
        let source = "cfg_if! {\n    if #[cfg(unix)] {\n        macro_rules! inner {\n            () => { GLOBAL.unwrap() };\n        }\n    }\n}";
        assert!(interior_locations(source).is_empty());
    }

    #[test]
    fn a_macro_after_a_nested_definition_still_reports() {
        // The header test must not skip a sibling macro that merely follows a
        // definition in the same interior.
        let source = "cfg_if! {\n    if #[cfg(unix)] {\n        macro_rules! inner {\n            () => { 1 };\n        }\n        write!(out, \"{}\", v.unwrap()).ok();\n    }\n}";
        let found = interior_locations(source);
        assert_eq!(found.len(), 1, "expected the write! call, got {found:?}");
        assert_eq!(found[0].text, "v.unwrap()");
    }

    #[test]
    fn a_token_only_macro_reports_nothing() {
        // These expand to a literal, a boolean, or tokens for another crate, so
        // no call in them runs here.
        for source in [
            "fn f(v: Option<u8>) { let _ = stringify!(v.unwrap()); }",
            "fn f() { let _ = quote! { let y = x.unwrap(); }; }",
            "fn f() { let _ = quote::quote! { let y = x.unwrap(); }; }",
            "fn f() { let _ = concat!(\"a\", stringify!(v.unwrap())); }",
        ] {
            assert!(
                interior_locations(source).is_empty(),
                "token-only macro reported a call: {source}"
            );
        }
    }

    #[test]
    fn a_token_only_macro_does_not_silence_its_neighbours() {
        let source = "fn f(v: Option<u8>) { let _ = stringify!(v.unwrap()); write!(o, \"{}\", v.unwrap()).ok(); }";
        let found = interior_locations(source);
        assert_eq!(
            found.len(),
            1,
            "expected only the write! call, got {found:?}"
        );
    }

    #[test]
    fn an_evaluating_macro_nested_in_an_evaluating_macro_still_reports() {
        // The owner test reads the nearest enclosing invocation, so an inner
        // macro written as bare tokens inherits the outer macro's decision.
        let found =
            interior_locations("fn f() { write!(o, \"{}\", format!(\"{}\", v.unwrap())).ok(); }");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "v.unwrap()");
    }

    #[test]
    fn an_item_interior_takes_the_interior_column_on_its_first_line() {
        // The item grammar needs no wrapper prefix, so this is the one offset
        // combination the other cases do not reach: item position, first line.
        let source = "thread_local! { static X: u8 = f().unwrap(); }";
        let found = interior_locations(source);
        assert_eq!(found.len(), 1);
        let expected = source.find("f().unwrap()").map(|i| i + 1);
        assert_eq!((found[0].line, Some(found[0].column)), (1, expected));
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // A multibyte character before the call on the interior's first line
        // shifts the byte offset but not the character column.
        let source = "fn f() { eprintln!(\"é{} ü\", v.unwrap()); }";
        let found = interior_locations(source);
        assert_eq!(found.len(), 1);
        let chars: Vec<char> = source.chars().collect();
        let needle: Vec<char> = "v.unwrap()".chars().collect();
        let column = chars
            .windows(needle.len())
            .position(|window| window == needle.as_slice())
            .map(|index| index + 1);
        assert_eq!((found[0].line, Some(found[0].column)), (1, column));
    }

    #[test]
    fn a_fragment_receiver_is_discarded_where_a_range_check_would_admit_it() {
        // `$x.unwrap()` is verbatim in the source, so its recovered range is
        // real and the re-parse yields a well-formed call. Only the receiver
        // test rejects it.
        let source = "fn f() { m!($x.unwrap()); }";
        assert!(source.contains("$x.unwrap()"));
        assert!(interior_locations(source).is_empty());
    }

    #[test]
    fn an_error_glued_receiver_is_discarded() {
        // The interior `"{}", v.unwrap()` parses as one call whose receiver is a
        // string literal joined to `v` through an ERROR node. The argument
        // wrapper recovers the genuine call, so the reported snippet is the call
        // alone and never the whole interior.
        let found = interior_locations("fn f() { format!(\"{}\", v.unwrap()); }");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "v.unwrap()");
    }

    #[test]
    fn a_comment_in_an_interior_is_not_a_call() {
        let found =
            interior_locations("fn f() {\n    m!(\n        // v.unwrap()\n        1\n    );\n}");
        assert!(found.is_empty());
    }

    #[test]
    fn the_offset_sanity_check_rejects_a_mismatched_position() {
        // Nothing in the matcher's own corpus makes this guard fire, so its
        // rejection path is pinned directly: it is what turns an arithmetic
        // error into a dropped candidate rather than a wrong line.
        let interior = "f, v.unwrap()";
        let at_call = InteriorPos { line: 0, column: 3 };
        assert!(interior_holds(interior, at_call, "v.unwrap()"));
        assert!(!interior_holds(
            interior,
            InteriorPos { line: 0, column: 4 },
            "v.unwrap()"
        ));
        assert!(!interior_holds(
            interior,
            InteriorPos { line: 5, column: 0 },
            "v.unwrap()"
        ));
    }

    #[test]
    fn the_expression_wrapper_prefix_stays_on_one_line() {
        // A newline in the prefix would shift every recovered line, and the
        // offset-sanity check would then reject the candidates rather than
        // misreport them: silent recall loss across the whole expression path.
        assert!(!EXPR_WRAPPER_PREFIX.contains('\n'));
    }

    #[test]
    fn an_interior_whose_delimiters_do_not_pair_yields_nothing() {
        // A token tree truncated at end of file carries no closing delimiter,
        // so there is no interior to speak for.
        assert!(interior_locations("fn f() { m!(v.unwrap()").is_empty());
    }
}
