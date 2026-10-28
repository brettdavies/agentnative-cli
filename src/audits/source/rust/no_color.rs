//! Audit: Detect NO_COLOR environment variable handling.
//!
//! Maps to: audit-p6-no-color from the existing 24 bash audits.
//! Principle: P6 (Composable Structure) — CLIs must respect NO_COLOR.
//! See <https://no-color.org/>
//!
//! This audit verifies the source references `NO_COLOR` as an env var.
//! It spans source + behavioral layers; this is the source half.

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::any_match;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// A clap `#[arg(...)]` attribute, which references NO_COLOR only when its
/// text names it, then patterns any match of which references it. The
/// single-quoted form parses to an error node and matches one in the target
/// (`f('NO_COLOR')`).
static NO_COLOR_PATTERNS: LazyLock<[Pattern; 7]> = LazyLock::new(|| {
    [
        Pattern::new("#[arg($$$ARGS)]", Rust),
        Pattern::new(r#"std::env::var("NO_COLOR")"#, Rust),
        Pattern::new(r#"env::var("NO_COLOR")"#, Rust),
        Pattern::new(r#"std::env::var_os("NO_COLOR")"#, Rust),
        Pattern::new(r#"env::var_os("NO_COLOR")"#, Rust),
        Pattern::new(r#""NO_COLOR""#, Rust),
        Pattern::new("'NO_COLOR'", Rust),
    ]
});

/// Audit trait implementation for NO_COLOR detection.
#[derive(Debug)]
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
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let found_any = pass::findings(project, Language::Rust, Rust, self)
            .iter()
            .any(|(_, found)| *found);

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
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

impl SourceScan<Rust> for NoColorSourceAudit {
    type Finding = bool;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> bool {
        references_no_color(file)
    }
}

/// Whether one file references NO_COLOR as an env var, a clap `env`
/// attribute, or a string literal.
fn references_no_color(file: &SourceFile<'_, Rust>) -> bool {
    any_match(&file.root(), &*NO_COLOR_PATTERNS, |index, m| {
        index > 0 || m.text().contains("NO_COLOR")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The verdict one file gets on its own.
    fn audit_no_color(file: &SourceFile<'_, Rust>) -> AuditStatus {
        if references_no_color(file) {
            AuditStatus::Pass
        } else {
            AuditStatus::Fail(format!(
                "{}: No reference to NO_COLOR found. CLIs must respect the NO_COLOR convention. \
                 See https://no-color.org/",
                file.label()
            ))
        }
    }

    #[test]
    fn pass_with_std_env_var() {
        let source = r#"
fn setup_color() {
    if std::env::var("NO_COLOR").is_ok() {
        disable_color();
    }
}
"#;
        let status = audit_no_color(&pass::rust(source, "src/output.rs"));
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
        let status = audit_no_color(&pass::rust(source, "src/output.rs"));
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
        let status = audit_no_color(&pass::rust(source, "src/cli.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn fail_when_no_color_absent() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let status = audit_no_color(&pass::rust(source, "src/main.rs"));
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
        let status = audit_no_color(&pass::rust(source, "src/color.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn run_passes_when_only_the_last_file_references_no_color() {
        let dir = std::env::temp_dir().join(format!("anc-nocolor-last-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        std::fs::write(dir.join("src/a.rs"), "fn main() {}\n").expect("write test a.rs");
        std::fs::write(
            dir.join("src/z.rs"),
            "fn color() -> bool {\n    std::env::var(\"NO_COLOR\").is_err()\n}\n",
        )
        .expect("write test z.rs");
        let project = Project::discover(&dir).expect("discover test project");

        let result = NoColorSourceAudit.run(&project).expect("run audit");

        assert_eq!(result.status, AuditStatus::Pass);
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
