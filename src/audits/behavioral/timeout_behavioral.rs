//! Audit: `p7-should-timeout`.
//!
//! A `--timeout` flag bounds execution time so agents are not blocked
//! indefinitely. SHOULD-tier, conceptually universal but only meaningful
//! when the CLI has long-running operations. We gate on long-running verbs
//! in the subcommand list (`serve`, `daemon`, `watch`, `tail`, `monitor`,
//! `follow`, `run`, `start`) — otherwise vacuous skip.
//!
//! The flag belongs on the long-running command (`mise run --timeout`), so
//! the audit reads each long-running subcommand's own `--help`, plus the
//! top-level help for a global flag every subcommand inherits. P7 asks for
//! a `--timeout` flag without naming every command, so one long-running
//! subcommand that carries one is a Pass.
//!
//! Distinct from the source-layer `p6-must-timeout-network`, which is
//! gated specifically on network-library usage. Both can fire on the same
//! CLI; they verify different requirements.

use crate::audit::Audit;
use crate::audits::behavioral::subcommand_help::{coverage, first_flag, probe_named};
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const TIMEOUT_FLAGS: &[&str] = &["--timeout", "--deadline", "--max-time"];

/// Verbs that suggest the CLI ships long-running operations. Behavioral
/// detection only — if a CLI buries its long-running work behind a non-
/// matching subcommand name, the gate is conservative and skips.
const LONG_RUNNING_VERBS: &[&str] = &[
    "serve", "daemon", "watch", "tail", "monitor", "follow", "run", "start", "stream",
];

pub struct TimeoutBehavioralAudit;

impl Audit for TimeoutBehavioralAudit {
    fn id(&self) -> &str {
        "p7-timeout-behavioral"
    }

    fn label(&self) -> &'static str {
        "`--timeout` flag for long-running operations"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P7
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-should-timeout"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let (status, pass_evidence) = match project.help_output() {
            None => (AuditStatus::Skip("could not probe --help".into()), None),
            Some(help) => {
                let subhelp = probe_named(project.runner_ref(), &long_running_subcommands(help));
                audit_timeout_behavioral(help, &subhelp)
            }
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: None,
            config_hint: None,
            pass_evidence,
        })
    }
}

fn long_running_subcommands(help: &HelpOutput) -> Vec<&str> {
    help.subcommands()
        .iter()
        .map(String::as_str)
        .filter(|s| {
            LONG_RUNNING_VERBS
                .iter()
                .any(|verb| s.eq_ignore_ascii_case(verb))
        })
        .collect()
}

