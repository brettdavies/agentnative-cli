//! Check: Detect TTY/terminal detection in source.
//!
//! Principle: P1 (Non-Interactive by Default) SHOULD — "Auto-detect
//! non-interactive context via TTY detection and suppress prompts when
//! stderr is not a terminal, even without an explicit `--no-interactive`
//! flag." The same `IsTerminal` machinery also satisfies P6's color
//! suppression MUST, but semantically this check verifies the P1 SHOULD
//! (renamed from `p6-tty-detection` in v0.1.1).
//!
//! This is a conditional check:
//!   Trigger: the source uses color/ANSI/style libraries
//!   Pass: `IsTerminal` or `is_terminal()` is used
//!   Skip: no color code detected
//!   Warn: color code exists but no terminal detection

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

/// Color/formatting indicators to search for in source text.
const COLOR_INDICATORS: &[&str] = &[
    "color",
    "ansi",
    "style",
    "colored",
    "owo-colors",
    "owo_colors",
    "termcolor",
    "yansi",
    "console",
];

/// TTY detection indicators.
const TTY_INDICATORS: &[&str] = &["IsTerminal", "is_terminal", "atty", "is_tty", "isatty"];

/// Check trait implementation for TTY detection.
pub struct TtyDetectionCheck;

impl Check for TtyDetectionCheck {
    fn id(&self) -> &str {
        "p1-tty-detection-source"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P1
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-should-tty-detection"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut has_color = false;
        let mut has_tty = false;

        for (_path, parsed_file) in parsed.iter() {
            match &check_tty_detection(&parsed_file.source) {
                CheckStatus::Skip(_) => {
                    // No color code in this file
                }
                CheckStatus::Pass => {
                    has_tty = true;
                    // If this file has color code, mark it
                    if source_has_color_code(&parsed_file.source) {
                        has_color = true;
                    }
                }
                CheckStatus::Warn(_) => {
                    has_color = true;
                }
                _ => {}
            }
        }

        let status = if has_tty {
            CheckStatus::Pass
        } else if !has_color {
            CheckStatus::Skip("no color/formatting code detected".to_string())
        } else {
            CheckStatus::Warn(
                "Color/ANSI code detected but no TTY detection found. \
                 Use IsTerminal or is_terminal() to avoid corrupting piped output."
                    .to_string(),
            )
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "TTY detection for color output".to_string(),
            group: self.group(),
            layer: self.layer(),
            status,
        })
    }
}

/// Check whether source has color/formatting code.
fn source_has_color_code(source: &str) -> bool {
    let lower = source.to_lowercase();
    COLOR_INDICATORS.iter().any(|ind| lower.contains(ind))
}

/// Check whether source has TTY detection code.
fn source_has_tty_detection(source: &str) -> bool {
    TTY_INDICATORS.iter().any(|ind| source.contains(ind))
}

/// Check a single source string for TTY detection.
pub(crate) fn check_tty_detection(source: &str) -> CheckStatus {
    let has_tty = source_has_tty_detection(source);

    if has_tty {
        return CheckStatus::Pass;
    }

    let has_color = source_has_color_code(source);

    if !has_color {
        return CheckStatus::Skip("no color/formatting code detected".to_string());
    }

    CheckStatus::Warn(
        "Color/ANSI code detected but no TTY detection found. \
         Use IsTerminal or is_terminal() to avoid corrupting piped output."
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_color_code() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let status = check_tty_detection(source);
        assert!(matches!(status, CheckStatus::Skip(_)));
    }

    #[test]
    fn pass_when_is_terminal_used() {
        let source = r#"
use std::io::IsTerminal;
use colored::Colorize;

fn setup_color() {
    if std::io::stdout().is_terminal() {
        enable_color();
    }
}
"#;
        let status = check_tty_detection(source);
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn pass_with_atty() {
        let source = r#"
use atty;
use termcolor::StandardStream;

fn setup() {
    if atty::is(atty::Stream::Stdout) {
        // colored output
    }
}
"#;
        let status = check_tty_detection(source);
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_color_but_no_tty() {
        let source = r#"
use colored::Colorize;

fn display(msg: &str) {
    println!("{}", msg.green());
}
"#;
        let status = check_tty_detection(source);
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
            assert!(evidence.contains("TTY detection"));
        }
    }

    #[test]
    fn warn_with_ansi_codes() {
        let source = r#"
fn display(msg: &str) {
    // Using ansi escape codes directly
    print!("\x1b[32m{msg}\x1b[0m");
}
"#;
        let status = check_tty_detection(source);
        assert!(matches!(status, CheckStatus::Warn(_)));
    }

    #[test]
    fn pass_with_is_terminal_trait() {
        let source = r#"
use std::io::IsTerminal;
use owo_colors::OwoColorize;

fn main() {
    let use_color = std::io::stdout().is_terminal();
    if use_color {
        println!("{}", "ok".green());
    }
}
"#;
        let status = check_tty_detection(source);
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let check = TtyDetectionCheck;
        let dir = std::env::temp_dir().join(format!("anc-tty-rust-{}", std::process::id()));
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
        let check = TtyDetectionCheck;
        let dir = std::env::temp_dir().join(format!("anc-tty-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
