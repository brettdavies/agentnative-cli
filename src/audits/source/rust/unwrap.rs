//! Audit: Detect `.unwrap()` calls in Rust source.
//!
//! Maps to: audit-code-unwrap from the existing 24 bash audits.
//! Principle: P4 (Actionable Errors) — CLIs should handle errors explicitly.
//!
//! `.unwrap()` calls inside `#[cfg(test)]`-gated items (inline `mod tests`,
//! gated helper functions, gated `impl` blocks, etc.) are exempt by default,
//! matching the convention that test code may panic on assertion failure. The
//! exemption is lifted when `Project::include_tests` is true (the
//! `--include-tests` CLI flag), restoring the original behavior so callers can
//! audit test code on demand.

use ast_grep_core::Node;
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::project::{Language, Project};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, SourceLocation};

/// Audit trait implementation for unwrap detection.
pub struct UnwrapAudit;

impl Audit for UnwrapAudit {
    fn id(&self) -> &str {
        "code-unwrap"
    }

    fn label(&self) -> &'static str {
        "No .unwrap() in source"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::CodeQuality
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let parsed = project.parsed_files();
        let mut all_evidence = Vec::new();

        for (path, parsed_file) in parsed.iter() {
            let file_str = path.display().to_string();
            if let AuditStatus::Fail(evidence) =
                audit_unwrap_with(&parsed_file.source, &file_str, project.include_tests)
            {
                all_evidence.push(evidence);
            }
        }

        let status = if all_evidence.is_empty() {
            AuditStatus::Pass
        } else {
            AuditStatus::Fail(all_evidence.join("\n"))
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// Audit a single source string for `.unwrap()` calls.
///
/// When `include_cfg_test` is false (the default), calls inside any
/// `#[cfg(test)]`-gated item are exempt. When true, every `.unwrap()` is
/// flagged regardless of cfg gating.
pub(crate) fn audit_unwrap_with(source: &str, file: &str, include_cfg_test: bool) -> AuditStatus {
    let root = Rust.ast_grep(source);
    let mut matches = Vec::new();
    walk(root.root(), file, false, include_cfg_test, &mut matches);

    if matches.is_empty() {
        AuditStatus::Pass
    } else {
        let evidence = matches
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        AuditStatus::Fail(evidence)
    }
}

fn walk<'a>(
    node: Node<'a, StrDoc<Rust>>,
    file: &str,
    inside_cfg_test: bool,
    include_cfg_test: bool,
    out: &mut Vec<SourceLocation>,
) {
    if (!inside_cfg_test || include_cfg_test)
        && let Some(snippet) = unwrap_call_snippet(&node)
    {
        let pos = node.start_pos();
        out.push(SourceLocation {
            file: file.to_string(),
            line: pos.line() + 1,
            column: pos.column(&node) + 1,
            text: snippet,
        });
    }

    // tree-sitter-rust models `#[cfg(test)]` as an `attribute_item` *sibling*
    // that precedes the item it decorates (not a child of it). Walk children
    // sequentially and propagate "next sibling is cfg(test)-gated" state.
    let mut next_is_cfg_test = false;
    for child in node.children() {
        let kind = child.kind();
        if kind.as_ref() == "attribute_item" || kind.as_ref() == "inner_attribute_item" {
            // An inner attribute (`#![cfg(test)]` at the head of a mod body)
            // gates the enclosing item — i.e. *every* sibling of the inner
            // attribute inherits the gate. Conservatively, treat
            // `inner_attribute_item` as if it gated all following siblings
            // here, which covers the canonical position at the top of a mod.
            if attribute_text_is_cfg_test(child.text().as_ref()) {
                next_is_cfg_test = true;
            }
            walk(child, file, inside_cfg_test, include_cfg_test, out);
            continue;
        }
        let child_inside = inside_cfg_test || (next_is_cfg_test && is_item_like(kind.as_ref()));
        // The one-shot gate fires on the very next sibling regardless of kind:
        // `#[cfg(test)] use foo;` consumes the flag without gating any later
        // sibling, since `use_declaration` is not item-like in our taxonomy.
        next_is_cfg_test = false;
        walk(child, file, child_inside, include_cfg_test, out);
    }
}

/// Match a `call_expression` whose receiver chain ends in `.unwrap()`.
///
/// Returns the matched snippet (the call text) for evidence reporting.
fn unwrap_call_snippet<'a>(node: &Node<'a, StrDoc<Rust>>) -> Option<String> {
    if node.kind() != "call_expression" {
        return None;
    }
    let text = node.text();
    let trimmed = text.trim_end();
    if !trimmed.ends_with(".unwrap()") {
        return None;
    }
    Some(trimmed.lines().next().unwrap_or(trimmed).trim().to_string())
}

