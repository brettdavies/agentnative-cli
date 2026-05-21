//! Check: `p7-may-auto-verbosity`.
//!
//! Automatic verbosity reduction in non-TTY contexts — the same behavior
//! `--quiet` explicitly requests, applied when stdout isn't a terminal.
//!
//! True TTY simulation needs a pty allocation, which is non-trivial in pure
//! Rust without a dedicated dependency. The defensible cheap path: scan the
//! help text for TTY-aware language. CLIs that describe TTY-conditional
//! behavior usually implement it; CLIs that don't, almost never do.
//!
//! - Pass when the help text mentions TTY-aware behavior.
//! - Warn when nothing is mentioned. MAY-tier — agents lose a small
//!   convenience but the CLI is not broken.

use crate::check::Check;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Phrases that indicate TTY-aware behavior in the help text. Case-insensitive
/// substring match.
const TTY_PHRASES: &[&str] = &[
    "when tty",
    "if tty",
    "non-tty",
    "non tty",
    "interactive terminal",
    "isatty",
    "is-a-tty",
    "is a terminal",
    "is a tty",
    "color=auto",
    "--color auto",
    "color auto",
    "pipe",
    "piped",
    "redirected",
    "auto-detect",
    "automatic verbosity",
    "automatic detection",
];

pub struct AutoVerbosityCheck;

impl Check for AutoVerbosityCheck {
    fn id(&self) -> &str {
        "p7-auto-verbosity"
    }

    fn label(&self) -> &'static str {
        "Help text advertises TTY-aware verbosity behavior"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P7
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-may-auto-verbosity"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_auto_verbosity(help),
        };
        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Low,
        })
    }
}

pub(crate) fn check_auto_verbosity(help: &HelpOutput) -> CheckStatus {
    let lower = help.raw().to_lowercase();
    let matched = TTY_PHRASES.iter().find(|phrase| lower.contains(*phrase));
    match matched {
        Some(_) => CheckStatus::Pass,
        None => CheckStatus::Warn(
            "no TTY-aware language found in `--help`. MAY-tier — automatic \
             verbosity reduction when stdout is piped or redirected lets \
             agents skip the explicit `--quiet` flag. Behavioral probes \
             cannot simulate a real TTY without a pty crate, so this check \
             relies on documented intent."
                .into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_help_mentions_tty() {
        let help = HelpOutput::from_raw(
            "Usage: tool [OPTIONS]\n\nProgress bars are shown when stdout is a TTY.\n",
        );
        assert_eq!(check_auto_verbosity(&help), CheckStatus::Pass);
    }

    #[test]
    fn pass_when_help_mentions_color_auto() {
        let help = HelpOutput::from_raw(
            "Options:\n      --color <WHEN>    auto-detect TTY, then enable color when supported.\n",
        );
        assert_eq!(check_auto_verbosity(&help), CheckStatus::Pass);
    }

    #[test]
    fn pass_when_help_mentions_piping() {
        let help = HelpOutput::from_raw(
            "Quiet mode is activated automatically when output is piped to another command.\n",
        );
        assert_eq!(check_auto_verbosity(&help), CheckStatus::Pass);
    }

    #[test]
    fn warn_when_no_tty_language() {
        let help = HelpOutput::from_raw(
            "Usage: tool [OPTIONS]\n\nOptions:\n  -q, --quiet    Be quiet.\n  -h, --help    Show help.\n",
        );
        match check_auto_verbosity(&help) {
            CheckStatus::Warn(msg) => assert!(msg.contains("TTY-aware")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn case_insensitive_match() {
        let help = HelpOutput::from_raw("Behavior changes WHEN TTY is connected.\n");
        assert_eq!(check_auto_verbosity(&help), CheckStatus::Pass);
    }
}
