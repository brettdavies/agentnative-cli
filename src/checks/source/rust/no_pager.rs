//! Check: Detect pager invocations without a `--no-pager` flag.
//!
//! Principle: P6 (Composable Structure) — Pagers block agents. If a CLI uses
//! a pager, it must offer `--no-pager` to disable it.
//!
//! This is a conditional check:
//!   Trigger: source references pager crate or spawns less/more
//!   Requirement: a `--no-pager` flag must exist

use crate::check::Check;
use crate::project::{Language, Project};
use crate::source::has_pattern;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

/// Pager-related patterns to detect in source text.
const PAGER_INDICATORS: &[&str] = &["pager::Pager", "Pager::new", "Pager::with_pager"];

/// Check trait implementation for no-pager detection.
pub struct NoPagerCheck;

impl Check for NoPagerCheck {
    fn id(&self) -> &str {
        "p6-no-pager"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut has_pager = false;
        let mut has_no_pager_flag = false;

        for (_path, parsed_file) in parsed.iter() {
            match &check_no_pager(&parsed_file.source) {
                // Pass + pager code present means both are true: has pager, has --no-pager flag
                CheckStatus::Pass if source_has_pager_code(&parsed_file.source) => {
                    has_pager = true;
                    has_no_pager_flag = true;
                }
                CheckStatus::Warn(_) => {
                    has_pager = true;
                }
                _ => {}
            }
        }

        let status = if !has_pager || has_no_pager_flag {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(
                "Pager code detected but no --no-pager flag found. \
                 Pagers block agents; provide --no-pager to disable."
                    .to_string(),
            )
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "No pager blocking agents".to_string(),
            group: self.group(),
            layer: self.layer(),
            status,
        })
    }
}

/// Check whether source has pager-related code.
fn source_has_pager_code(source: &str) -> bool {
    // String search for pager indicators
    let has_indicator = PAGER_INDICATORS.iter().any(|p| source.contains(p));

    // ast-grep search for Command::new("less") and Command::new("more")
    let has_less = has_pattern(source, r#"Command::new("less")"#);
    let has_more = has_pattern(source, r#"Command::new("more")"#);

    // Check for pager crate usage via `use pager`
    let has_pager_crate = source.contains("use pager") || source.contains("extern crate pager");

    has_indicator || has_less || has_more || has_pager_crate
}

/// Check a single source string for pager code and --no-pager flag.
pub(crate) fn check_no_pager(source: &str) -> CheckStatus {
    let has_pager = source_has_pager_code(source);

    if !has_pager {
        return CheckStatus::Pass;
    }

    // Check for --no-pager flag
    let has_flag = source.contains("no-pager") || source.contains("no_pager");

    if has_flag {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(
            "Pager code detected but no --no-pager flag found. \
             Pagers block agents; provide --no-pager to disable."
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_no_pager_code() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let status = check_no_pager(source);
        assert_eq!(status, CheckStatus::Pass);
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
        let status = check_no_pager(source);
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_pager_without_flag() {
        let source = r#"
use pager::Pager;

fn setup() {
    Pager::new().setup();
}
"#;
        let status = check_no_pager(source);
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
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
        let status = check_no_pager(source);
        assert!(matches!(status, CheckStatus::Warn(_)));
    }

    #[test]
    fn warn_when_command_more_without_flag() {
        let source = r#"
use std::process::Command;

fn show_output(text: &str) {
    Command::new("more").spawn().expect("spawn more");
}
"#;
        let status = check_no_pager(source);
        assert!(matches!(status, CheckStatus::Warn(_)));
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
        let status = check_no_pager(source);
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let check = NoPagerCheck;
        let dir = std::env::temp_dir().join(format!("anc-nopager-rust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(check.applicable(&project));
    }

    #[test]
    fn not_applicable_for_none() {
        let check = NoPagerCheck;
        let dir = std::env::temp_dir().join(format!("anc-nopager-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
