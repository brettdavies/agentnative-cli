//! Check: `p4-must-actionable-errors`.
//!
//! Error messages must name what failed, why, and what to do next — not just
//! "request failed" or "invalid arg". Behaviorally we surface the bad-flag
//! probe's stderr and verify (a) stderr is non-empty and (b) it contains at
//! least one hint phrase from a curated list. Clap's default diagnostic
//! shape (`error: unexpected argument ... For more information, try '--help'`)
//! satisfies the rubric; bare one-liners do not.

use crate::check::Check;
use crate::checks::behavioral::error_probe::probe_bad_invocation;
use crate::project::Project;
use crate::runner::{RunResult, RunStatus};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Hint phrases — substrings that suggest the error message tells the user
/// what to do next. Case-insensitive. Curated against clap, argparse, click,
/// and cobra default diagnostics; small enough to inspect on review and
/// large enough to avoid false-Fail on idiomatic CLIs.
const HINT_PHRASES: &[&str] = &[
    "try ",
    "use ",
    "expected ",
    "did you mean",
    "see ",
    "run ",
    "for more",
    "tip:",
    "help:",
    "hint:",
    "suggestion:",
    "available ",
];

pub struct ActionableErrorsCheck;

impl Check for ActionableErrorsCheck {
    fn id(&self) -> &str {
        "p4-actionable-errors"
    }

    fn label(&self) -> &'static str {
        "Error messages include a hint or remediation phrase"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P4
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-must-actionable-errors"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let result = probe_bad_invocation(project.runner_ref());
        let status = check_actionable_errors(&result);
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

pub(crate) fn check_actionable_errors(result: &RunResult) -> CheckStatus {
    if let RunStatus::Crash { signal } = result.status {
        return CheckStatus::Fail(format!(
            "binary crashed on bad invocation (signal {signal}); no error \
             message to evaluate."
        ));
    }
    let stderr = result.stderr.trim();
    if stderr.is_empty() {
        return CheckStatus::Fail(
            "stderr was empty on bad invocation. Argument parsers must \
             explain why the input was rejected — silent failure forces the \
             agent to guess."
                .into(),
        );
    }
    let lower = stderr.to_lowercase();
    let matched = HINT_PHRASES.iter().find(|phrase| lower.contains(*phrase));
    match matched {
        Some(_) => CheckStatus::Pass,
        None => CheckStatus::Fail(format!(
            "stderr lacks a hint phrase. Looked for one of: {}. Error \
             messages must name what to do next, not just the symptom.",
            HINT_PHRASES.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_result(stderr: &str, exit_code: i32) -> RunResult {
        RunResult {
            exit_code: Some(exit_code),
            stdout: String::new(),
            stderr: stderr.to_string(),
            status: RunStatus::Ok,
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    #[test]
    fn pass_on_clap_default_message() {
        // Real clap output captured from anc --bad-flag.
        let stderr =
            "error: unexpected argument '--bad' found\n\nFor more information, try '--help'.";
        assert_eq!(
            check_actionable_errors(&fake_result(stderr, 2)),
            CheckStatus::Pass
        );
    }

    #[test]
    fn pass_on_did_you_mean() {
        let stderr = "error: unknown subcommand 'lis'\n  did you mean 'list'?";
        assert_eq!(
            check_actionable_errors(&fake_result(stderr, 2)),
            CheckStatus::Pass
        );
    }

    #[test]
    fn pass_on_use_hint() {
        let stderr = "error: invalid value\n  use --format=json|text";
        assert_eq!(
            check_actionable_errors(&fake_result(stderr, 2)),
            CheckStatus::Pass
        );
    }

    #[test]
    fn fail_on_bare_message() {
        let stderr = "error: bad input";
        match check_actionable_errors(&fake_result(stderr, 2)) {
            CheckStatus::Fail(msg) => assert!(msg.contains("hint phrase")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_on_empty_stderr() {
        match check_actionable_errors(&fake_result("", 2)) {
            CheckStatus::Fail(msg) => assert!(msg.contains("stderr was empty")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_on_crash() {
        let result = RunResult {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            status: RunStatus::Crash { signal: 11 },
            stdout_truncated: false,
            stderr_truncated: false,
        };
        match check_actionable_errors(&result) {
            CheckStatus::Fail(msg) => assert!(msg.contains("crashed")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
