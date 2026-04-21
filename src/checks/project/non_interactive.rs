//! Check: Detect prompt library dependencies in Cargo.toml.
//!
//! Principle: P1 (Non-Interactive) — CLIs should not depend on interactive prompt
//! libraries like dialoguer, inquire, rustyline, or crossterm.

use std::fs;

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Interactive prompt libraries that conflict with agent-native operation.
const PROMPT_LIBS: &[&str] = &["dialoguer", "inquire", "rustyline", "crossterm"];

pub struct NonInteractiveSourceCheck;

impl Check for NonInteractiveSourceCheck {
    fn id(&self) -> &str {
        "p1-non-interactive-source"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P1
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Project
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-no-interactive"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.path.is_dir()
            && project.language == Some(Language::Rust)
            && project.manifest_path.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let manifest_path = project
            .manifest_path
            .as_ref()
            .expect("manifest_path must exist when applicable() returns true");

        let content = fs::read_to_string(manifest_path)?;

        let found: Vec<&str> = PROMPT_LIBS
            .iter()
            .filter(|lib| content.contains(**lib))
            .copied()
            .collect();

        let status = if found.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(format!(
                "Interactive prompt libraries detected: {}",
                found.join(", ")
            ))
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "No interactive prompt dependencies".into(),
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
            "anc-nonint-{suffix}-{}-{}",
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
        assert!(NonInteractiveSourceCheck.applicable(&project));
    }

    #[test]
    fn not_applicable_without_manifest() {
        let dir = temp_dir("no-manifest");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!NonInteractiveSourceCheck.applicable(&project));
    }

    #[test]
    fn not_applicable_for_python() {
        let dir = temp_dir("python");
        fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test pyproject.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!NonInteractiveSourceCheck.applicable(&project));
    }

    #[test]
    fn pass_when_no_prompt_libs() {
        let dir = temp_dir("pass");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
clap = "4"
serde = "1"
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = NonInteractiveSourceCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_dialoguer_present() {
        let dir = temp_dir("dialoguer");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
clap = "4"
dialoguer = "0.11"
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = NonInteractiveSourceCheck.run(&project).expect("run check");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("dialoguer"));
        }
    }

    #[test]
    fn warn_with_multiple_prompt_libs() {
        let dir = temp_dir("multi");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
inquire = "0.7"
rustyline = "14"
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = NonInteractiveSourceCheck.run(&project).expect("run check");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("inquire"));
            assert!(evidence.contains("rustyline"));
        }
    }

    #[test]
    fn metadata_is_correct() {
        let check = NonInteractiveSourceCheck;
        assert_eq!(check.id(), "p1-non-interactive-source");
        assert_eq!(check.group(), CheckGroup::P1);
        assert_eq!(check.layer(), CheckLayer::Project);
    }
}
