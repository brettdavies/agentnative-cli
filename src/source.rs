use std::path::Path;

use ast_grep_core::Pattern;
use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::Rust;

use crate::types::SourceLocation;

/// Check whether a Rust source string contains at least one match for the given pattern.
pub fn has_pattern(source: &str, pattern_str: &str) -> bool {
    let pattern = match Pattern::try_new(pattern_str, Rust) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = Rust.ast_grep(source);
    root.root().find(&pattern).is_some()
}

/// Parse a Rust source file and find all matches for a pattern.
pub fn find_pattern_matches(source: &str, pattern_str: &str) -> Vec<SourceLocation> {
    let pattern = match Pattern::try_new(pattern_str, Rust) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let root = Rust.ast_grep(source);
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

/// Scan all `.rs` files under a directory for pattern matches.
pub fn scan_rust_files(dir: &Path, pattern_str: &str) -> anyhow::Result<Vec<SourceLocation>> {
    let mut results = Vec::new();

    for entry in walkdir(dir)? {
        let path = entry;
        if path.extension().is_some_and(|e| e == "rs") {
            let source = std::fs::read_to_string(&path)?;
            let mut matches = find_pattern_matches(&source, pattern_str);
            for m in &mut matches {
                m.file = path.display().to_string();
            }
            results.extend(matches);
        }
    }

    Ok(results)
}

/// Simple recursive directory walk (no external dep needed).
fn walkdir(dir: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    if dir.is_file() {
        files.push(dir.to_path_buf());
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(walkdir(&path)?);
        } else {
            files.push(path);
        }
    }
    Ok(files)
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
}
