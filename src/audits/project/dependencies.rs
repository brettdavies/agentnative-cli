//! Audit: Verify recommended dependencies for Rust projects.
//!
//! Principle: P6 (Composable Structure) — Rust CLI projects should use standard
//! ecosystem crates for errors (anyhow/thiserror), CLI parsing (clap), and
//! serialization (serde).

use std::fs;

use crate::audit::Audit;
use crate::project::{Language, Project};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Recommended dependency groups: (description, alternatives).
/// A group passes if any alternative is present.
const RECOMMENDED_DEPS: &[(&str, &[&str])] = &[
    (
        "error handling (anyhow or thiserror)",
        &["anyhow", "thiserror"],
    ),
    ("CLI parsing (clap)", &["clap"]),
    ("serialization (serde)", &["serde"]),
];

pub struct DependenciesAudit;

impl Audit for DependenciesAudit {
    fn id(&self) -> &str {
        "p6-dependencies"
    }

    fn label(&self) -> &'static str {
        "Recommended dependencies present"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Project
    }

    fn applicable(&self, project: &Project) -> bool {
        project.path.is_dir()
            && project.language == Some(Language::Rust)
            && project.manifest_path.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let manifest_path = project
            .manifest_path
            .as_ref()
            .expect("manifest_path must exist when applicable() returns true");

        let content = fs::read_to_string(manifest_path)?;

        let missing: Vec<&str> = RECOMMENDED_DEPS
            .iter()
            .filter(|(_, alternatives)| !alternatives.iter().any(|dep| content.contains(dep)))
            .map(|(desc, _)| *desc)
            .collect();

        let status = if missing.is_empty() {
            AuditStatus::Pass
        } else {
            AuditStatus::Warn(format!(
                "Missing recommended dependencies: {}",
                missing.join("; ")
            ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anc-deps-{suffix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after UNIX epoch")
                .as_nanos(),
        ));
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    fn write_cargo_toml(dir: &std::path::Path, content: &str) {
        fs::write(dir.join("Cargo.toml"), content).expect("write test Cargo.toml");
    }

    #[test]
    fn applicable_for_rust_with_manifest() {
        let dir = temp_dir("applicable");
        write_cargo_toml(&dir, "[package]\nname = \"test\"\nversion = \"0.1.0\"\n");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(DependenciesAudit.applicable(&project));
    }

    #[test]
    fn not_applicable_without_manifest() {
        let dir = temp_dir("no-manifest");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!DependenciesAudit.applicable(&project));
    }

    #[test]
    fn not_applicable_for_python() {
        let dir = temp_dir("python");
        fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write pyproject.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!DependenciesAudit.applicable(&project));
    }

    #[test]
    fn pass_when_all_deps_present() {
        let dir = temp_dir("pass");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = DependenciesAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn pass_with_thiserror_instead_of_anyhow() {
        let dir = temp_dir("thiserror");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
thiserror = "2"
clap = "4"
serde = "1"
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = DependenciesAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_missing_some_deps() {
        let dir = temp_dir("missing");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
clap = "4"
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = DependenciesAudit.run(&project).expect("run audit");
        assert!(matches!(result.status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("error handling"));
            assert!(evidence.contains("serde"));
            assert!(!evidence.contains("clap"));
        }
    }

    #[test]
    fn warn_when_all_deps_missing() {
        let dir = temp_dir("all-missing");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
tokio = "1"
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = DependenciesAudit.run(&project).expect("run audit");
        assert!(matches!(result.status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("error handling"));
            assert!(evidence.contains("CLI parsing"));
            assert!(evidence.contains("serialization"));
        }
    }

    #[test]
    fn metadata_is_correct() {
        let audit = DependenciesAudit;
        assert_eq!(audit.id(), "p6-dependencies");
        assert_eq!(audit.group(), AuditGroup::P6);
        assert_eq!(audit.layer(), AuditLayer::Project);
    }
}
