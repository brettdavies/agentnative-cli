// Matched source text is arbitrary UTF-8, so no byte offset in this module is
// a known char boundary; previews are bounded by character count.
#![deny(clippy::string_slice)]

use ast_grep_core::matcher::MatcherExt;
use ast_grep_core::{Doc, Matcher, Node, NodeMatch, Pattern};

use crate::types::SourceLocation;

/// Collapse `text` to one line and bound it to `max_chars` characters,
/// appending `...` only when characters were dropped.
///
/// The budget counts characters, never bytes: matched source text is
/// arbitrary UTF-8, and a byte offset can land inside a multi-byte sequence,
/// where `str` slicing panics.
pub fn evidence_preview(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars().map(|c| if c == '\n' { ' ' } else { c });
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

/// Whether any of `patterns` matches anywhere under `root`, found in one walk.
pub fn has_any<D: Doc>(root: &Node<'_, D>, patterns: &[Pattern]) -> bool {
    any_match(root, patterns, |_, _| true)
}

/// Whether any match of `patterns` under `root` passes `accept`, which gets
/// the index in `patterns` of the pattern that matched, found in one walk.
pub fn any_match<'r, D: Doc>(
    root: &Node<'r, D>,
    patterns: &[Pattern],
    accept: impl Fn(usize, &NodeMatch<'r, D>) -> bool,
) -> bool {
    let could_match = kind_filter(patterns);
    root.dfs().any(|node| {
        patterns.iter().enumerate().any(|(index, pattern)| {
            could_match(index, node.kind_id())
                && pattern
                    .match_node(node.clone())
                    .is_some_and(|m| accept(index, &m))
        })
    })
}

/// Every match of each of `patterns` under `root`, labeled `file`, from one
/// walk: one list per pattern in `patterns` order, each in tree order, the
/// same lists one `find_all` per pattern returns.
pub fn locations_each<D: Doc>(
    root: &Node<'_, D>,
    patterns: &[Pattern],
    file: &str,
) -> Vec<Vec<SourceLocation>> {
    let could_match = kind_filter(patterns);
    let mut found: Vec<Vec<SourceLocation>> = patterns.iter().map(|_| Vec::new()).collect();
    for node in root.dfs() {
        for (index, (pattern, out)) in patterns.iter().zip(found.iter_mut()).enumerate() {
            if !could_match(index, node.kind_id()) {
                continue;
            }
            if let Some(m) = pattern.match_node(node.clone()) {
                let pos = m.start_pos();
                out.push(SourceLocation {
                    file: file.to_string(),
                    line: pos.line() + 1,
                    column: pos.column(&m) + 1,
                    text: m.text().to_string(),
                });
            }
        }
    }
    found
}

/// Whether a node of kind `kind` could match the `index`th of `patterns`,
/// the node-kind prefilter `find_all` applies.
fn kind_filter(patterns: &[Pattern]) -> impl Fn(usize, u16) -> bool {
    let kinds: Vec<_> = patterns.iter().map(Matcher::potential_kinds).collect();
    move |index, kind| {
        kinds
            .get(index)
            .and_then(Option::as_ref)
            .is_none_or(|kinds| kinds.contains(kind.into()))
    }
}

#[cfg(test)]
mod tests {
    use ast_grep_core::tree_sitter::LanguageExt;
    use ast_grep_language::Rust;

    use super::*;

    fn rust_locations(source: &str, patterns: &[&str]) -> Vec<Vec<SourceLocation>> {
        let patterns: Vec<Pattern> = patterns.iter().map(|p| Pattern::new(p, Rust)).collect();
        locations_each(&Rust.ast_grep(source).root(), &patterns, "src/main.rs")
    }

    #[test]
    fn locations_each_finds_every_unwrap_call() {
        let source = r#"
fn main() {
    let x = foo().unwrap();
    let y = bar()?;
    let z = baz().unwrap();
}
"#;
        let found = rust_locations(source, &["$RECV.unwrap()"]);
        assert_eq!(found[0].len(), 2);
        assert!(found[0].iter().all(|m| m.text.contains("unwrap")));
        assert_eq!(
            (found[0][0].file.as_str(), found[0][0].line),
            ("src/main.rs", 3)
        );
    }

    #[test]
    fn locations_each_skips_comments_and_strings() {
        let source = r#"
fn main() {
    // foo().unwrap();
    let msg = "call .unwrap() to panic";
    let x = bar()?;
}
"#;
        assert!(rust_locations(source, &["$RECV.unwrap()"])[0].is_empty());
    }

    #[test]
    fn locations_each_groups_matches_by_pattern_in_tree_order() {
        let source = "fn f() { a.unwrap(); b.expect(\"x\"); c.unwrap(); }";
        let found = rust_locations(source, &["$R.expect($M)", "$R.unwrap()"]);
        let texts: Vec<Vec<&str>> = found
            .iter()
            .map(|matches| matches.iter().map(|m| m.text.as_str()).collect())
            .collect();
        assert_eq!(
            texts,
            [vec!["b.expect(\"x\")"], vec!["a.unwrap()", "c.unwrap()"]]
        );
    }

    #[test]
    fn has_any_reports_whether_any_pattern_matches() {
        let patterns = [
            Pattern::new("$RECV.unwrap()", Rust),
            Pattern::new("$RECV.expect($MSG)", Rust),
        ];
        let tree = Rust.ast_grep("fn main() { let x = foo().expect(\"x\"); }");
        assert!(has_any(&tree.root(), &patterns));
        let tree = Rust.ast_grep("fn main() { let x = foo()?; }");
        assert!(!has_any(&tree.root(), &patterns));
    }

    #[test]
    fn any_match_accepts_only_matches_the_test_passes() {
        let patterns = [Pattern::new("$RECV.unwrap()", Rust)];
        let tree = Rust.ast_grep("fn main() { let x = foo().unwrap(); }");
        assert!(any_match(&tree.root(), &patterns, |index, m| {
            index == 0 && m.text().starts_with("foo")
        }));
        assert!(!any_match(&tree.root(), &patterns, |_, m| m
            .text()
            .starts_with("bar")));
    }

    #[test]
    fn evidence_preview_keeps_short_text_without_ellipsis() {
        assert_eq!(evidence_preview("short", 80), "short");
    }

    #[test]
    fn evidence_preview_at_exact_budget_has_no_ellipsis() {
        let text = "x".repeat(80);
        assert_eq!(evidence_preview(&text, 80), text);
    }

    #[test]
    fn evidence_preview_collapses_newlines_to_spaces() {
        assert_eq!(evidence_preview("a\nb\nc", 80), "a b c");
    }

    #[test]
    fn evidence_preview_appends_exactly_one_ellipsis_when_truncating() {
        let text = "x".repeat(81);
        let preview = evidence_preview(&text, 80);
        assert_eq!(preview, format!("{}...", "x".repeat(80)));
        assert_eq!(preview.matches("...").count(), 1);
    }

    #[test]
    fn evidence_preview_bounds_by_characters_not_bytes() {
        let text = "⠋".repeat(100);
        let preview = evidence_preview(&text, 80);
        assert_eq!(preview, format!("{}...", "⠋".repeat(80)));
        assert_eq!(preview.chars().count(), 83);
    }

    #[test]
    fn evidence_preview_never_panics_mid_glyph() {
        for glyph in ["é", "⠋", "名", "🏆"] {
            let text = format!("for x in {} {{ }}", glyph.repeat(40));
            for budget in 0..text.len() + 1 {
                let preview = evidence_preview(&text, budget);
                assert!(preview.chars().count() <= budget + 3);
            }
        }
    }
}
