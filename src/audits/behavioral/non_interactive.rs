use crate::audit::Audit;
use crate::project::Project;
use crate::runner::{HelpOutput, RunStatus};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Flags that signal the tool exposes a headless path. A `--help` that
/// declares any one satisfies P1's "no blocking-interactive surface"
/// requirement even if bare invocation doesn't itself exit cleanly.
const AGENTIC_FLAGS: &[&str] = &[
    "--no-interactive",
    "--non-interactive",
    "--batch",
    "--headless",
    "--yes",
    "--no-input",
    "--no-browser",
    "--device-code",
    "-y",
    "-p",
    "--print",
];

/// Help-output markers on the bare invocation. `arg_required_else_help`
/// in clap prints a "Usage:" block and exits non-zero — this is the
/// canonical non-interactive-by-default CLI shape.
const HELP_ON_BARE_MARKERS: &[&str] = &["Usage:", "USAGE:", "usage:"];

pub struct NonInteractiveAudit;

impl Audit for NonInteractiveAudit {
    fn id(&self) -> &str {
        "p1-non-interactive"
    }

    fn label(&self) -> &'static str {
        "Non-interactive by default"
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
        // BinaryRunner already pipes /dev/null as stdin, so the probe is
        // safe even when the target is agentnative itself.
        let bare = project.runner_ref().run(&[], &[]);
        let bare_output = format!("{}{}", bare.stdout, bare.stderr);
        let status = audit_non_interactive(&bare.status, &bare_output, project.help_output());

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: AuditGroup::P1,
            layer: AuditLayer::Behavioral,
            status,
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

/// Core unit. P1 Option-ε gate: the audit passes when ANY of three
/// conditions evidences agent-safe behavior:
///   1. help-on-bare-invocation — binary prints Usage and exits (clap
///      `arg_required_else_help`). This is what `anc` itself does; without
///      this clause the linter warns itself.
///   2. agentic-flag-present — `--help` declares `--no-interactive` (or
///      another of [`AGENTIC_FLAGS`]). The tool honors non-interactive
///      callers even if bare invocation does something else.
///   3. stdin-as-primary-input — binary exits cleanly when stdin is
///      /dev/null. POSIX utilities (jq, sed) satisfy P1 vacuously.
pub(crate) fn audit_non_interactive(
    bare: &RunStatus,
    bare_output: &str,
    help: Option<&HelpOutput>,
) -> AuditStatus {
    if help.is_some_and(|help| help.find_flag(AGENTIC_FLAGS).is_some()) {
        return AuditStatus::Pass;
    }
    let cause = match bare {
        RunStatus::Ok => return AuditStatus::Pass,
        RunStatus::Timeout => {
            "bare invocation timed out, so the binary may be waiting for interactive input".into()
        }
        RunStatus::Crash { signal } => {
            format!("binary crashed on bare invocation (signal {signal})")
        }
        _ if prints_usage(bare_output) => return AuditStatus::Pass,
        _ => "bare invocation gave no help-on-bare or clean-exit signal".to_string(),
    };
    let message = format!("{cause}, and {}", no_flag_declared(AGENTIC_FLAGS));
    AuditStatus::Warn(match help {
        Some(help) => help.noting_dash_rule(AGENTIC_FLAGS, &message),
        None => message,
    })
}

/// Whether a bare invocation's output is the tool's usage.
pub(super) fn prints_usage(bare_output: &str) -> bool {
    HELP_ON_BARE_MARKERS
        .iter()
        .any(|marker| bare_output.contains(marker))
}

/// What a deny says when no definition declares any of `flags`, the
/// non-interactive flags an audit accepts.
pub(super) fn no_flag_declared(flags: &[&str]) -> String {
    format!(
        "no option definition in --help declares a non-interactive flag (one of: {}); usage \
         lines are not read.",
        flags.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audits::behavioral::tests::{test_project_with_runner, test_project_with_sh_script};
    use crate::types::AuditStatus;

    #[test]
    fn non_interactive_pass_with_echo() {
        let project = test_project_with_runner("/bin/echo");
        let result = NonInteractiveAudit.run(&project).expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Pass));
    }

    #[test]
    fn non_interactive_pass_with_false() {
        let project = test_project_with_runner("/bin/false");
        let result = NonInteractiveAudit.run(&project).expect("audit should run");
        assert!(matches!(result.status, AuditStatus::Pass));
    }

    #[test]
    fn non_interactive_handles_crash_without_agentic_flag() {
        let project = test_project_with_sh_script("kill -11 $$");
        let result = NonInteractiveAudit
            .run(&project)
            .expect("audit should not panic on crash");
        assert!(matches!(result.status, AuditStatus::Warn(_)));
    }

