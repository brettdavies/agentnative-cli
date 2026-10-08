//! Audit: `--help` advertises at least one non-interactive gate flag.
//!
//! Covers: `p1-must-no-interactive`. This is the second behavioral proof
//! of the same MUST — the existing `p1-non-interactive` audit probes
//! *runtime* behavior (bare invocation, stdin-primary). This audit probes
//! the *flag surface area* — does an option definition in the CLI's
//! `--help` declare any of the canonical non-interactive flags
//! (`--no-interactive`, `-p`, `--batch`, ...).
//!
//! Skip rather than Warn when the target already satisfies P1 via an
//! alternative gate (help-on-bare-invocation or stdin-clean-exit) — those
//! tools don't need an advertised flag to be agent-safe.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::{HelpOutput, RunStatus};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Canonical non-interactive gate flags. A tool that advertises any one
/// of these in `--help` is explicitly agent-addressable. Kept narrow on
/// purpose — broader matching produces false positives on tools where
/// `-y` means "yes file format" and similar collisions.
const GATE_FLAGS: &[&str] = &[
    "--no-interactive",
    "--non-interactive",
    "-p",
    "--print",
    "--no-input",
    "--batch",
    "--headless",
    "-y",
    "--yes",
    "--assume-yes",
];

const HELP_ON_BARE_MARKERS: &[&str] = &["Usage:", "USAGE:", "usage:"];

pub struct FlagExistenceAudit;

impl Audit for FlagExistenceAudit {
    fn id(&self) -> &str {
        "p1-flag-existence"
    }

    fn label(&self) -> &'static str {
        "Non-interactive gate flag advertised in --help"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P1
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-no-interactive"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let runner = project.runner_ref();

        // These probes hit BinaryRunner's cache when `p1-non-interactive`
        // already ran, so the cost is zero.
        let bare = runner.run(&[], &[]);
        let bare_output = format!("{}{}", bare.stdout, bare.stderr);
        let status = if has_alternative_gate(&bare_output, matches!(bare.status, RunStatus::Ok)) {
            AuditStatus::Skip(
                "target satisfies P1 via alternative gate (help-on-bare or stdin-primary)".into(),
            )
        } else {
            match project.help_output() {
                None => AuditStatus::Skip("could not probe --help".into()),
                Some(help) => audit_flag_existence(help),
            }
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

/// Whether the bare invocation already shows the target is safe to call
/// without a terminal: it printed usage, or it exited on its own.
fn has_alternative_gate(bare_output: &str, bare_exited: bool) -> bool {
    bare_exited || HELP_ON_BARE_MARKERS.iter().any(|m| bare_output.contains(m))
}

/// Core unit. Whether the help declares one of [`GATE_FLAGS`].
pub(crate) fn audit_flag_existence(help: &HelpOutput) -> AuditStatus {
    if help.raw().trim().is_empty() {
        return AuditStatus::Skip(
            "--help produced no output (likely non-English or unsupported)".into(),
        );
    }
    if help.find_flag(GATE_FLAGS).is_some() {
        return AuditStatus::Pass;
    }
    AuditStatus::Warn(help.noting_dash_rule(
        GATE_FLAGS,
        &format!(
            "no option definition in --help declares a non-interactive flag (one of: {}); usage \
             lines are not read.",
            GATE_FLAGS.join(", ")
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audit(help: &str) -> AuditStatus {
        audit_flag_existence(&HelpOutput::from_raw(help))
    }

    #[test]
    fn a_declared_gate_flag_passes() {
        for help in [
            "Options:\n      --batch    Run in batch mode.\n  -h, --help     Show help.\n",
            "Options:\n  -p, --print    Print output.\n  -h, --help     Show help.\n",
            "Options:\n  -y             Assume yes.\n  -h, --help     Show help.\n",
        ] {
            assert_eq!(audit(help), AuditStatus::Pass, "{help}");
        }
    }

    #[test]
    fn the_alternative_gates_are_usage_on_bare_or_a_clean_exit() {
        assert!(has_alternative_gate("Usage: foo [OPTIONS]\n", false));
        assert!(has_alternative_gate("", true));
        assert!(!has_alternative_gate("foo: waiting for input", false));
    }

    #[test]
    fn warn_names_the_flags_searched_and_where() {
        let help =
            "Options:\n      --color      When to color.\n      --version    Print version.\n";
        assert_eq!(
            audit(help),
            AuditStatus::Warn(
                "no option definition in --help declares a non-interactive flag (one of: \
                 --no-interactive, --non-interactive, -p, --print, --no-input, --batch, \
                 --headless, -y, --yes, --assume-yes); usage lines are not read."
                    .into()
            )
        );
    }

    #[test]
    fn empty_help_is_skipped_and_localized_help_is_still_read() {
        assert!(matches!(audit(""), AuditStatus::Skip(_)));
        let localized = "用法: outil\n选项:\n  -H, --header     自定义请求头\n";
        assert!(matches!(audit(localized), AuditStatus::Warn(_)));
    }

    /// Help text that names a gate flag without declaring it.
    const NOT_DECLARED: &[(&str, &str)] = &[
        (
            "a longer flag that starts with the name",
            "Options:\n      --print-json    Print as JSON.\n      --batching      Group requests.\n",
        ),
        (
            "a short cluster that starts with the letter",
            "Options:\n  -pr             Print and release.\n      --help      Show help.\n",
        ),
        (
            "a usage line",
            "Usage: tool [-y] [--batch] <file>\n\nOptions:\n  -h, --help    Show help.\n",
        ),
        (
            "another flag's wrapped description",
            "Options:\n      --confirm    Ask before each step. This is the default without\n                   --yes on a terminal.\n  -h, --help       Show help.\n",
        ),
        (
            "a sentence",
            "Run with --no-input in CI.\n\nOptions:\n  -h, --help    Show help.\n",
        ),
    ];

    #[test]
    fn a_gate_flag_named_but_not_declared_does_not_pass() {
        let passed: Vec<&str> = NOT_DECLARED
            .iter()
            .filter(|(_, help)| audit(help) == AuditStatus::Pass)
            .map(|(what, _)| *what)
            .collect();
        assert!(passed.is_empty(), "{passed:?}");
    }

    #[test]
    fn a_single_dash_word_passes_in_a_help_without_double_dash_names() {
        let help = "Usage of tool:\n  -batch\n    \tRun without prompting\n  -version\n    \tShow version\n";
        assert_eq!(audit(help), AuditStatus::Pass);
    }

    #[test]
    fn a_single_dash_word_beside_double_dash_names_warns_and_says_why() {
        // GNU findutils 4.10.0 declares `--help` and `--version`, and names
        // its actions with one dash.
        let help = "Options:\n      --help       display this help and exit\n      --version    output version information and exit\n  -print           print the full file name\n";
        match audit(help) {
            AuditStatus::Warn(msg) => assert!(
                msg.ends_with("`-print` is declared, but this help also declares double-dash names, so it does not count as `--print`."),
                "{msg}"
            ),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
