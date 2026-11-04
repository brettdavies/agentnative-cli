//! Audit: `p7-should-timeout`.
//!
//! A `--timeout` flag, or its documented equivalent `--deadline` or
//! `--max-time`, bounds execution time so agents are not blocked indefinitely.
//! SHOULD-tier. P7 applies the requirement to commands that can run unbounded:
//! a subcommand named for serving, watching, tailing, following, or streaming,
//! or a tool whose own help describes it as a server or watcher. The gate reads
//! those two signals: the subcommand names in `LONG_RUNNING_VERBS`, and the
//! paragraph of prose the top-level help opens with. Without either the row is
//! a vacuous skip.
//!
//! The flag belongs on the long-running command (`xr follow --timeout`), so
//! the audit reads each long-running subcommand's own `--help`, plus the
//! top-level help for a global flag every subcommand inherits. P7 asks for
//! a `--timeout` flag without naming every command, so one long-running
//! subcommand that carries one is a Pass. A tool that only its description
//! puts under the requirement has no such subcommand, so its top-level help
//! decides.
//!
//! Distinct from the source-layer `p6-must-timeout-network`, which is
//! gated specifically on network-library usage. Both can fire on the same
//! CLI; they verify different requirements.

use crate::audit::Audit;
use crate::audits::behavioral::flag_meaning::{first_sentence, names_any};
use crate::audits::behavioral::subcommand_help::{
    coverage, dash_rule_notes, first_flag, probe_named,
};
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub(crate) const TIMEOUT_FLAGS: &[&str] = &["--timeout", "--deadline", "--max-time"];

/// Subcommand names P7 reads as serving, watching, tailing, following, or
/// streaming. `run` and `start` stay out: P7 does not name them, and `gh run`
/// is a noun for workflow runs. Behavioral detection only: a CLI that buries
/// its long-running work behind another name skips unless its description
/// says what it is.
const LONG_RUNNING_VERBS: &[&str] = &[
    "serve", "daemon", "watch", "tail", "monitor", "follow", "stream",
];

