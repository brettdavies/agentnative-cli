//! Audit: `p8-may-install-all`.
//!
//! `--all` mode auto-detects installed agent runtimes (Claude Code, Cursor,
//! Codex, OpenCode) and installs across each. MAY-tier — absence is
//! informational, not a failure.
//!
//! Detection: probe `tool skill install --help` (chained probe) for an
//! option definition that declares `--all`.
//! Applicability gates on bundle presence at project root and the `skill`
//! subcommand existing on the binary's help surface.

use crate::audit::Audit;
use crate::audits::behavioral::flag_presence::pass_or_warn;
use crate::audits::project::bundle_exists::find_bundle;
use crate::project::Project;
use crate::runner::{BinaryRunner, HelpOutput, RunStatus};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const ALL_FLAG: &[&str] = &["--all"];

pub struct InstallAllAudit;

impl Audit for InstallAllAudit {
    fn id(&self) -> &str {
        "p8-install-all"
    }

    fn label(&self) -> &'static str {
        "`skill install --all` for multi-runtime install"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P8
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p8-may-install-all"]
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
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

/// Resolve the audit's status without constructing a `AuditResult`. Per
/// CLAUDE.md's Source Audit Convention, only `run()` constructs the result.
fn compute_status(project: &Project) -> AuditStatus {
    // Vacuous Pass when no bundle present.
    if find_bundle(&project.path).is_none() {
        return AuditStatus::Pass;
    }

    // Vacuous Pass when no `skill` subcommand surface — `p8-bundle-install`
    // already flags that case; this MAY audit should not stack-fail.
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

    audit_install_all(runner)
}

/// Probes `<binary> skill install --help` and grades what it prints.
pub(crate) fn audit_install_all(runner: &BinaryRunner) -> AuditStatus {
    let probe = runner.run(&["skill", "install", "--help"], &[]);
    match probe.status {
        RunStatus::Ok | RunStatus::Timeout | RunStatus::Crash { .. } => audit_install_help(
            &HelpOutput::from_raw(format!("{}{}", probe.stdout, probe.stderr)),
        ),
        RunStatus::NotFound => AuditStatus::Skip("binary not found".into()),
        RunStatus::PermissionDenied => AuditStatus::Skip("permission denied".into()),
        RunStatus::Error(msg) => AuditStatus::Skip(format!("probe error: {msg}")),
    }
}

/// Core unit. Whether the help of `skill install` declares `--all`.
pub(crate) fn audit_install_help(help: &HelpOutput) -> AuditStatus {
    pass_or_warn(
        help,
        ALL_FLAG,
        "no option definition in `skill install --help` declares `--all`; usage lines are not \
         read. MAY-tier — a single `skill install --all` invocation across detected runtimes \
         is convenient for multi-agent setups.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audit(help: &str) -> AuditStatus {
        audit_install_help(&HelpOutput::from_raw(help))
    }

    const SEARCHED: &str = "no option definition in `skill install --help` declares `--all`; usage lines are not read. MAY-tier — a single `skill install --all` invocation across detected runtimes is convenient for multi-agent setups.";

    #[test]
    fn a_declared_all_flag_passes() {
        // anc's own `skill install --help` lists `--all` on a line of its
        // own, with the description on the next.
        let help = "Usage: anc skill install [OPTIONS] [HOST]\n\nOptions:\n      --all\n          Install into every detected host\n\n  -h, --help\n          Print help\n";
        assert_eq!(audit(help), AuditStatus::Pass);
    }

    /// Help that carries the characters `--all` without declaring the flag.
    const NOT_DECLARED: &[(&str, &str)] = &[
        (
            "longer flags that start with --all",
            "Options:\n      --allow <HOST>      Allow one more host\n      --all-features      Build every feature\n  -h, --help              Print help\n",
        ),
        (
            "a usage line",
            "Usage: tool skill install [--all] [HOST]\n\nOptions:\n  -h, --help    Print help\n",
        ),
        (
            "another flag's description",
            "Arguments:\n  [HOST]\n          Target host. Required unless `--all` is set\n\nOptions:\n  -h, --help\n          Print help\n",
        ),
    ];

    #[test]
    fn all_named_but_not_declared_warns() {
        let passed: Vec<&str> = NOT_DECLARED
            .iter()
            .filter(|(_, help)| audit(help) == AuditStatus::Pass)
            .map(|(what, _)| *what)
            .collect();
        assert!(passed.is_empty(), "{passed:?}");
    }

    #[test]
    fn the_warn_names_what_was_searched() {
        assert_eq!(
            audit("Options:\n  -h, --help    Print help\n"),
            AuditStatus::Warn(SEARCHED.into())
        );
    }

    #[test]
    fn a_single_dash_all_follows_the_help_s_own_convention() {
        let go_flag = "Usage of install:\n  -all\n    \tInstall into every detected host\n  -host string\n    \tTarget host\n";
        assert_eq!(audit(go_flag), AuditStatus::Pass);

        let mixed = "Options:\n  -all          Install into every detected host\n      --help    Print help\n";
        match audit(mixed) {
            AuditStatus::Warn(msg) => {
                assert!(msg.starts_with(SEARCHED), "{msg}");
                assert!(msg.ends_with("so it does not count as `--all`."), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
