//! Check: `p8-may-bundle-update`.
//!
//! An `update` (or `upgrade`) subcommand under `tool skill` MAY pull the
//! latest bundle version. MAY-tier — absence is informational.
//!
//! Detection: probe `tool skill --help` for `update` or `upgrade` subcommands.
//! Gates on bundle presence and `skill` subcommand existence.

use crate::check::Check;
use crate::checks::project::bundle_exists::find_bundle;
use crate::project::Project;
use crate::runner::{BinaryRunner, RunStatus};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct BundleUpdateCheck;

impl Check for BundleUpdateCheck {
    fn id(&self) -> &str {
        "p8-bundle-update"
    }

    fn label(&self) -> &'static str {
        "`skill update` / `skill upgrade` for bundle refresh"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P8
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p8-may-bundle-update"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = compute_status(project);

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
        })
    }
}

/// Resolve the check's status without constructing a `CheckResult`. Per
/// CLAUDE.md's Source Check Convention, only `run()` constructs the result.
fn compute_status(project: &Project) -> CheckStatus {
    if find_bundle(&project.path).is_none() {
        return CheckStatus::Pass;
    }

    let Some(help) = project.help_output() else {
        return CheckStatus::Skip("could not probe --help".into());
    };
    let has_skill = help
        .subcommands()
        .iter()
        .any(|s| s.eq_ignore_ascii_case("skill"));
    if !has_skill {
        return CheckStatus::Pass;
    }

    let Some(runner) = project.runner.as_ref() else {
        return CheckStatus::Skip("no runner available for chained probe".into());
    };

    check_bundle_update(runner)
}

/// Core unit. Probes `<binary> skill --help` for `update` / `upgrade` in the
/// parsed subcommand list.
pub(crate) fn check_bundle_update(runner: &BinaryRunner) -> CheckStatus {
    let probe = runner.run(&["skill", "--help"], &[]);
    match probe.status {
        RunStatus::Ok | RunStatus::Timeout | RunStatus::Crash { .. } => {
            let combined = format!("{}{}", probe.stdout, probe.stderr);
            // Look for `update` or `upgrade` in the parsed subcommands.
            let combined_lower = combined.to_lowercase();
            if combined_lower.contains("update") || combined_lower.contains("upgrade") {
                CheckStatus::Pass
            } else {
                CheckStatus::Warn(
                    "no `update` or `upgrade` subcommand under `skill`. MAY-tier — \
                     a `skill update` lets agents stay current with the bundle's \
                     evolving surface without a full reinstall."
                        .into(),
                )
            }
        }
        RunStatus::NotFound => CheckStatus::Skip("binary not found".into()),
        RunStatus::PermissionDenied => CheckStatus::Skip("permission denied".into()),
        RunStatus::Error(msg) => CheckStatus::Skip(format!("probe error: {msg}")),
    }
}
