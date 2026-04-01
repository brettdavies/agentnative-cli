//! Check: Detect structured error types in Rust source.
//!
//! Principle: P4 (Actionable Errors) — CLIs should define structured error
//! enums so callers (and agents) can distinguish error categories.
//!
//! This is a presence check: pass if any enum with "Error" in its name exists.

use ast_grep_core::Pattern;
use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::Rust;

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

/// Check trait implementation for structured error type detection.
pub struct ErrorTypesCheck;

impl Check for ErrorTypesCheck {
    fn id(&self) -> &str {
        "p4-error-types"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P4
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut found = false;

        for (_path, parsed_file) in parsed.iter() {
            if has_error_enum(&parsed_file.source) {
                found = true;
                break;
            }
        }

        let status = if found {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(
                "No structured error enum found. Consider defining an enum with \"Error\" in \
                 its name so callers can distinguish error categories programmatically."
                    .to_string(),
            )
        };

        Ok(CheckResult {
            id: "p4-error-types".to_string(),
            label: "Structured error types".to_string(),
            group: CheckGroup::P4,
            layer: CheckLayer::Source,
            status,
        })
    }
}

/// Check whether a source string contains an enum whose name includes "Error".
pub(crate) fn has_error_enum(source: &str) -> bool {
    let pattern = match Pattern::try_new("enum $NAME { $$$BODY }", Rust) {
        Ok(p) => p,
        Err(_) => return false,
    };

    let root = Rust.ast_grep(source);
    for m in root.root().find_all(&pattern) {
        let text = m.text();
        // Extract the enum name: first line after "enum " up to the next space or "{"
        if let Some(name) = text
            .strip_prefix("enum ")
            .and_then(|rest| rest.split_whitespace().next())
        {
            if name.contains("Error") {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_with_error_enum() {
        let source = r#"
#[derive(Debug)]
enum AppError {
    NotFound,
    InvalidInput(String),
    Io(std::io::Error),
}
"#;
        assert!(has_error_enum(source));
    }

    #[test]
    fn pass_with_custom_error_name() {
        let source = r#"
enum CliError {
    BadArg,
    MissingFile,
}
"#;
        assert!(has_error_enum(source));
    }

    #[test]
    fn warn_when_no_error_enum() {
        let source = r#"
enum Command {
    Check,
    Lint,
}

fn main() {
    println!("Hello");
}
"#;
        assert!(!has_error_enum(source));
    }

    #[test]
    fn warn_when_no_enums_at_all() {
        let source = r#"
fn main() {
    println!("Hello");
}
"#;
        assert!(!has_error_enum(source));
    }

    #[test]
    fn pass_with_error_suffix() {
        let source = r#"
enum ParseError {
    UnexpectedToken,
    Eof,
}
"#;
        assert!(has_error_enum(source));
    }

    #[test]
    fn applicable_for_rust() {
        let check = ErrorTypesCheck;
        let dir = std::env::temp_dir().join(format!("anc-errtypes-rust-{}", std::process::id()));
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
        let check = ErrorTypesCheck;
        let dir = std::env::temp_dir().join(format!("anc-errtypes-py-{}", std::process::id()));
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
