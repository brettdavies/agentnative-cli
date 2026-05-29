//! Audit: `p8-may-bundle-update`.
//!
//! An `update` (or `upgrade`) subcommand under `tool skill` MAY pull the
//! latest bundle version. MAY-tier — absence is informational.
//!
//! Detection: probe `tool skill --help` for `update` or `upgrade` subcommands.
//! Gates on bundle presence and `skill` subcommand existence.

use crate::audit::Audit;
use crate::audits::project::bundle_exists::find_bundle;
use crate::project::Project;
use crate::runner::{BinaryRunner, RunStatus};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct BundleUpdateAudit;

impl Audit for BundleUpdateAudit {
    fn id(&self) -> &str {
        "p8-bundle-update"
    }

    fn label(&self) -> &'static str {
        "`skill update` / `skill upgrade` for bundle refresh"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P8
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p8-may-bundle-update"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = compute_status(project);

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
        })
    }
}

/// Resolve the audit's status without constructing a `AuditResult`. Per
/// CLAUDE.md's Source Audit Convention, only `run()` constructs the result.
fn compute_status(project: &Project) -> AuditStatus {
    if find_bundle(&project.path).is_none() {
        return AuditStatus::Pass;
    }

    let Some(help) = project.help_output() else {
        return AuditStatus::Skip("could not probe --help".into());
    };
    let has_skill = help
        .subcommands()
        .iter()
        .any(|s| s.eq_ignore_ascii_case("skill"));
    if !has_skill {
        return AuditStatus::Pass;
    }

    let Some(runner) = project.runner.as_ref() else {
        return AuditStatus::Skip("no runner available for chained probe".into());
    };

    audit_bundle_update(runner)
}

/// Core unit. Probes `<binary> skill --help` for `update` / `upgrade` in the
/// parsed subcommand list.
pub(crate) fn audit_bundle_update(runner: &BinaryRunner) -> AuditStatus {
    let probe = runner.run(&["skill", "--help"], &[]);
    match probe.status {
        RunStatus::Ok | RunStatus::Timeout | RunStatus::Crash { .. } => {
            let combined = format!("{}{}", probe.stdout, probe.stderr);
            // Look for `update` or `upgrade` in the parsed subcommands.
            let combined_lower = combined.to_lowercase();
            if combined_lower.contains("update") || combined_lower.contains("upgrade") {
                AuditStatus::Pass
            } else {
                AuditStatus::Warn(
                    "no `update` or `upgrade` subcommand under `skill`. MAY-tier — \
                     a `skill update` lets agents stay current with the bundle's \
                     evolving surface without a full reinstall."
                        .into(),
                )
            }
        }
        RunStatus::NotFound => AuditStatus::Skip("binary not found".into()),
        RunStatus::PermissionDenied => AuditStatus::Skip("permission denied".into()),
        RunStatus::Error(msg) => AuditStatus::Skip(format!("probe error: {msg}")),
    }
}