    #[test]
    fn non_interactive_passes_when_bare_prints_usage() {
        // Simulates a clap-style `arg_required_else_help` binary: exits
        // non-zero and writes Usage to stderr. This is the dogfood shape.
        let script = r#"
if [ "$1" = "--help" ]; then
    echo "Usage: myapp [OPTIONS]"
    exit 0
fi
echo "Usage: myapp [OPTIONS]" >&2
exit 2
"#;
        let project = test_project_with_sh_script(script);
        let result = NonInteractiveAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn non_interactive_passes_when_help_declares_an_agentic_flag() {
        // Bare invocation crashes, and `--help` declares `--no-interactive`.
        let script = r#"
if [ "$1" = "--help" ]; then
    echo "Options:"
    echo "      --no-interactive    Never prompt"
    exit 0
fi
kill -11 $$
"#;
        let project = test_project_with_sh_script(script);
        let result = NonInteractiveAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass);
    }

    /// The verdict for a bare invocation that timed out, given `help`.
    fn blocked(help: &str) -> AuditStatus {
        audit_non_interactive(&RunStatus::Timeout, "", Some(&HelpOutput::from_raw(help)))
    }

    const SEARCHED: &str = "no option definition in --help declares a non-interactive flag (one of: --no-interactive, --non-interactive, --batch, --headless, --yes, --no-input, --no-browser, --device-code, -y, -p, --print); usage lines are not read.";

    #[test]
    fn a_declared_agentic_flag_passes_a_bare_call_that_blocks() {
        for help in [
            "Options:\n  -p, --print       Print the response and exit.\n  -h, --help        Show help.\n",
            "Options:\n  -y, --yes         Assume yes.\n  -h, --help        Show help.\n",
            "Options:\n      --no-input    Never prompt.\n  -h, --help        Show help.\n",
        ] {
            assert_eq!(blocked(help), AuditStatus::Pass, "{help}");
        }
    }

    /// Help that names an agentic flag without declaring it.
    const NOT_DECLARED: &[(&str, &str)] = &[
        (
            "opencode 1.18.34: a longer flag that starts with the name",
            "Options:\n  -h, --help          show help                                                            [boolean]\n      --print-logs    print logs to stderr                                                 [boolean]\n",
        ),
        (
            "longer flags that start with --batch and --yes",
            "Options:\n      --batch-size <N>           Rows per request.\n      --yes-i-really-mean-it     Skip the safety check.\n  -h, --help                     Show help.\n",
        ),
        (
            "a usage line",
            "Usage: foo [--no-interactive] <file>\n\nOptions:\n  -h, --help    Show help.\n",
        ),
        (
            "an example",
            "Options:\n  -h, --help    Show help.\n\nExamples:\n  foo -p \"summarize this\"\n",
        ),
        (
            "a sentence",
            "Pass -y to skip the confirmation.\n\nOptions:\n  -h, --help    Show help.\n",
        ),
    ];

    #[test]
    fn an_agentic_flag_named_but_not_declared_does_not_pass() {
        let passed: Vec<&str> = NOT_DECLARED
            .iter()
            .filter(|(_, help)| blocked(help) == AuditStatus::Pass)
            .map(|(what, _)| *what)
            .collect();
        assert!(passed.is_empty(), "{passed:?}");
    }

    #[test]
    fn a_single_dash_word_passes_in_a_help_without_double_dash_names() {
        let help = "Usage of tool:\n  -batch\n    \tRun without prompting\n  -version\n    \tShow version\n";
        assert_eq!(blocked(help), AuditStatus::Pass);
    }

    #[test]
    fn a_single_dash_print_beside_double_dash_names_warns_and_says_why() {
        // GNU findutils 4.10.0 declares `--help` and `--version`, and names
        // its actions with one dash.
        let help = "Options:\n      --help       display this help and exit\n      --version    output version information and exit\n  -print           print the full file name\n";
        match blocked(help) {
            AuditStatus::Warn(msg) => assert!(
                msg.ends_with("`-print` is declared, but this help also declares double-dash names, so it does not count as `--print`."),
                "{msg}"
            ),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn each_warn_says_what_the_bare_call_did_and_what_was_searched() {
        let help = HelpOutput::from_raw("Options:\n  -h, --help    Show help.\n");
        let warn = |bare: RunStatus| match audit_non_interactive(&bare, "", Some(&help)) {
            AuditStatus::Warn(msg) => msg,
            other => panic!("expected Warn, got {other:?}"),
        };
        assert_eq!(
            warn(RunStatus::Timeout),
            format!(
                "bare invocation timed out, so the binary may be waiting for interactive input, and {SEARCHED}"
            )
        );
        assert_eq!(
            warn(RunStatus::Crash { signal: 11 }),
            format!("binary crashed on bare invocation (signal 11), and {SEARCHED}")
        );
        assert_eq!(
            warn(RunStatus::Error("spawn failed".into())),
            format!("bare invocation gave no help-on-bare or clean-exit signal, and {SEARCHED}")
        );
    }

    #[test]
    fn a_bare_call_that_exits_or_prints_usage_passes_without_a_flag() {
        let help = HelpOutput::from_raw("Options:\n  -h, --help    Show help.\n");
        assert_eq!(
            audit_non_interactive(&RunStatus::Ok, "", Some(&help)),
            AuditStatus::Pass
        );
        assert_eq!(
            audit_non_interactive(&RunStatus::Ok, "", None),
            AuditStatus::Pass
        );
    }
}
