//! Check: `p7-should-verbose`.
//!
//! `--verbose` flag (or `-v` / `-vv`) escalates diagnostic detail when
//! agents need to debug failures. SHOULD-tier; counterpart to `p7-must-quiet`.

use crate::check::Check;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct VerboseFlagCheck;

impl Check for VerboseFlagCheck {
    fn id(&self) -> &str {
        "p7-verbose"
    }

    fn label(&self) -> &'static str {
        "`--verbose` flag for diagnostic escalation"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P7
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-should-verbose"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_verbose_flag(help),
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

pub(crate) fn check_verbose_flag(help: &HelpOutput) -> CheckStatus {
    let has_verbose = help.flags().iter().any(|f| {
        f.matches("--verbose")
            || f.matches("-v")
            // Some tools advertise the stacked short form as `-vv` in help text;
            // the parsed `Flag` will still expose `-v` as the short form, but
            // a CLI that ONLY exposes `-vv` (no long `--verbose`) is rare.
            || f.matches("-vv")
    });
    if has_verbose {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(
            "no `--verbose` / `-v` flag advertised. SHOULD-tier — agents \
             debugging failures need a way to escalate diagnostic detail."
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_with_long_form() {
        let help = HelpOutput::from_raw(
            "Options:\n      --verbose    Show detail.\n  -h, --help    Show help.\n",
        );
        assert_eq!(check_verbose_flag(&help), CheckStatus::Pass);
    }

    #[test]
    fn pass_with_short_form() {
        let help = HelpOutput::from_raw(
            "Options:\n  -v, --debug    Show detail.\n  -h, --help    Show help.\n",
        );
        assert_eq!(check_verbose_flag(&help), CheckStatus::Pass);
    }

    #[test]
    fn warn_when_missing() {
        let help = HelpOutput::from_raw("Options:\n  -h, --help    Show help.\n");
        match check_verbose_flag(&help) {
            CheckStatus::Warn(msg) => assert!(msg.contains("--verbose")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
