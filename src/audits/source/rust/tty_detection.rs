//! Audit: Detect TTY/terminal detection in source.
//!
//! Principle: P1 (Non-Interactive by Default) SHOULD — "Auto-detect
//! non-interactive context via TTY detection and suppress prompts when
//! stderr is not a terminal, even without an explicit `--no-interactive`
//! flag." The same `IsTerminal` machinery also satisfies P6's color
//! suppression MUST, but semantically this audit verifies the P1 SHOULD
//! (renamed from `p6-tty-detection` in v0.1.1).
//!
//! This is a conditional audit:
//!   Trigger: the source uses color/ANSI/style libraries
//!   Pass: `IsTerminal` or `is_terminal()` is used
//!   Skip: no color code detected
//!   Warn: color code exists but no terminal detection

use crate::audit::Audit;
use crate::project::{Language, Project};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

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

/// Audit trait implementation for TTY detection.
pub struct TtyDetectionAudit;

impl Audit for TtyDetectionAudit {
    fn id(&self) -> &str {
        "p1-tty-detection-source"
    }

    fn label(&self) -> &'static str {
        "TTY detection for color output"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P1
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-should-tty-detection"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let parsed = project.parsed_files();
        let mut has_color = false;
        let mut has_tty = false;

        for (_path, parsed_file) in parsed.iter() {
            match &audit_tty_detection(&parsed_file.source) {
                AuditStatus::Skip(_) => {
                    // No color code in this file
                }
                AuditStatus::Pass => {
                    has_tty = true;
                    // If this file has color code, mark it
                    if source_has_color_code(&parsed_file.source) {
                        has_color = true;
                    }
                }
                AuditStatus::Warn(_) => {
                    has_color = true;
                }
                _ => {}
            }
        }

        let status = if has_tty {
            AuditStatus::Pass
        } else if !has_color {
            AuditStatus::Skip("no color/formatting code detected".to_string())
        } else {
            AuditStatus::Warn(
                "Color/ANSI code detected but no TTY detection found. \
                 Use IsTerminal or is_terminal() to avoid corrupting piped output."
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
        })
    }
}

/// Audit whether source has color/formatting code.
fn source_has_color_code(source: &str) -> bool {
    let lower = source.to_lowercase();
    COLOR_INDICATORS.iter().any(|ind| lower.contains(ind))
}

/// Audit whether source has TTY detection code.
fn source_has_tty_detection(source: &str) -> bool {
    TTY_INDICATORS.iter().any(|ind| source.contains(ind))
}

/// Audit a single source string for TTY detection.
pub(crate) fn audit_tty_detection(source: &str) -> AuditStatus {
    let has_tty = source_has_tty_detection(source);

    if has_tty {
        return AuditStatus::Pass;
    }

    let has_color = source_has_color_code(source);

    if !has_color {
        return AuditStatus::Skip("no color/formatting code detected".to_string());
    }

    AuditStatus::Warn(
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
        let status = audit_tty_detection(source);
        assert!(matches!(status, AuditStatus::Skip(_)));
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
        let status = audit_tty_detection(source);
        assert_eq!(status, AuditStatus::Pass);
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
        let status = audit_tty_detection(source);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_color_but_no_tty() {
        let source = r#"
use colored::Colorize;

fn display(msg: &str) {
    println!("{}", msg.green());
}
"#;
        let status = audit_tty_detection(source);
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
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
        let status = audit_tty_detection(source);
        assert!(matches!(status, AuditStatus::Warn(_)));
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
        let status = audit_tty_detection(source);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let audit = TtyDetectionAudit;
        let dir = std::env::temp_dir().join(format!("anc-tty-rust-{}", std::process::id()));
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
        let audit = TtyDetectionAudit;
        let dir = std::env::temp_dir().join(format!("anc-tty-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }
}
