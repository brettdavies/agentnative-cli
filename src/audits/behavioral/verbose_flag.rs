//! Audit: `p7-should-verbose`.
//!
//! `--verbose` flag (or `-v` / `-vv`) escalates diagnostic detail when
//! agents need to debug failures. SHOULD-tier; counterpart to `p7-must-quiet`.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

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
        })
    }
}

pub(crate) fn audit_verbose_flag(help: &HelpOutput) -> AuditStatus {
    // Check for --verbose or -vv first (unambiguous verbose flags)
    let has_unambiguous_verbose = help
        .flags()
        .iter()
        .any(|f| f.matches("--verbose") || f.matches("-vv"));
    if has_unambiguous_verbose {
        return AuditStatus::Pass;
    }

    // Check for -v, but verify it's not -v for --version
    let has_v_flag = help.flags().iter().any(|f| f.matches("-v"));
    if has_v_flag {
        // Check if -v is documented as version by looking for patterns like:
        // "-v, --version" or lines with "-v" that contain "version"
        let raw = help.raw();

        // Look for lines containing -v and check if they mention version
        for line in raw.lines() {
            let line_lower = line.to_lowercase();
            if line_lower.contains("-v") && !line_lower.contains("--verbose") {
                // If this line mentions version, -v is for version, not verbose
                if line_lower.contains("version") {
                    return AuditStatus::Warn(
                        "no `--verbose` flag advertised. Found `-v` but it appears to be for \
                         version, not verbosity. SHOULD-tier — agents debugging failures need a \
                         way to escalate diagnostic detail."
                            .into(),
                    );
                }
            }
        }

        // If -v exists but doesn't seem to be for version, accept it as verbose
        return AuditStatus::Pass;
    }

    AuditStatus::Warn(
        "no `--verbose` / `-v` flag advertised. SHOULD-tier — agents \
         debugging failures need a way to escalate diagnostic detail."
            .into(),
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

    #[test]
    fn warn_when_missing() {
        let help = HelpOutput::from_raw("Options:\n  -h, --help    Show help.\n");
        match audit_verbose_flag(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("--verbose")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn warn_when_v_is_version() {
        let help = HelpOutput::from_raw(
            "Options:\n  -v, --version    Show version.\n  -h, --help    Show help.\n",
        );
        match audit_verbose_flag(&help) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("--verbose"));
                assert!(msg.contains("version"));
            }
            other => panic!("expected Warn when -v is for version, got {other:?}"),
        }
    }

    #[test]
    fn pass_when_v_is_verbose_not_version() {
        let help = HelpOutput::from_raw(
            "Options:\n  -v, --verbose    Increase verbosity.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_verbose_flag(&help), AuditStatus::Pass);
    }
}
