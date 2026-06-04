//! Audit: `p2-may-raw-flag`.
//!
//! `--raw` flag for unformatted output suitable for piping to other tools.
//! Universal applicability; MAY-tier so absence is Warn, never Fail.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct RawFlagAudit;

impl Audit for RawFlagAudit {
    fn id(&self) -> &str {
        "p2-raw-flag"
    }

    fn label(&self) -> &'static str {
        "`--raw` flag for pipe-safe unformatted output"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P2
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-may-raw-flag"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_raw_flag(help),
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

pub(crate) fn audit_raw_flag(help: &HelpOutput) -> AuditStatus {
    if help.flags().iter().any(|f| f.matches("--raw")) {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(
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
        assert_eq!(audit_raw_flag(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_missing() {
        let help =
            HelpOutput::from_raw("Options:\n      --output <FMT>\n  -h, --help    Show help.\n");
        match audit_raw_flag(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("--raw")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
