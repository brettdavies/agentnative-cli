//! Audit: Detect raw integer literals in `process::exit()` calls.
//!
//! Principle: P4 (Actionable Errors) — Exit codes should use named constants
//! so their meaning is self-documenting and stable across versions.
//!
//! Violation: `process::exit(1)` — a raw integer literal.
//! Allowed: `process::exit(EXIT_SUCCESS)` — a named constant.

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::locations_each;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, SourceLocation};

static EXIT_CALLS: LazyLock<[Pattern; 2]> = LazyLock::new(|| {
    [
        Pattern::new("process::exit($CODE)", Rust),
        Pattern::new("std::process::exit($CODE)", Rust),
    ]
});

/// Audit trait implementation for raw exit code detection.
#[derive(Debug)]
pub struct ExitCodesAudit;

impl Audit for ExitCodesAudit {
    fn id(&self) -> &str {
        "p4-exit-codes"
    }

    fn label(&self) -> &'static str {
        "Exit codes use named constants"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P4
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-must-exit-code-mapping"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let mut all_evidence = Vec::new();

        for (_, status) in pass::findings(project, Language::Rust, Rust, self) {
            if let AuditStatus::Warn(evidence) = status {
                all_evidence.push(evidence);
            }
        }

        let status = if all_evidence.is_empty() {
            AuditStatus::Pass
        } else {
            AuditStatus::Warn(all_evidence.join("\n"))
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

impl SourceScan<Rust> for ExitCodesAudit {
    type Finding = AuditStatus;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> AuditStatus {
        audit_exit_codes(file)
    }
}

/// Audit one file for raw integer exit codes.
pub(crate) fn audit_exit_codes(file: &SourceFile<'_, Rust>) -> AuditStatus {
    let violations = find_raw_exit_codes(file);

    if violations.is_empty() {
        AuditStatus::Pass
    } else {
        let evidence = violations
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        AuditStatus::Warn(evidence)
    }
}

/// Find `process::exit()` calls that use raw integer literals.
fn find_raw_exit_codes(file: &SourceFile<'_, Rust>) -> Vec<SourceLocation> {
    locations_each(&file.root(), &*EXIT_CALLS, &file.label())
        .into_iter()
        .flatten()
        .filter(|m| exits_with_raw_literal(&m.text))
        .collect()
}

/// Whether the exit call `text` passes a raw integer literal.
fn exits_with_raw_literal(text: &str) -> bool {
    let (Some(start), Some(end)) = (text.rfind('('), text.rfind(')')) else {
        return false;
    };
    #[expect(
        clippy::string_slice,
        reason = "start and end come from rfind on the ASCII parens"
    )]
    let arg = text[start + 1..end].trim();
    // A raw integer literal is all digits (possibly with a leading minus)
    !arg.is_empty()
        && arg
            .strip_prefix('-')
            .unwrap_or(arg)
            .chars()
            .all(|c| c.is_ascii_digit())
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
        let status = audit_exit_codes(&pass::rust(source, "src/main.rs"));
        assert_eq!(status, AuditStatus::Pass);
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
        let status = audit_exit_codes(&pass::rust(source, "src/main.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_exit_uses_raw_literal() {
        let source = r#"
use std::process;

fn main() {
    process::exit(1);
}
"#;
        let status = audit_exit_codes(&pass::rust(source, "src/main.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
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
        let status = audit_exit_codes(&pass::rust(source, "src/lib.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
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
        let status = audit_exit_codes(&pass::rust(source, "src/main.rs"));
        if let AuditStatus::Warn(evidence) = &status {
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
        let status = audit_exit_codes(&pass::rust(source, "src/main.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let audit = ExitCodesAudit;
        let dir = std::env::temp_dir().join(format!("anc-exitcodes-rust-{}", std::process::id()));
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
        let audit = ExitCodesAudit;
        let dir = std::env::temp_dir().join(format!("anc-exitcodes-py-{}", std::process::id()));
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
