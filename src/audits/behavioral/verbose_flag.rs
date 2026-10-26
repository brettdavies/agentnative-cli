//! Audit: `p7-should-verbose`.
//!
//! `--verbose` flag (or `-v` / `-vv`) escalates diagnostic detail when
//! agents need to debug failures. SHOULD-tier; counterpart to `p7-must-quiet`.

use crate::audit::Audit;
use crate::audits::behavioral::flag_meaning::Meaning;
use crate::audits::behavioral::flag_presence::pass_or_warn;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const VERBOSE_FLAGS: &[&str] = &["--verbose", "-v", "-vv"];

/// `-v` is also `--version` (claude, lazygit, bun) and `--invert-match`
/// (rg, grep). The words come from real `-v` descriptions: "increase
/// verbosity" (argparse, click), "verbosely list files" (tar), "set logging
/// level" (ffmpeg), "Enable debug output".
const VERBOSE: Meaning = Meaning {
    what: "verbose",
    long_names: &[],
    words: &["verbose", "verbosely", "verbosity", "logging", "debug"],
};

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
        Some(&VERBOSE),
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
            "Options:\n  -v, --debug    Enable debug output.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_verbose_flag(&help), AuditStatus::Pass);
    }

    fn warning(help: &str) -> String {
        match audit_verbose_flag(&HelpOutput::from_raw(help)) {
            AuditStatus::Warn(msg) => msg,
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn a_lone_v_whose_description_names_verbosity_passes() {
        // probe-go-flag's `-v` and ffmpeg 7.1's `-v <loglevel>`.
        for help in [
            "Usage of probe:\n  -v\tverbose\n",
            "Main options:\n-v loglevel         set logging level\n-y                  overwrite output files\n",
        ] {
            assert_eq!(
                audit_verbose_flag(&HelpOutput::from_raw(help)),
                AuditStatus::Pass,
                "{help}"
            );
        }
    }

    #[test]
    fn a_v_that_means_version_warns_and_says_why() {
        // claude 2.1's `--help`.
        let msg = warning(
            "Options:\n  -p, --print                           Print response and exit\n  -v, --version                         Output the version number\n",
        );
        assert!(
            msg.ends_with(
                "`-v` is declared, but its definition (`-v, --version  Output the version \
                 number`) does not mean verbose."
            ),
            "{msg}"
        );
    }

    #[test]
    fn a_v_that_means_something_else_does_not_pass() {
        // rg 15.1 and dust 1.2 declare `-v` for other flags.
        for help in [
            "OPTIONS:\n    -v, --invert-match\n        Invert matching.\n",
            "Options:\n  -v, --invert-filter <REGEX>  Exclude filepaths matching this regex.\n",
        ] {
            assert!(warning(help).contains("does not mean verbose"), "{help}");
        }
    }

    #[test]
    fn a_lone_v_with_no_description_does_not_pass() {
        let msg = warning("Options:\n  -v\n  -h, --help    Show help.\n");
        assert!(
            msg.ends_with("`-v` is declared, but its definition (`-v`) does not mean verbose."),
            "{msg}"
        );
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
