//! Audit: `p7-should-verbose`.
//!
//! `--verbose` flag (or `-v` / `-vv`) escalates diagnostic detail when
//! agents need to debug failures. SHOULD-tier; counterpart to `p7-must-quiet`.

use crate::audit::Audit;
use crate::audits::behavioral::flag_presence::pass_or_warn;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const VERBOSE_FLAGS: &[&str] = &["--verbose", "-v", "-vv"];

pub struct VerboseFlagAudit;

impl Audit for VerboseFlagAudit {
    fn id(&self) -> &str {
        "p7-verbose"
    }

    fn label(&self) -> &'static str {
        "`--verbose` flag for diagnostic escalation"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P7
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-should-verbose"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_verbose_flag(help),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

pub(crate) fn audit_verbose_flag(help: &HelpOutput) -> AuditStatus {
    pass_or_warn(
        help,
        VERBOSE_FLAGS,
        "no `--verbose` / `-v` flag advertised. SHOULD-tier — agents \
         debugging failures need a way to escalate diagnostic detail.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_with_long_form() {
        let help = HelpOutput::from_raw(
            "Options:\n      --verbose    Show detail.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_verbose_flag(&help), AuditStatus::Pass);
    }

    #[test]
    fn pass_with_short_form() {
        let help = HelpOutput::from_raw(
            "Options:\n  -v, --debug    Show detail.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_verbose_flag(&help), AuditStatus::Pass);
    }

    // Excerpt of actionlint 1.7.12's `--help`, which declares no
    // double-dash name.
    const ACTIONLINT_HELP: &str = "Usage: actionlint [FLAGS] [FILES...] [-]\n\nFlags:\n  -color\n    \tAlways enable colorful output. This is useful to force colorful outputs\n  -format string\n    \tCustom template to format error messages in Go template syntax.\n  -verbose\n    \tEnable verbose output\n  -version\n    \tShow version and how this binary was installed\n";

    // The global options of terraform 1.16.4's `--help`.
    const TERRAFORM_HELP: &str = "Global options (use these before the subcommand, if any):\n  -chdir=DIR    Switch to a different working directory before executing the\n                given subcommand.\n  -help         Show this help output or the help for a specified subcommand.\n  -version      An alias for the \"version\" subcommand.\n";

    const DASH_RULE_NOTE: &str = "`-verbose` is declared, but this help also declares double-dash names, so it does not count as `--verbose`.";

    #[test]
    fn a_single_dash_verbose_passes_in_a_help_without_double_dash_names() {
        let help = HelpOutput::from_raw(ACTIONLINT_HELP);
        assert_eq!(audit_verbose_flag(&help), AuditStatus::Pass);
    }

    #[test]
    fn a_single_dash_version_is_not_a_verbose_flag() {
        let help = HelpOutput::from_raw(TERRAFORM_HELP);
        match audit_verbose_flag(&help) {
            AuditStatus::Warn(msg) => assert!(!msg.contains("is declared"), "{msg}"),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn a_single_dash_verbose_beside_double_dash_names_warns_and_says_why() {
        let help = HelpOutput::from_raw(
            "Options:\n  -verbose      Show detail.\n      --help    Show help.\n",
        );
        match audit_verbose_flag(&help) {
            AuditStatus::Warn(msg) => assert!(msg.ends_with(DASH_RULE_NOTE), "{msg}"),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn warn_when_missing() {
        let help = HelpOutput::from_raw("Options:\n  -h, --help    Show help.\n");
        match audit_verbose_flag(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("--verbose")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
