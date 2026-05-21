//! Check: `p2-may-raw-flag`.
//!
//! `--raw` flag for unformatted output suitable for piping to other tools.
//! Universal applicability; MAY-tier so absence is Warn, never Fail.

use crate::check::Check;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct RawFlagCheck;

impl Check for RawFlagCheck {
    fn id(&self) -> &str {
        "p2-raw-flag"
    }

    fn label(&self) -> &'static str {
        "`--raw` flag for pipe-safe unformatted output"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P2
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-may-raw-flag"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_raw_flag(help),
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

pub(crate) fn check_raw_flag(help: &HelpOutput) -> CheckStatus {
    if help.flags().iter().any(|f| f.matches("--raw")) {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(
            "no `--raw` flag advertised. MAY-tier — useful for pipelines that \
             want to strip formatting before piping to other tools."
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_raw_flag_present() {
        let help = HelpOutput::from_raw(
            "Options:\n      --raw    Unformatted output.\n  -h, --help    Show help.\n",
        );
        assert_eq!(check_raw_flag(&help), CheckStatus::Pass);
    }

    #[test]
    fn warn_when_missing() {
        let help =
            HelpOutput::from_raw("Options:\n      --output <FMT>\n  -h, --help    Show help.\n");
        match check_raw_flag(&help) {
            CheckStatus::Warn(msg) => assert!(msg.contains("--raw")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
