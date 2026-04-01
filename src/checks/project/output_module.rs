//! Check: Detect a centralized output/format module in the project source tree.
//!
//! Principle: P2 (Structured Output) — Projects should have a centralized output
//! formatting module (e.g., src/output.rs, src/format.rs, src/display.rs).

use crate::check::Check;
use crate::project::Project;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

/// File names that indicate a centralized output module.
const OUTPUT_CANDIDATES: &[&str] = &[
    "output.rs",
    "format.rs",
    "display.rs",
    "output.py",
    "format.py",
    "display.py",
];

pub struct OutputModuleCheck;

impl Check for OutputModuleCheck {
    fn id(&self) -> &str {
        "p2-output-module"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P2
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Project
    }

    fn applicable(&self, project: &Project) -> bool {
        project.path.is_dir() && project.language.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let src_dir = project.path.join("src");

        for candidate in OUTPUT_CANDIDATES {
            if src_dir.join(candidate).exists() {
                return Ok(CheckResult {
                    id: self.id().to_string(),
                    label: "Centralized output module exists".into(),
                    group: self.group(),
                    layer: self.layer(),
                    status: CheckStatus::Pass,
                });
            }
        }

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Centralized output module exists".into(),
            group: self.group(),
            layer: self.layer(),
            status: CheckStatus::Warn(
                "No centralized output module found (expected src/output.rs, src/format.rs, or src/display.rs)".into(),
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anc-outmod-{suffix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after UNIX epoch")
                .as_nanos(),
        ));
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn applicable_when_language_detected() {
        let dir = temp_dir("applicable");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(OutputModuleCheck.applicable(&project));
    }

    #[test]
    fn not_applicable_without_language() {
        let dir = temp_dir("no-lang");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!OutputModuleCheck.applicable(&project));
    }

    #[test]
    fn pass_with_output_rs() {
        let dir = temp_dir("output-rs");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src dir");
        fs::write(src.join("output.rs"), "pub fn emit() {}").expect("write output.rs");
        let project = Project::discover(&dir).expect("discover test project");
        let result = OutputModuleCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn pass_with_format_rs() {
        let dir = temp_dir("format-rs");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src dir");
        fs::write(src.join("format.rs"), "pub fn format_output() {}").expect("write format.rs");
        let project = Project::discover(&dir).expect("discover test project");
        let result = OutputModuleCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn pass_with_display_rs() {
        let dir = temp_dir("display-rs");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src dir");
        fs::write(src.join("display.rs"), "pub fn show() {}").expect("write display.rs");
        let project = Project::discover(&dir).expect("discover test project");
        let result = OutputModuleCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_no_output_module() {
        let dir = temp_dir("no-output");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src dir");
        fs::write(src.join("main.rs"), "fn main() {}").expect("write main.rs");
        let project = Project::discover(&dir).expect("discover test project");
        let result = OutputModuleCheck.run(&project).expect("run check");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
    }

    #[test]
    fn warn_when_no_src_dir() {
        let dir = temp_dir("no-src");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        let result = OutputModuleCheck.run(&project).expect("run check");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
    }

    #[test]
    fn metadata_is_correct() {
        let check = OutputModuleCheck;
        assert_eq!(check.id(), "p2-output-module");
        assert_eq!(check.group(), CheckGroup::P2);
        assert_eq!(check.layer(), CheckLayer::Project);
    }
}