/// The status, plus the Pass evidence naming the long-running subcommands
/// that carry a timeout flag.
pub(crate) fn audit_timeout_behavioral(
    help: &HelpOutput,
    subhelp: &[(String, HelpOutput)],
) -> (AuditStatus, Option<String>) {
    let triggers = long_running_subcommands(help);
    if triggers.is_empty() {
        return (
            AuditStatus::Skip(
                "no long-running subcommand detected (serve/daemon/watch/tail/monitor/\
                 follow/run/start/stream); vacuous skip for the conditional SHOULD."
                    .into(),
            ),
            None,
        );
    }

    let cov = coverage(&triggers, help, subhelp, |h| {
        first_flag(h, |f| f.declares_any(TIMEOUT_FLAGS).is_some())
    });
    if !cov.with.is_empty() {
        let lacking = if cov.without.is_empty() {
            String::new()
        } else {
            format!(" Without one: {}.", cov.without_list())
        };
        return (
            AuditStatus::Pass,
            Some(format!(
                "long-running subcommand(s) with a timeout flag: {}.{lacking}",
                cov.with_list()
            )),
        );
    }
    (
        AuditStatus::Warn(format!(
            "no long-running subcommand advertises a timeout flag in its --help: {} \
             (looked for {}). SHOULD-tier: without a bound, agents that hit a hung \
             operation have to enforce timeouts externally.",
            cov.without_list(),
            TIMEOUT_FLAGS.join(", "),
        )),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Excerpts of mise's `mise --help` and `mise run --help`.
    const MISE_HELP: &str = "\
Usage: mise [FLAGS] [TASK] [SUBCOMMAND]

Commands:
  prune         Delete unused versions of tools
  run           Run tasks and their dependencies [aliases: r]
  search        Search for available tools
  watch         Run task(s) and rerun them when files change
                [aliases: w]
  which         Show the path a tool's executable resolves to

Arguments:
  [TASK]  Task to run.
";

    const MISE_RUN_HELP: &str = "\
Run tasks and their dependencies

Usage: mise run [FLAGS] [TASK] [ARGS]…

Flags:
      --task-cache-stats         Report task output cache hits, restored bytes,
                                 and time saved
      --timeout <TIMEOUT>        Timeout for the task to complete
                                 e.g.: 30s, 5m
";

    // Excerpts of vhs 0.12's `vhs --help` and `vhs serve --help`.
    const VHS_HELP: &str = "\
Run a given tape file and generates its outputs.

Usage:
  vhs <file> [flags]
  vhs [command]

Available Commands:
  help        Help about any command
  new         Create a new tape file with example tape file contents and documentation
  record      Create a new tape file by recording your actions
  serve       Start the VHS SSH server
  validate    Validate a glob file path and parses all the files to ensure they are valid without running them.

Flags:
  -h, --help             help for vhs
  -o, --output strings   file name(s) of video output
  -q, --quiet            quiet do not log messages. If publish flag is provided, it will log shareable URL
";

    const VHS_SERVE_HELP: &str = "\
Start the VHS SSH server

Usage:
  vhs serve [flags]

Flags:
  -h, --help   help for serve

Global Flags:
  -q, --quiet   quiet do not log messages. If publish flag is provided, it will log shareable URL
";

    fn sub(name: &str, raw: &str) -> (String, HelpOutput) {
        (name.to_string(), HelpOutput::from_raw(raw))
    }

    #[test]
    fn skip_when_no_long_running_verb() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  build    Build.\n  test    Run tests.\n",
        );
        match audit_timeout_behavioral(&help, &[]).0 {
            AuditStatus::Skip(msg) => assert!(msg.contains("vacuous")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn pass_with_serve_and_timeout() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  serve    Start server.\n\n\
             Options:\n      --timeout <SECS>    Max execution time.\n  -h, --help\n",
        );
        assert_eq!(audit_timeout_behavioral(&help, &[]).0, AuditStatus::Pass);
    }

    #[test]
    fn pass_with_watch_and_max_time() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  watch    Watch files.\n\n\
             Options:\n      --max-time <SECS>    Cap runtime.\n  -h, --help\n",
        );
        assert_eq!(audit_timeout_behavioral(&help, &[]).0, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_long_running_without_timeout() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  serve    Start server.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        match audit_timeout_behavioral(&help, &[]).0 {
            AuditStatus::Warn(msg) => assert!(msg.contains("timeout")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn timeout_flag_on_one_long_running_subcommand_passes() {
        let help = HelpOutput::from_raw(MISE_HELP);
        let subhelp = vec![sub("run", MISE_RUN_HELP)];
        let (status, evidence) = audit_timeout_behavioral(&help, &subhelp);
        assert_eq!(status, AuditStatus::Pass);
        assert_eq!(
            evidence.as_deref(),
            Some(
                "long-running subcommand(s) with a timeout flag: run (--timeout). Without one: watch."
            )
        );
    }

    #[test]
    fn warn_names_the_long_running_subcommands_it_read() {
        let help = HelpOutput::from_raw(VHS_HELP);
        let subhelp = vec![sub("serve", VHS_SERVE_HELP)];
        match audit_timeout_behavioral(&help, &subhelp) {
            (AuditStatus::Warn(msg), None) => {
                assert!(msg.contains("in its --help: serve (looked for"), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
