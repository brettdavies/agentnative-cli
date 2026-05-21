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
    // Vacuous Pass when no bundle present.
    if find_bundle(&project.path).is_none() {
        return CheckStatus::Pass;
    }

    // Vacuous Pass when no `skill` subcommand surface — `p8-bundle-install`
    // already flags that case; this MAY check should not stack-fail.
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

    check_install_all(runner)
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
