use ast_grep_core::Pattern;
use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::{Python, Rust};

use crate::project::Language;
use crate::types::SourceLocation;

/// Check whether a Rust source string contains at least one match for the given pattern.
pub fn has_pattern(source: &str, pattern_str: &str) -> bool {
    has_pattern_with(source, pattern_str, Rust)
}

/// Parse a Rust source file and find all matches for a pattern.
pub fn find_pattern_matches(source: &str, pattern_str: &str) -> Vec<SourceLocation> {
    find_pattern_matches_with(source, pattern_str, Rust)
}

/// Check whether `source` contains at least one match for `pattern_str` in `lang`.
pub fn has_pattern_in(source: &str, pattern_str: &str, lang: Language) -> bool {
    match lang {
        Language::Rust => has_pattern_with(source, pattern_str, Rust),
        Language::Python => has_pattern_with(source, pattern_str, Python),
        _ => false,
    }
}

/// Find all matches for `pattern_str` in `source`, parsed as `lang`.
///
/// Currently used by tests; kept as a symmetric counterpart to `has_pattern_in`
/// for future Python checks that need evidence locations rather than a boolean.
#[allow(dead_code)]
pub fn find_pattern_matches_in(
    source: &str,
    pattern_str: &str,
    lang: Language,
) -> Vec<SourceLocation> {
    match lang {
        Language::Rust => find_pattern_matches_with(source, pattern_str, Rust),
        Language::Python => find_pattern_matches_with(source, pattern_str, Python),
        _ => vec![],
    }
}

fn has_pattern_with<L>(source: &str, pattern_str: &str, lang: L) -> bool
where
    L: LanguageExt + Copy,
{
    let pattern = match Pattern::try_new(pattern_str, lang) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = lang.ast_grep(source);
    root.root().find(&pattern).is_some()
}

fn find_pattern_matches_with<L>(source: &str, pattern_str: &str, lang: L) -> Vec<SourceLocation>
where
    L: LanguageExt + Copy,
{
    let pattern = match Pattern::try_new(pattern_str, lang) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let root = lang.ast_grep(source);
    root.root()
        .find_all(&pattern)
        .map(|m| {
            let pos = m.start_pos();
            SourceLocation {
                file: String::new(),
                line: pos.line() + 1,
                column: pos.column(&m) + 1,
                text: m.text().to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_unwrap_calls() {
        let source = r#"
fn main() {
    let x = foo().unwrap();
    let y = bar()?;
    let z = baz().unwrap();
}
"#;
        let matches = find_pattern_matches(source, "$RECV.unwrap()");
        assert_eq!(matches.len(), 2);
        assert!(matches[0].text.contains("unwrap"));
        assert!(matches[1].text.contains("unwrap"));
    }

    #[test]
    fn test_no_false_positives_in_comments() {
        let source = r#"
fn main() {
    // foo().unwrap();
    let x = bar()?;
}
"#;
        let matches = find_pattern_matches(source, "$RECV.unwrap()");
        assert_eq!(matches.len(), 0, "Should not match inside comments");
    }

    #[test]
    fn test_no_false_positives_in_strings() {
        let source = r#"
fn main() {
    let msg = "call .unwrap() to panic";
    let x = bar()?;
}
"#;
        let matches = find_pattern_matches(source, "$RECV.unwrap()");
        assert_eq!(matches.len(), 0, "Should not match inside strings");
    }

    #[test]
    fn test_invalid_pattern_returns_empty() {
        let matches = find_pattern_matches("fn main() {}", "<<<invalid>>>");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_has_pattern_found() {
        let source = "fn main() { let x = foo().unwrap(); }";
        assert!(has_pattern(source, "$RECV.unwrap()"));
    }

    #[test]
    fn test_has_pattern_not_found() {
        let source = "fn main() { let x = foo()?; }";
        assert!(!has_pattern(source, "$RECV.unwrap()"));
    }

    #[test]
    fn test_has_pattern_invalid_pattern() {
        assert!(!has_pattern("fn main() {}", "<<<invalid>>>"));
    }

    #[test]
    fn test_python_bare_except_matches() {
        let source = "try:\n    foo()\nexcept:\n    pass\n";
        let matches = find_pattern_matches_in(
            source,
            "try:\n    $$$BODY\nexcept:\n    $$$HANDLER",
            Language::Python,
        );
        assert!(!matches.is_empty(), "bare except pattern should match");
    }

    #[test]
    fn test_python_typed_except_does_not_match_bare() {
        let source = "try:\n    foo()\nexcept ValueError:\n    pass\n";
        let bare_matches = find_pattern_matches_in(
            source,
            "try:\n    $$$BODY\nexcept:\n    $$$HANDLER",
            Language::Python,
        );
        assert!(
            bare_matches.is_empty(),
            "typed except should not match bare except pattern"
        );
    }

    #[test]
    fn test_python_sys_exit_matches() {
        let source = "import sys\nsys.exit(1)\n";
        let matches = find_pattern_matches_in(source, "sys.exit($$$ARGS)", Language::Python);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_python_pattern_against_rust_source_returns_empty() {
        let source = "fn main() { sys.exit(1); }";
        // Rust-shaped source parsed as Python should yield no matches for our Python pattern.
        let matches = find_pattern_matches_in(source, "fn main() { $$$BODY }", Language::Python);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_unsupported_language_returns_empty() {
        let source = "package main\nfunc main() {}";
        assert!(find_pattern_matches_in(source, "anything", Language::Go).is_empty());
        assert!(!has_pattern_in(source, "anything", Language::Node));
    }
}