/// Words a tool's own description uses to say it is a server or watcher,
/// matched whole. The singular forms keep out a tool that manages `servers`.
const SERVER_OR_WATCHER_WORDS: &[&str] = &[
    "server",
    "serve",
    "serves",
    "serving",
    "daemon",
    "watcher",
    "watch",
    "watches",
    "watched",
    "watching",
    "monitor",
    "monitors",
    "monitoring",
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
            row_statuses: Vec::new(),
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

/// The opening paragraph of `help`, when it describes a server or watcher.
fn server_or_watcher_description(help: &HelpOutput) -> Option<String> {
    help.description()
        .filter(|description| names_any(description, SERVER_OR_WATCHER_WORDS))
}

fn find_timeout_flag(help: &HelpOutput) -> Option<String> {
    first_flag(help, |f| f.declares_any(TIMEOUT_FLAGS).is_some())
}

/// The verdict for a tool with no long-running subcommand whose top-level help
/// describes a server or watcher, read from that help.
fn audit_described_tool(help: &HelpOutput, description: &str) -> (AuditStatus, Option<String>) {
    let quoted = first_sentence(description);
    match find_timeout_flag(help) {
        Some(flag) => (
            AuditStatus::Pass,
            Some(format!(
                "the top-level help describes a server or watcher (`{quoted}`) and declares {flag}."
            )),
        ),
        None => (
            AuditStatus::Warn(help.noting_dash_rule(
                TIMEOUT_FLAGS,
                &format!(
                    "the top-level help describes a server or watcher (`{quoted}`) and declares no \
                     timeout flag (looked for {}, the names P7 accepts). SHOULD-tier: P7 applies \
                     this to a tool its own help describes as a server or watcher, because an agent \
                     that hits a hung operation otherwise has to enforce a timeout externally.",
                    TIMEOUT_FLAGS.join(", "),
                ),
            )),
            None,
        ),
    }
}

/// The status, plus the Pass evidence naming what carries a timeout flag: the
/// long-running subcommands, or the tool its description puts under the
/// requirement.
pub(crate) fn audit_timeout_behavioral(
    help: &HelpOutput,
    subhelp: &[(String, HelpOutput)],
) -> (AuditStatus, Option<String>) {
    let triggers = long_running_subcommands(help);
    if triggers.is_empty() {
        return match server_or_watcher_description(help) {
            Some(description) => audit_described_tool(help, &description),
            None => (
                AuditStatus::Skip(
                    "no long-running subcommand detected (serve/daemon/watch/tail/monitor/\
                     follow/stream), and the top-level help describes no server or watcher, the \
                     two signals P7 names; vacuous skip for the conditional SHOULD."
                        .into(),
                ),
                None,
            ),
        };
    }

    let cov = coverage(&triggers, help, subhelp, find_timeout_flag);
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
             operation have to enforce timeouts externally.{}",
            cov.without_list(),
            TIMEOUT_FLAGS.join(", "),
            dash_rule_notes(&cov.without, subhelp, TIMEOUT_FLAGS, None),
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

    // Excerpt of miniserve 0.35's `miniserve --help`.
    const MINISERVE_HELP: &str = "\
For when you really just want to serve some files over HTTP right now!

Usage: miniserve [OPTIONS] [PATH]

Arguments:
  [PATH]
          Which path to serve

          [env: MINISERVE_PATH=]

Options:
  -v, --verbose
          Be verbose, includes emitting access logs

  -p, --port <PORT>
          Port to use

  -h, --help
          Print help (see a summary with '-h')
";

    // Excerpt of watchexec 2.7's `watchexec --help`.
    const WATCHEXEC_HELP: &str = "\
Execute commands when watched files change.

Recursively monitors the current directory for changes, executing the command when a filesystem
change is detected (among other event sources).

Usage: watchexec [OPTIONS] [COMMAND]...

Arguments:
  [COMMAND]...
          Command (program and arguments) to run on changes

Options:
      --stop-timeout <TIMEOUT>
          Time to wait for the command to exit gracefully

      --timeout <TIMEOUT>
          Kill the command if it runs longer than this duration

  -h, --help
          Print help (see a summary with '-h')
";

    fn sub(name: &str, raw: &str) -> (String, HelpOutput) {
        (name.to_string(), HelpOutput::from_raw(raw))
    }

    #[test]
    fn a_tool_described_as_a_server_is_graded_and_warns_without_a_timeout() {
        let help = HelpOutput::from_raw(MINISERVE_HELP);
        match audit_timeout_behavioral(&help, &[]) {
            (AuditStatus::Warn(msg), None) => {
                assert!(
                    msg.starts_with(
                        "the top-level help describes a server or watcher (`For when you really \
                         just want to serve some files over HTTP right now!`) and declares no \
                         timeout flag (looked for --timeout, --deadline, --max-time, the names \
                         P7 accepts)."
                    ),
                    "{msg}"
                );
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn a_tool_described_as_a_watcher_is_graded_and_passes_with_a_timeout() {
        let help = HelpOutput::from_raw(WATCHEXEC_HELP);
        let (status, evidence) = audit_timeout_behavioral(&help, &[]);
        assert_eq!(status, AuditStatus::Pass);
        assert_eq!(
            evidence.as_deref(),
            Some(
                "the top-level help describes a server or watcher (`Execute commands when watched \
                 files change.`) and declares --timeout."
            )
        );
    }

    #[test]
    fn a_described_tool_with_a_single_dash_timeout_warns_and_says_why() {
        let help = HelpOutput::from_raw(
            "A static file server.\n\nOptions:\n  -timeout <SECS>  Stop after this long.\n      --help    Show help.\n",
        );
        match audit_timeout_behavioral(&help, &[]).0 {
            AuditStatus::Warn(msg) => assert!(
                msg.ends_with("`-timeout` is declared, but this help also declares double-dash names, so it does not count as `--timeout`."),
                "{msg}"
            ),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn only_the_opening_prose_can_describe_a_server_or_watcher() {
        let cases = [
            // ripgrep: no wording at all.
            (
                "ripgrep (rg) recursively searches the current directory.\n\nUsage: rg [OPTIONS]\n",
                false,
            ),
            // The words count whole, and only in the singular forms.
            ("Reserve a seat.\n\nUsage: tool\n", false),
            ("Manage your MCP servers.\n\nUsage: tool\n", false),
            // A later paragraph and an option's description are not the opening prose.
            (
                "Convert a file.\n\nThe output can be sent to a server.\n\nUsage: tool\n",
                false,
            ),
            (
                "Convert a file.\n\nUsage: tool [OPTIONS]\n\nOptions:\n  --send  Send to a server\n",
                false,
            ),
            // No prose before the first section.
            (
                "Usage: tool [OPTIONS]\n\nOptions:\n  --send  Send to a server\n",
                false,
            ),
            ("A static file server.\n\nUsage: tool\n", true),
            ("Usage: tool\n\nWatches a directory.\n", true),
            ("Run a development daemon.\n\nUsage: tool\n", true),
            ("A process monitor.\n\nUsage: tool\n", true),
        ];
        let verdicts: Vec<(&str, bool)> = cases
            .iter()
            .map(|&(raw, _)| {
                let (status, _) = audit_timeout_behavioral(&HelpOutput::from_raw(raw), &[]);
                (raw, !matches!(status, AuditStatus::Skip(_)))
            })
            .collect();
        assert_eq!(verdicts, cases);
    }

    #[test]
    fn a_long_running_subcommand_outranks_the_description() {
        let help = HelpOutput::from_raw(
            "A static file server.\n\nUsage:\n  tool [command]\n\nAvailable Commands:\n  serve       Start the server\n",
        );
        let subhelp = vec![sub(
            "serve",
            "Usage:\n  tool serve [flags]\n\nFlags:\n      --timeout <SECS>   Stop after this long\n",
        )];
        let (status, evidence) = audit_timeout_behavioral(&help, &subhelp);
        assert_eq!(status, AuditStatus::Pass);
        assert_eq!(
            evidence.as_deref(),
            Some("long-running subcommand(s) with a timeout flag: serve (--timeout).")
        );
    }

    #[test]
    fn the_subcommand_names_p7_reads_trigger_the_gate_and_run_and_start_do_not() {
        let cases = [
            ("serve", "Serve a remote over a protocol", true),
            ("daemon", "Run the background service", true),
            (
                "watch",
                "Run task(s) and rerun them when files change",
                true,
            ),
            ("tail", "Print the end of a log", true),
            ("monitor", "Report on open connections", true),
            ("follow", "Print new posts as they arrive", true),
            ("stream", "Print events as they happen", true),
            // gh, a noun for workflow runs
            ("run:", "View details about workflow runs", false),
            // docker 29.8
            ("run", "Create and run a new container from an image", false),
            ("start", "Start one or more stopped containers", false),
            // kubectl 1.37
            ("run", "Run a particular image on the cluster", false),
        ];
        let verdicts: Vec<(&str, &str, bool)> = cases
            .iter()
            .map(|&(name, summary, _)| {
                let help = HelpOutput::from_raw(format!(
                    "Usage: tool <COMMAND>\n\nCommands:\n  {name}    {summary}\n"
                ));
                let (status, _) = audit_timeout_behavioral(&help, &[]);
                (name, summary, !matches!(status, AuditStatus::Skip(_)))
            })
            .collect();
        assert_eq!(verdicts, cases);
    }

    #[test]
    fn a_timeout_on_a_run_subcommand_earns_no_credit() {
        let help = HelpOutput::from_raw(MISE_HELP);
        let subhelp = vec![sub("run", MISE_RUN_HELP)];
        match audit_timeout_behavioral(&help, &subhelp) {
            (AuditStatus::Warn(msg), None) => {
                assert!(msg.contains("in its --help: watch (looked for"), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_no_long_running_verb() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  build    Build.\n  test    Run tests.\n",
        );
        match audit_timeout_behavioral(&help, &[]).0 {
            AuditStatus::Skip(msg) => {
                assert!(msg.contains("vacuous"), "{msg}");
                assert!(msg.contains("describes no server or watcher"), "{msg}");
            }
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
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  follow    Print new posts.\n  watch     Rerun on change.\n",
        );
        let subhelp = vec![sub(
            "follow",
            "Usage: tool follow\n\nOptions:\n      --timeout <SECS>    Stop after this long.\n",
        )];
        let (status, evidence) = audit_timeout_behavioral(&help, &subhelp);
        assert_eq!(status, AuditStatus::Pass);
        assert_eq!(
            evidence.as_deref(),
            Some(
                "long-running subcommand(s) with a timeout flag: follow (--timeout). Without one: watch."
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

    #[test]
    fn a_single_dash_timeout_in_a_long_running_subcommand_warns_and_says_why() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  serve    Start server.\n\nOptions:\n  -h, --help    Show help.\n",
        );
        let subhelp = vec![sub(
            "serve",
            "Options:\n  -timeout <SECS>    Stop after this long.\n      --help         Show help.\n",
        )];
        match audit_timeout_behavioral(&help, &subhelp).0 {
            AuditStatus::Warn(msg) => assert!(
                msg.ends_with(" In `serve`, `-timeout` is declared, but this help also declares double-dash names, so it does not count as `--timeout`."),
                "{msg}"
            ),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
