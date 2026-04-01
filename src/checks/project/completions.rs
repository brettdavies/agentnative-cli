//! Check: Detect clap_complete dependency when clap is present.
//!
//! Principle: P6 (Composable Structure) — CLIs built with clap should also include
//! clap_complete for shell completion generation.

use std::fs;

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

pub struct CompletionsCheck;

impl Check for CompletionsCheck {
    fn id(&self) -> &str {
        "p6-completions"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Project
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

        let has_clap = content.contains("clap");
        let has_clap_complete =
            content.contains("clap_complete") || content.contains("clap-complete");

        let status = if !has_clap {
            CheckStatus::Skip("No clap dependency detected".into())
        } else if has_clap_complete {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn("clap detected but clap_complete is missing".into())
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Shell completions support".into(),
            group: self.group(),
            layer: self.layer(),
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anc-completions-{suffix}-{}-{}",
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
        assert!(CompletionsCheck.applicable(&project));
    }

    #[test]
    fn not_applicable_without_manifest() {
        let dir = temp_dir("no-manifest");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!CompletionsCheck.applicable(&project));
    }

    #[test]
    fn skip_when_no_clap() {
        let dir = temp_dir("no-clap");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
serde = "1"
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = CompletionsCheck.run(&project).expect("run check");
        assert!(matches!(result.status, CheckStatus::Skip(_)));
    }

    #[test]
    fn pass_when_clap_complete_present() {
        let dir = temp_dir("pass");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
clap = { version = "4", features = ["derive"] }
clap_complete = "4"
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = CompletionsCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_clap_without_complete() {
        let dir = temp_dir("warn");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
clap = { version = "4", features = ["derive"] }
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = CompletionsCheck.run(&project).expect("run check");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("clap_complete"));
        }
    }

    #[test]
    fn pass_with_hyphenated_clap_complete() {
        let dir = temp_dir("hyphen");
        write_cargo_toml(
            &dir,
            r#"[package]
name = "myapp"
version = "0.1.0"

[dependencies]
clap = "4"
clap-complete = "4"
"#,
        );
        let project = Project::discover(&dir).expect("discover test project");
        let result = CompletionsCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn metadata_is_correct() {
        let check = CompletionsCheck;
        assert_eq!(check.id(), "p6-completions");
        assert_eq!(check.group(), CheckGroup::P6);
        assert_eq!(check.layer(), CheckLayer::Project);
    }
}
