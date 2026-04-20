use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

/// Agentic flag markers in `--help` output that signal the tool exposes a
/// headless path. Matching any one satisfies P1's "no blocking-interactive
/// surface" requirement even if bare invocation doesn't itself exit cleanly.
const AGENTIC_FLAG_MARKERS: &[&str] = &[
    "--no-interactive",
    "--non-interactive",
    "--batch",
    "--headless",
    "--yes",
    "--no-input",
    "--no-browser",
    "--device-code",
    "-y,",
    "-y ",
    " -p,",
    " -p ",
    "--print",
];

/// Help-output markers on the bare invocation. `arg_required_else_help`
/// in clap prints a "Usage:" block and exits non-zero — this is the
/// canonical non-interactive-by-default CLI shape.
const HELP_ON_BARE_MARKERS: &[&str] = &["Usage:", "USAGE:", "usage:"];

pub struct NonInteractiveCheck;

impl Check for NonInteractiveCheck {
    fn id(&self) -> &str {
        "p1-non-interactive"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P1
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-no-interactive"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner_ref();

        // P1 Option-ε gate: the check passes when ANY of three conditions
        // evidences agent-safe behavior:
        //   1. help-on-bare-invocation — binary prints Usage and exits
        //      (clap `arg_required_else_help`). This is what `anc` itself
        //      does; without this clause the linter warns itself.
        //   2. agentic-flag-present — `--help` advertises `--no-interactive`
        //      (or equivalent). The tool honors non-interactive callers
        //      even if bare invocation does something else.
        //   3. stdin-as-primary-input — binary exits cleanly when stdin is
        //      /dev/null. POSIX utilities (jq, sed) satisfy P1 vacuously.
        //
        // BinaryRunner already pipes /dev/null as stdin, so the probe is
        // safe even when the target is agentnative itself.
        let bare = runner.run(&[], &[]);
        let bare_output = format!("{}{}", bare.stdout, bare.stderr);
        let help_on_bare = matches_any(&bare_output, HELP_ON_BARE_MARKERS);

        let help = runner.run(&["--help"], &[]);
        let help_output = format!("{}{}", help.stdout, help.stderr);
        let agentic_flag = matches_any(&help_output, AGENTIC_FLAG_MARKERS);

        let stdin_clean_exit = matches!(bare.status, RunStatus::Ok);

        let status = match bare.status {
            RunStatus::Timeout if !agentic_flag => {
                CheckStatus::Warn("binary may be waiting for interactive input".into())
            }
            RunStatus::Crash { signal } if !agentic_flag => CheckStatus::Warn(format!(
                "binary crashed on bare invocation (signal {signal})"
            )),
            _ => {
                if help_on_bare || agentic_flag || stdin_clean_exit {
                    CheckStatus::Pass
                } else {
                    // Bare exited without a status that clearly evidences
                    // agent-safety. Surface as Warn so the operator sees it.
                    CheckStatus::Warn(
                        "no help-on-bare, agentic flag, or clean-exit signal detected".into(),
                    )
                }
            }
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Non-interactive by default".into(),
            group: CheckGroup::P1,
            layer: CheckLayer::Behavioral,
            status,
        })
    }
}

fn matches_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| haystack.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::behavioral::tests::{test_project_with_runner, test_project_with_sh_script};
    use crate::types::CheckStatus;

    #[test]
    fn non_interactive_pass_with_echo() {
        let project = test_project_with_runner("/bin/echo");
        let result = NonInteractiveCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[test]
    fn non_interactive_pass_with_false() {
        let project = test_project_with_runner("/bin/false");
        let result = NonInteractiveCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[test]
    fn non_interactive_handles_crash_without_agentic_flag() {
        let project = test_project_with_sh_script("kill -11 $$");
        let result = NonInteractiveCheck
            .run(&project)
            .expect("check should not panic on crash");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
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
        let result = NonInteractiveCheck.run(&project).expect("check should run");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn non_interactive_passes_when_help_advertises_agentic_flag() {
        // Simulates a tool where bare invocation does something non-obvious
        // but `--help` advertises `--no-interactive` — that's the contract.
        let script = r#"
if [ "$1" = "--help" ]; then
    echo "Usage: foo [--no-interactive]"
    exit 0
fi
echo "running default action"
"#;
        let project = test_project_with_sh_script(script);
        let result = NonInteractiveCheck.run(&project).expect("check should run");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn matches_any_finds_marker() {
        assert!(matches_any("Usage: foo [OPTIONS]", HELP_ON_BARE_MARKERS));
        assert!(matches_any(
            "  --no-interactive   skip prompts",
            AGENTIC_FLAG_MARKERS
        ));
        assert!(!matches_any("just some text", AGENTIC_FLAG_MARKERS));
    }
}
