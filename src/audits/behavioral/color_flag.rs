//! Audit: `p6-may-color-flag`.
//!
//! `--color auto|always|never` flag for explicit color control beyond the
//! TTY auto-detection enforced by `p6-must-no-color`. MAY-tier.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct ColorFlagAudit;

impl Audit for ColorFlagAudit {
    fn id(&self) -> &str {
        "p6-color-flag"
    }

    fn label(&self) -> &'static str {
        "`--color` flag for explicit color control"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-may-color-flag"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_color_flag(help),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

pub(crate) fn audit_color_flag(help: &HelpOutput) -> AuditStatus {
    if help.flags().iter().any(|f| f.matches("--color")) {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(
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
        assert_eq!(audit_color_flag(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_missing() {
        let help = HelpOutput::from_raw("Options:\n  -h, --help    Show help.\n");
        match audit_color_flag(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("--color")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
