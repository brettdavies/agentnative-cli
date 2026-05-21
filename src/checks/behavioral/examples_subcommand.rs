//! Check: `p3-may-examples-subcommand`.
//!
//! Dedicated `examples` subcommand or `--examples` flag for curated usage
//! patterns. MAY-tier; mirrors the `p2-schema-print` detection shape.

use crate::check::Check;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct ExamplesSubcommandCheck;

impl Check for ExamplesSubcommandCheck {
    fn id(&self) -> &str {
        "p3-examples-subcommand"
    }

    fn label(&self) -> &'static str {
        "`examples` subcommand or `--examples` flag for curated usage patterns"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P3
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-may-examples-subcommand"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_examples_subcommand(help),
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

pub(crate) fn check_examples_subcommand(help: &HelpOutput) -> CheckStatus {
    let has_flag = help.flags().iter().any(|f| f.matches("--examples"));
    if has_flag {
        return CheckStatus::Pass;
    }
    let has_subcmd = help
        .subcommands()
        .iter()
        .any(|s| s.eq_ignore_ascii_case("examples"));
    if has_subcmd {
        return CheckStatus::Pass;
    }
    // Fallback: scan raw for `  examples  ` line the parser may have skipped.
    let in_section = help
        .raw()
        .lines()
        .any(|line| line.starts_with("  ") && line.trim_start().starts_with("examples"));
    if in_section {
        return CheckStatus::Pass;
    }

    CheckStatus::Warn(
        "no `examples` subcommand or `--examples` flag found. MAY-tier — \
         a curated usage block keeps agents from hunting through long help text."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_subcommand_present() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  check    Run checks\n  examples Show usage\n",
        );
        assert_eq!(check_examples_subcommand(&help), CheckStatus::Pass);
    }

    #[test]
    fn pass_when_flag_present() {
        let help = HelpOutput::from_raw(
            "Options:\n      --examples    Show curated usage.\n  -h, --help    Show help.\n",
        );
        assert_eq!(check_examples_subcommand(&help), CheckStatus::Pass);
    }

    #[test]
    fn warn_when_missing() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  check    Run checks.\n  -h, --help    Show help.\n",
        );
        match check_examples_subcommand(&help) {
            CheckStatus::Warn(msg) => assert!(msg.contains("examples")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
