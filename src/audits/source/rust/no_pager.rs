//! Audit: Detect pager invocations without a `--no-pager` flag.
//!
//! Principle: P6 (Composable Structure) — Pagers block agents. If a CLI uses
//! a pager, it must offer `--no-pager` to disable it.
//!
//! This is a conditional audit:
//!   Trigger: source references pager crate or spawns less/more
//!   Requirement: a `--no-pager` flag must exist

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::has_any;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Pager-related patterns to detect in source text.
const PAGER_INDICATORS: &[&str] = &["pager::Pager", "Pager::new", "Pager::with_pager"];

static PAGER_COMMANDS: LazyLock<[Pattern; 2]> = LazyLock::new(|| {
    [
        Pattern::new(r#"Command::new("less")"#, Rust),
        Pattern::new(r#"Command::new("more")"#, Rust),
    ]
});

const MISSING_FLAG: &str = "Pager code detected but no --no-pager flag found. \
                            Pagers block agents; provide --no-pager to disable.";

/// Audit trait implementation for no-pager detection.
#[derive(Debug)]
pub struct NoPagerAudit;

impl Audit for NoPagerAudit {
    fn id(&self) -> &str {
        "p6-no-pager"
    }

    fn label(&self) -> &'static str {
        "No pager blocking agents"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-must-no-pager"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let files = pass::findings(project, Language::Rust, Rust, self);
        let has_pager = files.iter().any(|(_, file)| file.pager_code);
        let has_no_pager_flag = files
            .iter()
            .any(|(_, file)| file.pager_code && file.no_pager_flag);

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status: verdict(has_pager, has_no_pager_flag),
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

impl SourceScan<Rust> for NoPagerAudit {
    type Finding = PagerUse;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> PagerUse {
        pager_use(file)
    }
}

/// What one file shows about pagers.
#[derive(Debug, Clone, Copy)]
pub struct PagerUse {
    pager_code: bool,
    no_pager_flag: bool,
}

fn pager_use(file: &SourceFile<'_, Rust>) -> PagerUse {
    let source = file.source();
    let pager_code = PAGER_INDICATORS.iter().any(|p| source.contains(p))
        || source.contains("use pager")
        || source.contains("extern crate pager")
        || has_any(&file.root(), &*PAGER_COMMANDS);
    PagerUse {
        pager_code,
        no_pager_flag: source.contains("no-pager") || source.contains("no_pager"),
    }
}

fn verdict(has_pager: bool, has_no_pager_flag: bool) -> AuditStatus {
    if !has_pager || has_no_pager_flag {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(MISSING_FLAG.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The verdict a project made of this one file gets.
    fn audit_no_pager(file: &SourceFile<'_, Rust>) -> AuditStatus {
        let found = pager_use(file);
        verdict(found.pager_code, found.pager_code && found.no_pager_flag)
    }

    #[test]
    fn pass_when_no_pager_code() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let status = audit_no_pager(&pass::rust(source, "src/main.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn pass_when_pager_with_no_pager_flag() {
        let source = r#"
use pager::Pager;

fn setup_pager(no_pager: bool) {
    if !no_pager {
        Pager::new().setup();
    }
}
"#;
        let status = audit_no_pager(&pass::rust(source, "src/main.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_pager_without_flag() {
        let source = r#"
use pager::Pager;

fn setup() {
    Pager::new().setup();
}
"#;
        let status = audit_no_pager(&pass::rust(source, "src/main.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
            assert!(evidence.contains("no --no-pager"));
        }
    }

    #[test]
    fn warn_when_command_less_without_flag() {
        let source = r#"
use std::process::Command;

fn show_output(text: &str) {
    Command::new("less").spawn().expect("spawn less");
}
"#;
        let status = audit_no_pager(&pass::rust(source, "src/main.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
    }

    #[test]
    fn warn_when_command_more_without_flag() {
        let source = r#"
use std::process::Command;

fn show_output(text: &str) {
    Command::new("more").spawn().expect("spawn more");
}
"#;
        let status = audit_no_pager(&pass::rust(source, "src/main.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
    }

    #[test]
    fn pass_when_command_less_with_no_pager_flag() {
        let source = r#"
use std::process::Command;

fn show_output(text: &str, no_pager: bool) {
    if no_pager {
        println!("{text}");
    } else {
        Command::new("less").spawn().expect("spawn less");
    }
}
"#;
        let status = audit_no_pager(&pass::rust(source, "src/main.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let audit = NoPagerAudit;
        let dir = std::env::temp_dir().join(format!("anc-nopager-rust-{}", std::process::id()));
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
    fn not_applicable_for_none() {
        let audit = NoPagerAudit;
        let dir = std::env::temp_dir().join(format!("anc-nopager-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }
}
