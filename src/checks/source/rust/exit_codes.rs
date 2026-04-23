//! Check: Detect raw integer literals in `process::exit()` calls.
//!
//! Principle: P4 (Actionable Errors) — Exit codes should use named constants
//! so their meaning is self-documenting and stable across versions.
//!
//! Violation: `process::exit(1)` — a raw integer literal.
//! Allowed: `process::exit(EXIT_SUCCESS)` — a named constant.

use ast_grep_core::Pattern;
use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::Rust;

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence, SourceLocation};

/// Check trait implementation for raw exit code detection.
pub struct ExitCodesCheck;

impl Check for ExitCodesCheck {
    fn id(&self) -> &str {
        "p4-exit-codes"
    }

    fn label(&self) -> &'static str {
        "Exit codes use named constants"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P4
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-must-exit-code-mapping"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut all_evidence = Vec::new();

        for (path, parsed_file) in parsed.iter() {
            let file_str = path.display().to_string();
            if let CheckStatus::Warn(evidence) = check_exit_codes(&parsed_file.source, &file_str) {
                all_evidence.push(evidence);
            }
        }

        let status = if all_evidence.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(all_evidence.join("\n"))
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// Check a single source string for raw integer exit codes.
///
/// Kept public(crate) for unit testing with inline source strings.
pub(crate) fn check_exit_codes(source: &str, file: &str) -> CheckStatus {
    let violations = find_raw_exit_codes(source, file);

    if violations.is_empty() {
        CheckStatus::Pass
    } else {
        let evidence = violations
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        CheckStatus::Warn(evidence)
    }
}

/// Find `process::exit()` calls that use raw integer literals.
fn find_raw_exit_codes(source: &str, file: &str) -> Vec<SourceLocation> {
    let patterns = ["process::exit($CODE)", "std::process::exit($CODE)"];

    let root = Rust.ast_grep(source);
    let root_node = root.root();
    let mut violations = Vec::new();

    for pattern_str in &patterns {
        let pattern = match Pattern::try_new(pattern_str, Rust) {
            Ok(p) => p,
            Err(_) => continue,
        };

        for m in root_node.find_all(&pattern) {
            let text = m.text().to_string();
            // Extract the argument inside exit(...)
            if let Some(start) = text.rfind('(') {
                if let Some(end) = text.rfind(')') {
                    let arg = text[start + 1..end].trim();
                    // A raw integer literal is all digits (possibly with a leading minus)
                    let is_raw_literal = !arg.is_empty()
                        && arg
                            .strip_prefix('-')
                            .unwrap_or(arg)
                            .chars()
                            .all(|c| c.is_ascii_digit());
                    if is_raw_literal {
                        let pos = m.start_pos();
                        violations.push(SourceLocation {
                            file: file.to_string(),
                            line: pos.line() + 1,
                            column: pos.column(&m) + 1,
                            text,
                        });
                    }
                }
            }
        }
    }

    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_no_process_exit() {
        let source = r#"
fn main() -> anyhow::Result<()> {
    let config = load_config()?;
    Ok(())
}
"#;
        let status = check_exit_codes(source, "src/main.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn pass_when_exit_uses_named_constant() {
        let source = r#"
use std::process;

const EXIT_SUCCESS: i32 = 0;
const EXIT_FAILURE: i32 = 1;

fn main() {
    if error {
        process::exit(EXIT_FAILURE);
    }
    process::exit(EXIT_SUCCESS);
}
"#;
        let status = check_exit_codes(source, "src/main.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_exit_uses_raw_literal() {
        let source = r#"
use std::process;

fn main() {
    process::exit(1);
}
"#;
        let status = check_exit_codes(source, "src/main.rs");
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
            assert!(evidence.contains("process::exit(1)"));
            assert!(evidence.contains("src/main.rs"));
        }
    }

    #[test]
    fn warn_when_std_process_exit_uses_raw_literal() {
        let source = r#"
fn bail() {
    std::process::exit(2);
}
"#;
        let status = check_exit_codes(source, "src/lib.rs");
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
            assert!(evidence.contains("std::process::exit(2)"));
        }
    }

    #[test]
    fn warn_counts_multiple_raw_exits() {
        let source = r#"
use std::process;

fn main() {
    if bad {
        process::exit(1);
    }
    process::exit(0);
}
"#;
        let status = check_exit_codes(source, "src/main.rs");
        if let CheckStatus::Warn(evidence) = &status {
            assert_eq!(evidence.lines().count(), 2);
        } else {
            panic!("Expected Warn");
        }
    }

    #[test]
    fn pass_with_exit_zero_named() {
        let source = r#"
use std::process;

fn main() {
    process::exit(code);
}
"#;
        let status = check_exit_codes(source, "src/main.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let check = ExitCodesCheck;
        let dir = std::env::temp_dir().join(format!("anc-exitcodes-rust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(check.applicable(&project));
    }

    #[test]
    fn not_applicable_for_python() {
        let check = ExitCodesCheck;
        let dir = std::env::temp_dir().join(format!("anc-exitcodes-py-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test pyproject.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
