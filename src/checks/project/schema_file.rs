//! Check: `p2-should-schema-file`.
//!
//! Output schemas are exported to a stable file path so CI / static-analysis
//! consumers pin without invoking the tool. Canonical shapes:
//! `schema/<command>.json`, `schemas/`, or any top-level `*.schema.json`.
//!
//! SHOULD-tier: emit Warn (not Fail) on absence. Project-layer file existence
//! check, applicable to any directory-shaped project.

use crate::check::Check;
use crate::project::Project;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct SchemaFileCheck;

impl Check for SchemaFileCheck {
    fn id(&self) -> &str {
        "p2-schema-file"
    }

    fn label(&self) -> &'static str {
        "Output schema exported to a stable file path"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P2
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Project
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-should-schema-file"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.path.is_dir()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = check_schema_file(&project.path);

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// Core unit for tests. Inspects the project root for canonical schema-file
/// shapes. Pass when any are found; Warn otherwise.
pub(crate) fn check_schema_file(root: &std::path::Path) -> CheckStatus {
    if root.join("schema").is_dir() || root.join("schemas").is_dir() {
        return CheckStatus::Pass;
    }

    // Top-level *.schema.json — read_dir is bounded to the project root, no
    // recursive walk needed for SHOULD-tier coverage.
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && name.ends_with(".schema.json")
            {
                return CheckStatus::Pass;
            }
        }
    }

    CheckStatus::Warn(
        "no schema files found at project root (`schema/`, `schemas/`, or \
         `*.schema.json`). CI consumers cannot pin against the output shape \
         without invoking the tool."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anc-schema-file-{suffix}-{}-{}",
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
    fn happy_path_schema_dir() {
        let dir = temp_dir("schemadir");
        fs::create_dir_all(dir.join("schema")).expect("mkdir schema");
        assert_eq!(check_schema_file(&dir), CheckStatus::Pass);
    }

    #[test]
    fn happy_path_schemas_dir() {
        let dir = temp_dir("schemasdir");
        fs::create_dir_all(dir.join("schemas")).expect("mkdir schemas");
        assert_eq!(check_schema_file(&dir), CheckStatus::Pass);
    }

    #[test]
    fn happy_path_top_level_schema_json() {
        let dir = temp_dir("toplevel");
        fs::write(dir.join("output.schema.json"), "{}").expect("write schema");
        assert_eq!(check_schema_file(&dir), CheckStatus::Pass);
    }

    #[test]
    fn warn_no_schema_files() {
        let dir = temp_dir("warn");
        fs::write(dir.join("README.md"), "# Tool\n").expect("write readme");
        match check_schema_file(&dir) {
            CheckStatus::Warn(msg) => assert!(msg.contains("schema")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