/// Item-like node kinds whose preceding `#[cfg(test)]` attribute gates their
/// bodies. Mirrors tree-sitter-rust's item nodes.
const ITEM_KINDS: &[&str] = &[
    "mod_item",
    "function_item",
    "function_signature_item",
    "impl_item",
    "struct_item",
    "enum_item",
    "union_item",
    "trait_item",
    "const_item",
    "static_item",
    "type_item",
    "macro_definition",
];

fn is_item_like(kind: &str) -> bool {
    ITEM_KINDS.contains(&kind)
}

/// Strip `#[ ... ]` / `#![ ... ]` framing and check whether the inner attribute
/// is a `cfg(...)` invocation whose argument tree mentions a bare `test`
/// identifier.
fn attribute_text_is_cfg_test(attr: &str) -> bool {
    let trimmed = attr.trim();
    let body = trimmed
        .strip_prefix("#![")
        .or_else(|| trimmed.strip_prefix("#["))
        .and_then(|s| s.strip_suffix(']'))
        .map(str::trim);
    let Some(body) = body else { return false };

    let Some(rest) = body.strip_prefix("cfg") else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(args) = rest.strip_prefix('(').and_then(|s| s.strip_suffix(')')) else {
        return false;
    };
    cfg_args_contain_test(args)
}

/// Walk the textual argument list of a `cfg(...)` attribute looking for a bare
/// `test` predicate. Skips string literals (so `feature = "test"` is rejected)
/// and only matches full identifiers (so `testing_only` / `test_helper` /
/// `cfg(unix)` are rejected). Handles arbitrary nesting under `any(...)`,
/// `all(...)`, and `not(...)`.
fn cfg_args_contain_test(args: &str) -> bool {
    let bytes = args.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            // Skip string literal, accounting for escapes.
            i += 1;
            while i < bytes.len() {
                let c = bytes[i];
                if c == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if c == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = &args[start..i];
            // After the identifier, skip whitespace to see what follows.
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let next = bytes.get(j).copied();
            if ident == "test" {
                // Bare `test` predicate: must be followed by a separator
                // (`,`, `)`, or end-of-args), not `=` (would make it a key)
                // and not `(` (would make it a nested call, e.g. `test(...)`).
                if next.is_none() || matches!(next, Some(b',') | Some(b')')) {
                    return true;
                }
            } else if (ident == "any" || ident == "all" || ident == "not")
                && next == Some(b'(')
                && let Some(inner) = balanced_parens(&args[j..])
            {
                if cfg_args_contain_test(inner) {
                    return true;
                }
                i = j + inner.len() + 2; // skip past matching ')'
                continue;
            }
            // For all other identifiers (including `feature`, `testing_only`,
            // `cfg`, `unix`), keep scanning — they don't gate test code.
            continue;
        }
        i += 1;
    }
    false
}

