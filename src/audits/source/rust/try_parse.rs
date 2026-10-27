//! Audit: Detect `.parse().unwrap()` patterns.
//!
//! Principle: P4 (Actionable Errors) — Parsing user input should use
//! proper error handling, not panicking unwraps. Prefer `?` or `match`.
//!
//! Only flags `.parse().unwrap()` — `.parse().expect()` is not flagged because
//! the developer has already considered the error case and provided a message.

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::locations_each;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

static PATTERNS: LazyLock<[Pattern; 1]> =
    LazyLock::new(|| [Pattern::new("$RECV.parse().unwrap()", Rust)]);

/// Audit trait implementation for parse-unwrap detection.
#[derive(Debug)]
pub struct TryParseAudit;

impl Audit for TryParseAudit {
    fn id(&self) -> &str {
        "p4-try-parse"
    }

    fn label(&self) -> &'static str {
        "No .parse().unwrap()"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P4
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-must-try-parse"]
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

impl SourceScan<Rust> for TryParseAudit {
    type Finding = AuditStatus;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> AuditStatus {
        audit_try_parse(file)
    }
}

/// Audit one file for `.parse().unwrap()`.
pub(crate) fn audit_try_parse(file: &SourceFile<'_, Rust>) -> AuditStatus {
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
        AuditStatus::Warn(evidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_no_parse_unwrap() {
        let source = r#"
fn parse_port(s: &str) -> Result<u16, std::num::ParseIntError> {
    s.parse()
}
"#;
        let status = audit_try_parse(&pass::rust(source, "src/config.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn pass_with_question_mark() {
        let source = r#"
fn parse_port(s: &str) -> anyhow::Result<u16> {
    let port: u16 = s.parse()?;
    Ok(port)
}
"#;
        let status = audit_try_parse(&pass::rust(source, "src/config.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn warn_with_parse_unwrap() {
        let source = r#"
fn main() {
    let port: u16 = args[1].parse().unwrap();
}
"#;
        let status = audit_try_parse(&pass::rust(source, "src/main.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
            assert!(evidence.contains("parse().unwrap()"));
            assert!(evidence.contains("src/main.rs"));
        }
    }

    #[test]
    fn pass_with_parse_expect() {
        // .parse().expect() is acceptable — the developer already considered the error
        let source = r#"
fn main() {
    let port: u16 = args[1].parse().expect("bad port");
}
"#;
        let status = audit_try_parse(&pass::rust(source, "src/main.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn warn_counts_multiple() {
        let source = r#"
fn main() {
    let port: u16 = args[1].parse().unwrap();
    let count: usize = args[2].parse().unwrap();
    let timeout: u64 = args[3].parse().expect("bad timeout");
}
"#;
        let status = audit_try_parse(&pass::rust(source, "src/main.rs"));
        if let AuditStatus::Warn(evidence) = &status {
            // Only .parse().unwrap() lines are flagged, not .expect()
            assert_eq!(evidence.lines().count(), 2);
        } else {
            panic!("Expected Warn");
        }
    }

    #[test]
    fn pass_with_match_on_parse() {
        let source = r#"
fn parse_port(s: &str) -> u16 {
    match s.parse::<u16>() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Invalid port: {e}");
            std::process::exit(1);
        }
    }
}
"#;
        let status = audit_try_parse(&pass::rust(source, "src/config.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let audit = TryParseAudit;
        let dir = std::env::temp_dir().join(format!("anc-tryparse-rust-{}", std::process::id()));
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
        let audit = TryParseAudit;
        let dir = std::env::temp_dir().join(format!("anc-tryparse-py-{}", std::process::id()));
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
