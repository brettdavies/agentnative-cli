//! Audit: Detect a dedicated error module in the project source tree.
//!
//! Principle: P4 (Actionable Errors) — Projects should have a centralized error
//! module (e.g., src/error.rs, src/errors.rs, or a src/error/ directory module)
//! for consistent error handling.

use std::fs;

use crate::audit::Audit;
use crate::project::Project;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Paths, relative to a source directory, that hold an error module: a file
/// module, or a directory module (Rust `mod.rs`, a Python package).
const MODULE_PATHS: &[&str] = &[
    "error.rs",
    "errors.rs",
    "error/mod.rs",
    "errors/mod.rs",
    "error.py",
    "errors.py",
    "error/__init__.py",
    "errors/__init__.py",
];

#[derive(Debug)]
pub struct ErrorModuleAudit;

impl Audit for ErrorModuleAudit {
    fn id(&self) -> &str {
        "p4-error-module"
    }

    fn label(&self) -> &'static str {
        "Dedicated error module exists"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P4
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Project
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-should-structured-enum"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.path.is_dir() && project.language.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let package_root = project
            .manifest_path
            .as_deref()
            .and_then(std::path::Path::parent)
            .unwrap_or(&project.path);
        let src_dir = package_root.join("src");

        let has_module = |dir: &std::path::Path| MODULE_PATHS.iter().any(|p| dir.join(p).exists());

        let found = has_module(&src_dir)
            || (src_dir.is_dir()
                && fs::read_dir(&src_dir).is_ok_and(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.path())
                        .any(|path| path.is_dir() && has_module(&path))
                }));
        if found {
            return Ok(AuditResult {
                id: self.id().to_string(),
                label: self.label().into(),
                group: self.group(),
                layer: self.layer(),
                status: AuditStatus::Pass,
                confidence: Confidence::High,
                mitigation: None,
                config_hint: None,
                pass_evidence: None,
            });
        }

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status: AuditStatus::Warn(format!(
                "No dedicated error module found in src/ or one directory below it (looked for {})",
                MODULE_PATHS.join(", ")
            )),
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
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
        assert!(ErrorModuleAudit.applicable(&project));
    }

    #[test]
    fn not_applicable_without_language() {
        let dir = temp_dir("no-lang");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!ErrorModuleAudit.applicable(&project));
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
        let result = ErrorModuleAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
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
        let result = ErrorModuleAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
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
        let result = ErrorModuleAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn warning_names_every_path_it_looked_for() {
        let dir = temp_dir("no-module");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        fs::create_dir_all(dir.join("src")).expect("create src dir");
        let project = Project::discover(&dir).expect("discover test project");
        let result = ErrorModuleAudit.run(&project).expect("run audit");
        assert_eq!(
            result.status,
            AuditStatus::Warn(
                "No dedicated error module found in src/ or one directory below it (looked for \
                 error.rs, errors.rs, error/mod.rs, errors/mod.rs, error.py, errors.py, \
                 error/__init__.py, errors/__init__.py)"
                    .into()
            )
        );
    }

    #[test]
    fn pass_with_error_directory_module() {
        let dir = temp_dir("error-dir");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let module = dir.join("src").join("error");
        fs::create_dir_all(&module).expect("create src/error dir");
        fs::write(module.join("mod.rs"), "pub enum AppError {}").expect("write mod.rs");
        let project = Project::discover(&dir).expect("discover test project");
        let result = ErrorModuleAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn pass_with_errors_python_package() {
        let dir = temp_dir("errors-pkg");
        fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write pyproject.toml");
        let package = dir.join("src").join("errors");
        fs::create_dir_all(&package).expect("create src/errors dir");
        fs::write(
            package.join("__init__.py"),
            "class AppError(Exception): ...",
        )
        .expect("write __init__.py");
        let project = Project::discover(&dir).expect("discover test project");
        let result = ErrorModuleAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
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
        let result = ErrorModuleAudit.run(&project).expect("run audit");
        assert!(matches!(result.status, AuditStatus::Warn(_)));
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
        let result = ErrorModuleAudit.run(&project).expect("run audit");
        assert!(matches!(result.status, AuditStatus::Warn(_)));
    }

    #[test]
    fn metadata_is_correct() {
        let audit = ErrorModuleAudit;
        assert_eq!(audit.id(), "p4-error-module");
        assert_eq!(audit.group(), AuditGroup::P4);
        assert_eq!(audit.layer(), AuditLayer::Project);
    }
}
