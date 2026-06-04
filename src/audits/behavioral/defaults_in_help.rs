//! Audit: `p1-should-defaults-in-help`.
//!
//! Document default values for prompted inputs in `--help` output. SHOULD-tier,
//! universal applicability. A simple Pass/Warn rubric on whether any flag in
//! the help text advertises a `[default: ...]` annotation (clap's convention)
//! or `(default: ...)` prose (older parsers).

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct DefaultsInHelpAudit;

impl Audit for DefaultsInHelpAudit {
    fn id(&self) -> &str {
        "p1-defaults-in-help"
    }

    fn label(&self) -> &'static str {
        "`--help` advertises default values for flags"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P1
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-should-defaults-in-help"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_defaults_in_help(help),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: None,
        })
    }
}

pub(crate) fn audit_defaults_in_help(help: &HelpOutput) -> AuditStatus {
    let raw_lower = help.raw().to_lowercase();
    // Clap and most parsers render defaults as `[default: <value>]`. Older
    // tools sometimes use `(default: ...)` prose; some print `Default: ...`.
    let has_default = raw_lower.contains("[default:")
        || raw_lower.contains("(default:")
        || raw_lower.contains("default:");
    if has_default {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(
            "no default-value annotations found in --help. SHOULD-tier — \
             agents reading help text need to see what value a flag falls \
             back to when omitted (`[default: <value>]` per clap convention)."
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_with_clap_default() {
        let help = HelpOutput::from_raw(
            "Options:\n      --output <FMT>    Output format [default: text]\n  -h, --help\n",
        );
        assert_eq!(audit_defaults_in_help(&help), AuditStatus::Pass);
    }

    #[test]
    fn pass_with_prose_default() {
        let help = HelpOutput::from_raw(
            "Options:\n      --output <FMT>    Output format. Default: text.\n  -h, --help\n",
        );
        assert_eq!(audit_defaults_in_help(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_no_defaults_documented() {
        let help = HelpOutput::from_raw(
            "Options:\n      --output <FMT>    Output format.\n  -h, --help    Show help.\n",
        );
        match audit_defaults_in_help(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("default")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
