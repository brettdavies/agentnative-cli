//! Check: `p8-may-install-all`.
//!
//! `--all` mode auto-detects installed agent runtimes (Claude Code, Cursor,
//! Codex, OpenCode) and installs across each. MAY-tier — absence is
//! informational, not a failure.
//!
//! Detection: probe `tool skill install --help` (chained probe) for `--all`.
//! Applicability gates on bundle presence at project root and the `skill`
//! subcommand existing on the binary's help surface.

use crate::check::Check;
use crate::checks::project::bundle_exists::find_bundle;
use crate::project::Project;
use crate::runner::{BinaryRunner, RunStatus};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct InstallAllCheck;

impl Check for InstallAllCheck {
    fn id(&self) -> &str {
        "p8-install-all"
    }

    fn label(&self) -> &'static str {
        "`skill install --all` for multi-runtime install"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P8
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p8-may-install-all"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        // Vacuous Pass when no bundle present.
        if find_bundle(&project.path).is_none() {
            return Ok(make_result(self, CheckStatus::Pass));
        }

        // Vacuous Pass when no `skill` subcommand surface — `p8-bundle-install`
        // already flags that case; this MAY check should not stack-fail.
        let Some(help) = project.help_output() else {
            return Ok(make_result(
                self,
                CheckStatus::Skip("could not probe --help".into()),
            ));
        };
        let has_skill = help
            .subcommands()
            .iter()
            .any(|s| s.eq_ignore_ascii_case("skill"));
        if !has_skill {
            return Ok(make_result(self, CheckStatus::Pass));
        }

        let Some(runner) = project.runner.as_ref() else {
            return Ok(make_result(
                self,
                CheckStatus::Skip("no runner available for chained probe".into()),
            ));
        };

        let status = check_install_all(runner);
        Ok(make_result(self, status))
    }
}

fn make_result(check: &InstallAllCheck, status: CheckStatus) -> CheckResult {
    CheckResult {
        id: check.id().to_string(),
        label: check.label().into(),
        group: check.group(),
        layer: check.layer(),
        status,
        confidence: Confidence::Medium,
    }
}

/// Core unit. Probes `<binary> skill install --help` and inspects the
/// captured output for an `--all` flag mention.
pub(crate) fn check_install_all(runner: &BinaryRunner) -> CheckStatus {
    let probe = runner.run(&["skill", "install", "--help"], &[]);
    match probe.status {
        RunStatus::Ok | RunStatus::Timeout | RunStatus::Crash { .. } => {
            let combined = format!("{}{}", probe.stdout, probe.stderr);
            if combined.contains("--all") {
                CheckStatus::Pass
            } else {
                CheckStatus::Warn(
                    "no `--all` flag found in `skill install --help`. MAY-tier — \
                     a single `skill install --all` invocation across detected \
                     runtimes is convenient for multi-agent setups."
                        .into(),
                )
            }
        }
        RunStatus::NotFound => CheckStatus::Skip("binary not found".into()),
        RunStatus::PermissionDenied => CheckStatus::Skip("permission denied".into()),
        RunStatus::Error(msg) => CheckStatus::Skip(format!("probe error: {msg}")),
    }
}
