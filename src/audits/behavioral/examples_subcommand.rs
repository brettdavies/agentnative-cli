//! Audit: `p3-may-examples-subcommand`.
//!
//! Dedicated `examples` subcommand or `--examples` flag for curated usage
//! patterns. MAY-tier; mirrors the `p2-schema-print` detection shape.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct ExamplesSubcommandAudit;

impl Audit for ExamplesSubcommandAudit {
    fn id(&self) -> &str {
        "p3-examples-subcommand"
    }

    fn label(&self) -> &'static str {
        "`examples` subcommand or `--examples` flag for curated usage patterns"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P3
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-may-examples-subcommand"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_examples_subcommand(help),
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

pub(crate) fn audit_examples_subcommand(help: &HelpOutput) -> AuditStatus {
    let has_flag = help.flags().iter().any(|f| f.matches("--examples"));
    if has_flag {
        return AuditStatus::Pass;
    }
    let has_subcmd = help
        .subcommands()
        .iter()
        .any(|s| s.eq_ignore_ascii_case("examples"));
    if has_subcmd {
        return AuditStatus::Pass;
    }
    // Fallback: scan raw for `  examples  ` line the parser may have skipped.
    let in_section = help
        .raw()
        .lines()
        .any(|line| line.starts_with("  ") && line.trim_start().starts_with("examples"));
    if in_section {
        return AuditStatus::Pass;
    }

    AuditStatus::Warn(
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
            "Usage: tool [COMMAND]\n\nCommands:\n  audit    Run audits\n  examples Show usage\n",
        );
        assert_eq!(audit_examples_subcommand(&help), AuditStatus::Pass);
    }

    #[test]
    fn pass_when_flag_present() {
        let help = HelpOutput::from_raw(
            "Options:\n      --examples    Show curated usage.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_examples_subcommand(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_missing() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  audit    Run audits.\n  -h, --help    Show help.\n",
        );
        match audit_examples_subcommand(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("examples")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
