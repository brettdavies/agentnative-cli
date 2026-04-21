//! Check: Detect a dedicated error module in the project source tree.
//!
//! Principle: P4 (Actionable Errors) — Projects should have a centralized error
//! module (e.g., src/error.rs, src/errors.rs) for consistent error handling.

use std::fs;

use crate::check::Check;
use crate::project::Project;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct ErrorModuleCheck;

impl Check for ErrorModuleCheck {
    fn id(&self) -> &str {
        "p4-error-module"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P4
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Project
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-should-structured-enum"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.path.is_dir() && project.language.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let src_dir = project.path.join("src");

        // Check direct files: src/error.rs, src/errors.rs
        let direct_candidates = ["error.rs", "errors.rs", "error.py", "errors.py"];
        for candidate in &direct_candidates {
            if src_dir.join(candidate).exists() {
                return Ok(CheckResult {
                    id: self.id().to_string(),
                    label: "Dedicated error module exists".into(),
                    group: self.group(),
                    layer: self.layer(),
                    status: CheckStatus::Pass,
                    confidence: Confidence::High,
                });
            }
        }

        // Check subdirectories: src/*/error.rs, src/*/errors.rs
        if src_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&src_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        for name in &["error.rs", "errors.rs", "error.py", "errors.py"] {
                            if path.join(name).exists() {
                                return Ok(CheckResult {
                                    id: self.id().to_string(),
                                    label: "Dedicated error module exists".into(),
                                    group: self.group(),
                                    layer: self.layer(),
                                    status: CheckStatus::Pass,
                                    confidence: Confidence::High,
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Dedicated error module exists".into(),
            group: self.group(),
            layer: self.layer(),
            status: CheckStatus::Warn(
                "No dedicated error module found (expected src/error.rs or src/errors.rs)".into(),
            ),
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
            "anc-errmod-{suffix}-{}-{}",
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
        assert!(ErrorModuleCheck.applicable(&project));
    }

    #[test]
    fn not_applicable_without_language() {
        let dir = temp_dir("no-lang");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!ErrorModuleCheck.applicable(&project));
    }

    #[test]
    fn pass_with_error_rs() {
        let dir = temp_dir("error-rs");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src dir");
        fs::write(src.join("error.rs"), "pub enum AppError {}").expect("write error.rs");
        let project = Project::discover(&dir).expect("discover test project");
        let result = ErrorModuleCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn pass_with_errors_rs() {
        let dir = temp_dir("errors-rs");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src dir");
        fs::write(src.join("errors.rs"), "pub enum AppError {}").expect("write errors.rs");
        let project = Project::discover(&dir).expect("discover test project");
        let result = ErrorModuleCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn pass_with_nested_error_module() {
        let dir = temp_dir("nested");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let submod = dir.join("src").join("core");
        fs::create_dir_all(&submod).expect("create submod dir");
        fs::write(submod.join("error.rs"), "pub enum CoreError {}").expect("write error.rs");
        let project = Project::discover(&dir).expect("discover test project");
        let result = ErrorModuleCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_no_error_module() {
        let dir = temp_dir("no-error");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("create src dir");
        fs::write(src.join("main.rs"), "fn main() {}").expect("write main.rs");
        let project = Project::discover(&dir).expect("discover test project");
        let result = ErrorModuleCheck.run(&project).expect("run check");
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
        let result = ErrorModuleCheck.run(&project).expect("run check");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
    }

    #[test]
    fn metadata_is_correct() {
        let check = ErrorModuleCheck;
        assert_eq!(check.id(), "p4-error-module");
        assert_eq!(check.group(), CheckGroup::P4);
        assert_eq!(check.layer(), CheckLayer::Project);
    }
}
