//! Check: `p1-may-rich-tui`.
//!
//! Rich interactive experiences (spinners, progress bars, menus) when TTY is
//! detected and `--no-interactive` is not set. MAY-tier; detection is
//! heuristic — looks for indicator words in `--help` output or a `--tui` /
//! `--interactive` family of flags.

use crate::check::Check;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Indicator words that suggest a CLI ships rich-TUI behavior. Looked for
/// case-insensitively anywhere in `--help` output.
const TUI_INDICATORS: &[&str] = &[
    "spinner",
    "progress bar",
    "progress-bar",
    "interactive",
    "tui",
    "ncurses",
    "indicatif",
];

/// Flags that advertise a TUI / interactive mode.
const TUI_FLAGS: &[&str] = &["--tui", "--interactive", "--ui"];

pub struct RichTuiCheck;

impl Check for RichTuiCheck {
    fn id(&self) -> &str {
        "p1-rich-tui"
    }

    fn label(&self) -> &'static str {
        "Rich-TUI affordance for TTY contexts"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P1
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-may-rich-tui"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_rich_tui(help),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
        })
    }
}

pub(crate) fn check_rich_tui(help: &HelpOutput) -> CheckStatus {
    let has_flag = help
        .flags()
        .iter()
        .any(|f| TUI_FLAGS.iter().any(|name| f.matches(name)));
    if has_flag {
        return CheckStatus::Pass;
    }
    let raw_lower = help.raw().to_lowercase();
    if TUI_INDICATORS.iter().any(|t| raw_lower.contains(t)) {
        return CheckStatus::Pass;
    }
    CheckStatus::Warn(
        "no rich-TUI affordance detected (no `--tui`/`--interactive`/`--ui` \
         flag, no spinner/progress/tui mention in --help). MAY-tier — \
         rich TUI in TTY contexts is a nice-to-have, not required."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_with_tui_flag() {
        let help = HelpOutput::from_raw(
            "Options:\n      --tui    Launch the TUI dashboard.\n  -h, --help\n",
        );
        assert_eq!(check_rich_tui(&help), CheckStatus::Pass);
    }

    #[test]
    fn pass_with_interactive_flag() {
        let help = HelpOutput::from_raw(
            "Options:\n      --interactive    Run in interactive mode.\n  -h, --help\n",
        );
        assert_eq!(check_rich_tui(&help), CheckStatus::Pass);
    }

    #[test]
    fn pass_with_progress_bar_mention() {
        let help = HelpOutput::from_raw(
            "Usage: tool\n\nThis tool shows a progress bar during long-running operations.\n",
        );
        assert_eq!(check_rich_tui(&help), CheckStatus::Pass);
    }

    #[test]
    fn warn_when_no_tui_signal() {
        let help = HelpOutput::from_raw(
            "Options:\n      --output <FMT>    Output format.\n  -h, --help    Show help.\n",
        );
        match check_rich_tui(&help) {
            CheckStatus::Warn(msg) => assert!(msg.contains("rich-TUI")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
