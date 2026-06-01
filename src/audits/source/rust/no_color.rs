//! Audit: Detect NO_COLOR environment variable handling.
//!
//! Maps to: audit-p6-no-color from the existing 24 bash audits.
//! Principle: P6 (Composable Structure) — CLIs must respect NO_COLOR.
//! See https://no-color.org/
//!
//! This audit verifies the source references `NO_COLOR` as an env var.
//! It spans source + behavioral layers; this is the source half.

use ast_grep_core::Pattern;
use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::project::{Language, Project};
use crate::source::{has_pattern, has_string_literal_in};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Audit trait implementation for NO_COLOR detection.
pub struct NoColorSourceAudit;

impl Audit for NoColorSourceAudit {
    fn id(&self) -> &str {
        "p6-no-color"
    }

    fn label(&self) -> &'static str {
        "Respects NO_COLOR"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-must-no-color"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let parsed = project.parsed_files();
        let mut found_any = false;

        for (_path, parsed_file) in parsed.iter() {
            if matches!(audit_no_color(&parsed_file.source, ""), AuditStatus::Pass) {
                found_any = true;
                break;
            }
        }

        let status = if found_any {
            AuditStatus::Pass
        } else {
            AuditStatus::Fail(
                "No reference to NO_COLOR found in any source file. CLIs must respect the \
                 NO_COLOR convention. See https://no-color.org/"
                    .to_string(),
            )
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

/// Audit a single source string for NO_COLOR references.
///
/// Kept public(crate) for unit testing with inline source strings.
pub(crate) fn audit_no_color(source: &str, file: &str) -> AuditStatus {
    let found_env_var = has_pattern(source, r#"std::env::var("NO_COLOR")"#)
        || has_pattern(source, r#"env::var("NO_COLOR")"#)
        || has_pattern(source, r#"std::env::var_os("NO_COLOR")"#)
        || has_pattern(source, r#"env::var_os("NO_COLOR")"#);

    let found_clap_env = source_contains_no_color_clap_attr(source);

    let found_any = found_env_var
        || found_clap_env
        || has_string_literal_in(source, "NO_COLOR", Language::Rust);

    if found_any {
        AuditStatus::Pass
    } else {
        AuditStatus::Fail(format!(
            "{file}: No reference to NO_COLOR found. CLIs must respect the NO_COLOR convention. \
             See https://no-color.org/"
        ))
    }
}

/// Audit for clap attribute: `#[arg(env = "NO_COLOR")]`
fn source_contains_no_color_clap_attr(source: &str) -> bool {
    let pattern = match Pattern::try_new(r#"#[arg($$$ARGS)]"#, Rust) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = Rust.ast_grep(source);
    for m in root.root().find_all(&pattern) {
        if m.text().contains("NO_COLOR") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_with_std_env_var() {
        let source = r#"
fn setup_color() {
    if std::env::var("NO_COLOR").is_ok() {
        disable_color();
    }
}
"#;
        let status = audit_no_color(source, "src/output.rs");
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn pass_with_env_var_os() {
        let source = r#"
use std::env;

fn setup_color() {
    if env::var_os("NO_COLOR").is_some() {
        disable_color();
    }
}
"#;
        let status = audit_no_color(source, "src/output.rs");
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn pass_with_clap_env_attr() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long, env = "NO_COLOR")]
    no_color: bool,
}
"#;
        let status = audit_no_color(source, "src/cli.rs");
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn fail_when_no_color_absent() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let status = audit_no_color(source, "src/main.rs");
        assert!(matches!(status, AuditStatus::Fail(_)));
        if let AuditStatus::Fail(evidence) = &status {
            assert!(evidence.contains("NO_COLOR"));
            assert!(evidence.contains("no-color.org"));
        }
    }

    #[test]
    fn pass_with_string_literal_fallback() {
        let source = r#"
const COLOR_ENV: &str = "NO_COLOR";

fn audit_color() {
    std::env::var(COLOR_ENV).ok();
}
"#;
        let status = audit_no_color(source, "src/color.rs");
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let audit = NoColorSourceAudit;
        let dir = std::env::temp_dir().join(format!("anc-nocolor-rust-{}", std::process::id()));
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
        let audit = NoColorSourceAudit;
        let dir = std::env::temp_dir().join(format!("anc-nocolor-py-{}", std::process::id()));
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
