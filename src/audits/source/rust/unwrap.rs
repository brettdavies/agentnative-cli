//! Audit: Detect `.unwrap()` calls in Rust source.
//!
//! Maps to: audit-code-unwrap from the existing 24 bash audits.
//! Principle: P4 (Actionable Errors) — CLIs should handle errors explicitly.

use crate::audit::Audit;
use crate::project::{Language, Project};
use crate::source::find_pattern_matches;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const PATTERN: &str = "$RECV.unwrap()";

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
            if let AuditStatus::Fail(evidence) = audit_unwrap(&parsed_file.source, &file_str) {
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
/// Kept public(crate) for unit testing with inline source strings.
pub(crate) fn audit_unwrap(source: &str, file: &str) -> AuditStatus {
    let mut matches = find_pattern_matches(source, PATTERN);
    for m in &mut matches {
        m.file = file.to_string();
    }

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
        let status = audit_unwrap(source, "src/main.rs");
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn fail_when_unwrap_present() {
        let source = r#"
fn main() {
    let config = load_config().unwrap();
}
"#;
        let status = audit_unwrap(source, "src/main.rs");
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
        let status = audit_unwrap(source, "src/lib.rs");
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
    // Previously: config.unwrap()
    let config = load_config()?;
    Ok(())
}
"#;
        let status = audit_unwrap(source, "src/main.rs");
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn ignores_unwrap_in_strings() {
        let source = r#"
fn main() -> anyhow::Result<()> {
    eprintln!("Don't use .unwrap() in production");
    Ok(())
}
"#;
        let status = audit_unwrap(source, "src/main.rs");
        assert_eq!(status, AuditStatus::Pass);
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
