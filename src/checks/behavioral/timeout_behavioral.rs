//! Check: `p7-should-timeout`.
//!
//! A `--timeout` flag bounds execution time so agents are not blocked
//! indefinitely. SHOULD-tier, conceptually universal but only meaningful
//! when the CLI has long-running operations. We gate on long-running verbs
//! in the subcommand list (`serve`, `daemon`, `watch`, `tail`, `monitor`,
//! `follow`, `run`, `start`) — otherwise vacuous skip.
//!
//! Distinct from the source-layer `p6-must-timeout-network`, which is
//! gated specifically on network-library usage. Both can fire on the same
//! CLI; they verify different requirements.

use crate::check::Check;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

const TIMEOUT_FLAGS: &[&str] = &["--timeout", "--deadline", "--max-time"];

/// Verbs that suggest the CLI ships long-running operations. Behavioral
/// detection only — if a CLI buries its long-running work behind a non-
/// matching subcommand name, the gate is conservative and skips.
const LONG_RUNNING_VERBS: &[&str] = &[
    "serve", "daemon", "watch", "tail", "monitor", "follow", "run", "start", "stream",
];

pub struct TimeoutBehavioralCheck;

impl Check for TimeoutBehavioralCheck {
    fn id(&self) -> &str {
        "p7-timeout-behavioral"
    }

    fn label(&self) -> &'static str {
        "`--timeout` flag for long-running operations"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P7
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-should-timeout"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => check_timeout_behavioral(help),
        };

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

pub(crate) fn check_timeout_behavioral(help: &HelpOutput) -> CheckStatus {
    let has_long_running_verb = help.subcommands().iter().any(|s| {
        LONG_RUNNING_VERBS
            .iter()
            .any(|verb| s.eq_ignore_ascii_case(verb))
    });
    if !has_long_running_verb {
        return CheckStatus::Skip(
            "no long-running subcommand detected (serve/daemon/watch/tail/monitor/\
             follow/run/start/stream); vacuous skip for the conditional SHOULD."
                .into(),
        );
    }

    let has_timeout = help
        .flags()
        .iter()
        .any(|f| TIMEOUT_FLAGS.iter().any(|name| f.matches(name)));

    if has_timeout {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(format!(
            "long-running subcommand present but no timeout flag advertised \
             (looked for {}). SHOULD-tier — without a bound, agents that hit \
             a hung operation have to enforce timeouts externally.",
            TIMEOUT_FLAGS.join(", "),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_long_running_verb() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  build    Build.\n  test    Run tests.\n",
        );
        match check_timeout_behavioral(&help) {
            CheckStatus::Skip(msg) => assert!(msg.contains("vacuous")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn pass_with_serve_and_timeout() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  serve    Start server.\n\n\
             Options:\n      --timeout <SECS>    Max execution time.\n  -h, --help\n",
        );
        assert_eq!(check_timeout_behavioral(&help), CheckStatus::Pass);
    }

    #[test]
    fn pass_with_watch_and_max_time() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  watch    Watch files.\n\n\
             Options:\n      --max-time <SECS>    Cap runtime.\n  -h, --help\n",
        );
        assert_eq!(check_timeout_behavioral(&help), CheckStatus::Pass);
    }

    #[test]
    fn warn_when_long_running_without_timeout() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  serve    Start server.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        match check_timeout_behavioral(&help) {
            CheckStatus::Warn(msg) => assert!(msg.contains("timeout")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