/// Given a string that starts with `(`, return the slice between the opening
/// paren and the matching closing paren, respecting nested parens and string
/// literals. Returns `None` if no balanced match exists.
fn balanced_parens(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.first().copied() != Some(b'(') {
        return None;
    }
    let mut depth: usize = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            i += 1;
            while i < bytes.len() {
                let c = bytes[i];
                if c == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if c == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b == b'(' {
            depth += 1;
        } else if b == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(&s[1..i]);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_no_unwrap() {
        let source = r#"
fn main() -> anyhow::Result<()> {
    let config = load_config()?;
    let data = fetch_data()?;
    Ok(())
}
"#;
        let status = audit_unwrap_with(source, "src/main.rs", false);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn fail_when_unwrap_present() {
        let source = r#"
fn main() {
    let config = load_config().unwrap();
}
"#;
        let status = audit_unwrap_with(source, "src/main.rs", false);
        assert!(matches!(status, AuditStatus::Fail(_)));
        if let AuditStatus::Fail(evidence) = &status {
            assert!(evidence.contains("unwrap"));
            assert!(evidence.contains("src/main.rs"));
        }
    }

    #[test]
    fn fail_counts_multiple_unwraps() {
        let source = r#"
fn main() {
    let a = foo().unwrap();
    let b = bar().unwrap();
    let c = baz().unwrap();
}
"#;
        let status = audit_unwrap_with(source, "src/lib.rs", false);
        if let AuditStatus::Fail(evidence) = &status {
            assert_eq!(evidence.lines().count(), 3);
        } else {
            panic!("Expected Fail");
        }
    }

    #[test]
    fn ignores_unwrap_in_comments() {
        let source = r#"
fn main() -> anyhow::Result<()> {
    // load_config().unwrap()
    let config = load_config()?;
    Ok(())
}
"#;
        let status = audit_unwrap_with(source, "src/main.rs", false);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn ignores_unwrap_in_strings() {
        let source = r#"
fn main() -> anyhow::Result<()> {
    eprintln!("do not call .unwrap() in production");
    Ok(())
}
"#;
        let status = audit_unwrap_with(source, "src/main.rs", false);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn exempts_inline_cfg_test_mod_and_helpers() {
        // One unwrap outside a test gate + three inside `#[cfg(test)] mod tests`
        // + one inside `#[cfg(test)] fn helper()`. Default audit flags only the
        // outside one; `--include-tests` flags all five.
        let source = r#"
fn run() {
    let v = compute().unwrap();
}

#[cfg(test)]
mod tests {
    fn a() { foo().unwrap(); }
    fn b() { bar().unwrap(); }
    fn c() { baz().unwrap(); }
}

#[cfg(test)]
fn helper() {
    qux().unwrap();
}
"#;
        let default_status = audit_unwrap_with(source, "src/lib.rs", false);
        if let AuditStatus::Fail(evidence) = &default_status {
            assert_eq!(
                evidence.lines().count(),
                1,
                "default should flag only the production-code unwrap: {evidence}"
            );
            assert!(evidence.contains("compute().unwrap()"));
        } else {
            panic!("expected Fail with one line, got {default_status:?}");
        }

        let include_status = audit_unwrap_with(source, "src/lib.rs", true);
        if let AuditStatus::Fail(evidence) = &include_status {
            assert_eq!(
                evidence.lines().count(),
                5,
                "--include-tests should flag every unwrap: {evidence}"
            );
        } else {
            panic!("expected Fail with five lines, got {include_status:?}");
        }
    }

    #[test]
    fn exempts_nested_cfg_test_mods() {
        let source = r#"
#[cfg(test)]
mod outer {
    #[cfg(test)]
    mod inner {
        fn t() { foo().unwrap(); }
    }
}
"#;
        let status = audit_unwrap_with(source, "src/lib.rs", false);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn cfg_test_use_does_not_gate_following_items() {
        // `#[cfg(test)] use foo::bar;` is a use statement; only items gate.
        // The `.unwrap()` below sits in production code and must flag.
        let source = r#"
#[cfg(test)]
use foo::bar;

fn run() {
    bar().unwrap();
}
"#;
        let status = audit_unwrap_with(source, "src/lib.rs", false);
        if let AuditStatus::Fail(evidence) = &status {
            assert_eq!(evidence.lines().count(), 1);
            assert!(evidence.contains("bar().unwrap()"));
        } else {
            panic!("expected Fail, got {status:?}");
        }
    }

    #[test]
    fn exempts_cfg_any_test_feature() {
        let source = r#"
#[cfg(any(test, feature = "x"))]
mod tests {
    fn t() { foo().unwrap(); }
}
"#;
        let status = audit_unwrap_with(source, "src/lib.rs", false);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn exempts_cfg_all_test_unix() {
        let source = r#"
#[cfg(all(test, unix))]
mod tests {
    fn t() { foo().unwrap(); }
}
"#;
        let status = audit_unwrap_with(source, "src/lib.rs", false);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn feature_named_test_string_does_not_gate() {
        // `feature = "test"` is a feature-name literal, not the test cfg flag.
        // The unwrap inside must still flag.
        let source = r#"
#[cfg(feature = "test")]
fn x() {
    foo().unwrap();
}
"#;
        let status = audit_unwrap_with(source, "src/lib.rs", false);
        if let AuditStatus::Fail(evidence) = &status {
            assert_eq!(evidence.lines().count(), 1);
            assert!(evidence.contains("foo().unwrap()"));
        } else {
            panic!("expected Fail, got {status:?}");
        }
    }

    #[test]
    fn similarly_named_cfg_identifiers_do_not_gate() {
        // `test_helper` is its own identifier; only a bare `test` gates.
        let source = r#"
#[cfg(test_helper)]
fn x() {
    foo().unwrap();
}
"#;
        let status = audit_unwrap_with(source, "src/lib.rs", false);
        if let AuditStatus::Fail(evidence) = &status {
            assert_eq!(evidence.lines().count(), 1);
            assert!(evidence.contains("foo().unwrap()"));
        } else {
            panic!("expected Fail, got {status:?}");
        }
    }

    #[test]
    fn cfg_unix_does_not_gate_unwrap() {
        let source = r#"
#[cfg(unix)]
fn x() {
    foo().unwrap();
}
"#;
        let status = audit_unwrap_with(source, "src/lib.rs", false);
        if let AuditStatus::Fail(evidence) = &status {
            assert_eq!(evidence.lines().count(), 1);
        } else {
            panic!("expected Fail, got {status:?}");
        }
    }

    #[test]
    fn cfg_test_impl_block_gates_unwrap() {
        let source = r#"
struct S;

#[cfg(test)]
impl S {
    fn t(&self) {
        foo().unwrap();
    }
}
"#;
        let status = audit_unwrap_with(source, "src/lib.rs", false);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn include_tests_flag_disables_exemption() {
        // Regression guard for U1's contract: every cfg(test)-gated unwrap
        // re-emerges when --include-tests is on.
        let source = r#"
#[cfg(test)]
mod tests {
    fn a() { foo().unwrap(); }
    fn b() { bar().unwrap(); }
}

#[cfg(test)]
fn helper() {
    baz().unwrap();
}
"#;
        let default = audit_unwrap_with(source, "src/lib.rs", false);
        assert_eq!(default, AuditStatus::Pass);

        let included = audit_unwrap_with(source, "src/lib.rs", true);
        if let AuditStatus::Fail(evidence) = &included {
            assert_eq!(evidence.lines().count(), 3);
        } else {
            panic!("expected Fail with --include-tests, got {included:?}");
        }
    }

    #[test]
    fn applicable_for_rust() {
        let audit = UnwrapAudit;
        let dir = std::env::temp_dir().join(format!("anc-unwrap-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(audit.applicable(&project));
    }

    #[test]
    fn not_applicable_for_python() {
        let audit = UnwrapAudit;
        let dir = std::env::temp_dir().join(format!("anc-unwrap-py-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test pyproject.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }

    #[test]
    fn not_applicable_for_none() {
        let audit = UnwrapAudit;
        let dir = std::env::temp_dir().join(format!("anc-unwrap-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }
}
