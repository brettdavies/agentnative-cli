//! Check: Detect `process::exit()` calls in non-main files.
//!
//! Principle: P4 (Actionable Errors) — `process::exit()` should only appear
//! in main.rs. Library code should return errors, not terminate the process.

use crate::check::Check;
use crate::project::{Language, Project};
use crate::source::find_pattern_matches;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

const PATTERNS: &[&str] = &["process::exit($CODE)", "std::process::exit($CODE)"];

/// Check trait implementation for process::exit location detection.
pub struct ProcessExitCheck;

impl Check for ProcessExitCheck {
    fn id(&self) -> &str {
        "p4-process-exit"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P4
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut all_evidence = Vec::new();

        for (path, parsed_file) in parsed.iter() {
            let file_str = path.display().to_string();
            // Allow process::exit in main.rs
            if file_str.ends_with("main.rs") {
                continue;
            }

            let result = check_process_exit(&parsed_file.source, &file_str);
            if let CheckStatus::Fail(evidence) = result.status {
                all_evidence.push(evidence);
            }
        }

        let status = if all_evidence.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail(all_evidence.join("\n"))
        };

        Ok(CheckResult {
            id: "p4-process-exit".to_string(),
            label: "No process::exit outside main".to_string(),
            group: CheckGroup::P4,
            layer: CheckLayer::Source,
            status,
        })
    }
}

/// Check a single source string for `process::exit()` calls.
///
/// Kept public(crate) for unit testing with inline source strings.
pub(crate) fn check_process_exit(source: &str, file: &str) -> CheckResult {
    let mut all_matches = Vec::new();

    for pattern_str in PATTERNS {
        let mut matches = find_pattern_matches(source, pattern_str);
        for m in &mut matches {
            m.file = file.to_string();
        }
        all_matches.extend(matches);
    }

    let status = if all_matches.is_empty() {
        CheckStatus::Pass
    } else {
        let evidence = all_matches
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        CheckStatus::Fail(evidence)
    };

    CheckResult {
        id: "p4-process-exit".to_string(),
        label: "No process::exit outside main".to_string(),
        group: CheckGroup::P4,
        layer: CheckLayer::Source,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_no_process_exit() {
        let source = r#"
fn handle_error(e: Error) -> Result<()> {
    Err(e.into())
}
"#;
        let result = check_process_exit(source, "src/lib.rs");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn fail_when_process_exit_in_lib() {
        let source = r#"
use std::process;

fn bail(msg: &str) {
    eprintln!("{msg}");
    process::exit(1);
}
"#;
        let result = check_process_exit(source, "src/lib.rs");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
        if let CheckStatus::Fail(evidence) = &result.status {
            assert!(evidence.contains("process::exit"));
            assert!(evidence.contains("src/lib.rs"));
        }
    }

    #[test]
    fn fail_with_std_process_exit_in_lib() {
        let source = r#"
fn bail() {
    std::process::exit(1);
}
"#;
        let result = check_process_exit(source, "src/util.rs");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
        if let CheckStatus::Fail(evidence) = &result.status {
            assert!(evidence.contains("std::process::exit"));
        }
    }

    #[test]
    fn run_skips_main_rs() {
        // The run() method skips main.rs, but the inner function does not.
        // This test verifies the inner function finds the call.
        let source = r#"
use std::process;

fn main() {
    process::exit(0);
}
"#;
        // Inner function always reports — it's run() that filters main.rs
        let result = check_process_exit(source, "src/main.rs");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
    }

    #[test]
    fn fail_counts_multiple_exits() {
        let source = r#"
use std::process;

fn bail_a() {
    process::exit(1);
}

fn bail_b() {
    process::exit(2);
}
"#;
        let result = check_process_exit(source, "src/commands.rs");
        if let CheckStatus::Fail(evidence) = &result.status {
            assert_eq!(evidence.lines().count(), 2);
        } else {
            panic!("Expected Fail");
        }
    }

    #[test]
    fn applicable_for_rust() {
        let check = ProcessExitCheck;
        let dir = std::env::temp_dir().join(format!("anc-procexit-rust-{}", std::process::id()));
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
    fn not_applicable_for_python() {
        let check = ProcessExitCheck;
        let dir = std::env::temp_dir().join(format!("anc-procexit-py-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test pyproject.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
