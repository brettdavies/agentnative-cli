//! Check: `p6-may-color-flag`.
//!
//! `--color auto|always|never` flag for explicit color control beyond the
//! TTY auto-detection enforced by `p6-must-no-color`. MAY-tier.

use crate::check::Check;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct ColorFlagCheck;

impl Check for ColorFlagCheck {
    fn id(&self) -> &str {
        "p6-color-flag"
    }

    fn label(&self) -> &'static str {
        "`--color` flag for explicit color control"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-may-color-flag"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_color_flag(help),
        };

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

pub(crate) fn check_color_flag(help: &HelpOutput) -> CheckStatus {
    if help.flags().iter().any(|f| f.matches("--color")) {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(
            "no `--color` flag advertised. MAY-tier — `auto|always|never` lets \
             agents and pipelines override the TTY-based default."
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_color_flag_present() {
        let help = HelpOutput::from_raw(
            "Options:\n      --color <WHEN>    [possible values: auto, always, never]\n",
        );
        assert_eq!(check_color_flag(&help), CheckStatus::Pass);
    }

    #[test]
    fn warn_when_missing() {
        let help = HelpOutput::from_raw("Options:\n  -h, --help    Show help.\n");
        match check_color_flag(&help) {
            CheckStatus::Warn(msg) => assert!(msg.contains("--color")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
