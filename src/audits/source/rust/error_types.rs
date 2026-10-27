//! Audit: Detect structured error types in Rust source.
//!
//! Principle: P4 (Actionable Errors) — CLIs should define structured error
//! enums so callers (and agents) can distinguish error categories.
//!
//! This is a presence audit: pass if any enum with "Error" in its name exists.

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::any_match;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Enum definitions with each visibility the audit recognizes.
static ENUM_PATTERNS: LazyLock<[Pattern; 3]> = LazyLock::new(|| {
    [
        Pattern::new("enum $NAME { $$$BODY }", Rust),
        Pattern::new("pub enum $NAME { $$$BODY }", Rust),
        Pattern::new("pub(crate) enum $NAME { $$$BODY }", Rust),
    ]
});

/// Audit trait implementation for structured error type detection.
#[derive(Debug)]
pub struct ErrorTypesAudit;

impl Audit for ErrorTypesAudit {
    fn id(&self) -> &str {
        "p4-error-types"
    }

    fn label(&self) -> &'static str {
        "Structured error types"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P4
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-should-structured-enum"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let found = pass::findings(project, Language::Rust, Rust, self)
            .iter()
            .any(|(_, found)| *found);

        let status = if found {
            AuditStatus::Pass
        } else {
            AuditStatus::Warn(
                "No structured error enum found. Consider defining an enum with \"Error\" in \
                 its name so callers can distinguish error categories programmatically."
                    .to_string(),
            )
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

impl SourceScan<Rust> for ErrorTypesAudit {
    type Finding = bool;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> bool {
        has_error_enum(file)
    }
}

/// Whether one file defines an enum whose name includes "Error", with or
/// without a visibility modifier.
pub(crate) fn has_error_enum(file: &SourceFile<'_, Rust>) -> bool {
    any_match(&file.root(), &*ENUM_PATTERNS, |_, m| {
        names_an_error(&m.text())
    })
}

/// Whether the enum definition `text` names an error type.
fn names_an_error(text: &str) -> bool {
    let Some(enum_pos) = text.find("enum ") else {
        return false;
    };
    #[expect(
        clippy::string_slice,
        reason = "enum_pos comes from find(\"enum \") and 5 is that ASCII needle's length"
    )]
    let after_enum = &text[enum_pos + 5..];
    after_enum
        .split_whitespace()
        .next()
        .is_some_and(|name| name.contains("Error"))
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
        assert!(has_error_enum(&pass::rust(source, "src/error.rs")));
    }

    #[test]
    fn pass_with_custom_error_name() {
        let source = r#"
enum CliError {
    BadArg,
    MissingFile,
}
"#;
        assert!(has_error_enum(&pass::rust(source, "src/error.rs")));
    }

    #[test]
    fn warn_when_no_error_enum() {
        let source = r#"
enum Command {
    Audit,
    Lint,
}

fn main() {
    println!("Hello");
}
"#;
        assert!(!has_error_enum(&pass::rust(source, "src/error.rs")));
    }

    #[test]
    fn warn_when_no_enums_at_all() {
        let source = r#"
fn main() {
    println!("Hello");
}
"#;
        assert!(!has_error_enum(&pass::rust(source, "src/error.rs")));
    }

    #[test]
    fn pass_with_error_suffix() {
        let source = r#"
enum ParseError {
    UnexpectedToken,
    Eof,
}
"#;
        assert!(has_error_enum(&pass::rust(source, "src/error.rs")));
    }

    #[test]
    fn run_passes_when_one_later_file_holds_the_error_enum() {
        let dir = std::env::temp_dir().join(format!("anc-errtypes-split-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        std::fs::write(dir.join("src/a.rs"), "fn main() {}\n").expect("write test a.rs");
        std::fs::write(
            dir.join("src/b.rs"),
            "pub enum CliError {\n    BadArg,\n}\n",
        )
        .expect("write test b.rs");
        std::fs::write(dir.join("src/c.rs"), "enum Command {\n    Run,\n}\n")
            .expect("write test c.rs");
        let project = Project::discover(&dir).expect("discover test project");

        let result = ErrorTypesAudit.run(&project).expect("run audit");

        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let audit = ErrorTypesAudit;
        let dir = std::env::temp_dir().join(format!("anc-errtypes-rust-{}", std::process::id()));
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
        let audit = ErrorTypesAudit;
        let dir = std::env::temp_dir().join(format!("anc-errtypes-py-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test pyproject.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }
}
