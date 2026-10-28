//! Audit: Detect `process::exit()` calls in non-main files.
//!
//! Principle: P4 (Actionable Errors) — `process::exit()` should only appear
//! in main.rs. Library code should return errors, not terminate the process.

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::locations_each;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

static PATTERNS: LazyLock<[Pattern; 2]> = LazyLock::new(|| {
    [
        Pattern::new("process::exit($CODE)", Rust),
        Pattern::new("std::process::exit($CODE)", Rust),
    ]
});

/// Audit trait implementation for process::exit location detection.
#[derive(Debug)]
pub struct ProcessExitAudit;

impl Audit for ProcessExitAudit {
    fn id(&self) -> &str {
        "p4-process-exit"
    }

    fn label(&self) -> &'static str {
        "No process::exit outside main"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P4
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
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
            if let AuditStatus::Fail(evidence) = status {
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
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

impl SourceScan<Rust> for ProcessExitAudit {
    type Finding = AuditStatus;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> AuditStatus {
        if file
            .path()
            .file_name()
            .is_some_and(|name| name == "main.rs")
        {
            return AuditStatus::Pass;
        }
        audit_process_exit(file)
    }
}

/// Audit one file for `process::exit()` calls.
pub(crate) fn audit_process_exit(file: &SourceFile<'_, Rust>) -> AuditStatus {
    let all_matches: Vec<_> = locations_each(&file.root(), &*PATTERNS, &file.label())
        .into_iter()
        .flatten()
        .collect();

    if all_matches.is_empty() {
        AuditStatus::Pass
    } else {
        let evidence = all_matches
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        AuditStatus::Fail(evidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_no_process_exit() {
        let source = r#"
fn handle_error(e: Error) -> Result<()> {
    Err(e.into())
}
"#;
        let status = audit_process_exit(&pass::rust(source, "src/lib.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn fail_when_process_exit_in_lib() {
        let source = r#"
use std::process;

fn bail(msg: &str) {
    eprintln!("{msg}");
    process::exit(1);
}
"#;
        let status = audit_process_exit(&pass::rust(source, "src/lib.rs"));
        assert!(matches!(status, AuditStatus::Fail(_)));
        if let AuditStatus::Fail(evidence) = &status {
            assert!(evidence.contains("process::exit"));
            assert!(evidence.contains("src/lib.rs"));
        }
    }

    #[test]
    fn fail_with_std_process_exit_in_lib() {
        let source = r#"
fn bail() {
    std::process::exit(1);
}
"#;
        let status = audit_process_exit(&pass::rust(source, "src/util.rs"));
        assert!(matches!(status, AuditStatus::Fail(_)));
        if let AuditStatus::Fail(evidence) = &status {
            assert!(evidence.contains("std::process::exit"));
        }
    }

    #[test]
    fn scan_exempts_only_files_named_main_rs() {
        let source = "fn stop() {\n    std::process::exit(1);\n}\n";
        let scan = |file| ProcessExitAudit.scan(&pass::rust(source, file));
        assert!(matches!(scan("src/domain.rs"), AuditStatus::Fail(_)));
        assert!(matches!(scan("src/remain.rs"), AuditStatus::Fail(_)));
        assert_eq!(scan("src/main.rs"), AuditStatus::Pass);
        assert_eq!(scan("src/bin/main.rs"), AuditStatus::Pass);
    }

    #[test]
    fn run_skips_main_rs() {
        // The scan skips main.rs, but the inner function does not.
        // This test verifies the inner function finds the call.
        let source = r#"
use std::process;

fn main() {
    process::exit(0);
}
"#;
        // Inner function always reports — it's the scan that filters main.rs
        let status = audit_process_exit(&pass::rust(source, "src/main.rs"));
        assert!(matches!(status, AuditStatus::Fail(_)));
    }

    #[test]
    fn fail_counts_multiple_exits() {
        let source = r#"
use std::process;

fn bail_a() {
    process::exit(1);
}

fn bail_b() {
    process::exit(2);
}
"#;
        let status = audit_process_exit(&pass::rust(source, "src/commands.rs"));
        if let AuditStatus::Fail(evidence) = &status {
            assert_eq!(evidence.lines().count(), 2);
        } else {
            panic!("Expected Fail");
        }
    }

    #[test]
    fn applicable_for_rust() {
        let audit = ProcessExitAudit;
        let dir = std::env::temp_dir().join(format!("anc-procexit-rust-{}", std::process::id()));
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
        let audit = ProcessExitAudit;
        let dir = std::env::temp_dir().join(format!("anc-procexit-py-{}", std::process::id()));
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
