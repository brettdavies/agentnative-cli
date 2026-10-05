//! Audit: `p7-should-limit`.
//!
//! A `--limit` or `--max-results` flag lets callers request exactly the
//! number of items they want from list-style commands. SHOULD-tier;
//! applicability is gated on the presence of a list-style subcommand
//! (see `list_style::list_style_subcommands`).
//!
//! The flag belongs on the list command (`helm list --max`), so the audit
//! reads each list-style subcommand's own `--help`, plus the top-level help
//! for a flag every subcommand inherits. P7's evidence asks for the flag on
//! every list / search command, so Pass needs all of them to carry one.

use crate::audit::Audit;
use crate::audits::behavioral::list_style::list_style_subcommands;
use crate::audits::behavioral::subcommand_help::{coverage, first_flag, probe_named};
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::runner::help_probe::Flag;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const LIMIT_FLAGS: &[&str] = &["--limit", "--max-results", "--max", "--top", "-n"];

pub struct LimitFlagAudit;

impl Audit for LimitFlagAudit {
    fn id(&self) -> &str {
        "p7-limit"
    }

    fn label(&self) -> &'static str {
        "`--limit` / `--max-results` flag for list operations"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P7
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-should-limit"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let (status, pass_evidence) = match project.help_output() {
            None => (AuditStatus::Skip("could not probe --help".into()), None),
            Some(help) => {
                let subhelp = probe_named(project.runner_ref(), &list_style_subcommands(help));
                audit_limit_flag(help, &subhelp)
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

/// The status, plus the Pass evidence naming each list-style subcommand and
/// the limit flag it carries.
pub(crate) fn audit_limit_flag(
    help: &HelpOutput,
    subhelp: &[(String, HelpOutput)],
) -> (AuditStatus, Option<String>) {
    let triggers = list_style_subcommands(help);
    if triggers.is_empty() {
        return (
            AuditStatus::Skip(
                "no list-style subcommand detected (list/ls/search/query/find/show/get); \
                 vacuous skip for the list-only SHOULD."
                    .into(),
            ),
            None,
        );
    }

    let cov = coverage(&triggers, help, subhelp, find_limit_flag);
    if cov.without.is_empty() {
        return (
            AuditStatus::Pass,
            Some(format!(
                "every list-style subcommand advertises a limit flag: {}.",
                cov.with_list()
            )),
        );
    }
    let carried = if cov.with.is_empty() {
        String::new()
    } else {
        format!(" Carries one: {}.", cov.with_list())
    };
    (
        AuditStatus::Warn(format!(
            "list-style subcommand(s) with no limit flag in their --help: {} \
             (looked for {}).{carried} SHOULD-tier: callers should be able to bound \
             response size directly rather than scrape-then-truncate.",
            cov.without_list(),
            LIMIT_FLAGS.join(", "),
        )),
        None,
    )
}

fn find_limit_flag(help: &HelpOutput) -> Option<String> {
    first_flag(help, is_limit_flag)
}

fn is_limit_flag(flag: &Flag) -> bool {
    LIMIT_FLAGS.iter().any(|name| flag.matches(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Excerpts of docker 29.8's `docker --help` and `docker search --help`.
    const DOCKER_HELP: &str = "\
Usage:  docker [OPTIONS] COMMAND

A self-sufficient runtime for containers

Common Commands:
  run         Create and run a new container from an image
  ps          List containers
  search      Search Docker Hub for images
  version     Show the Docker version information

Global Options:
      --config string      Location of client config files (default
                           \"/home/runner/.docker\")
  -D, --debug              Enable debug mode
  -H, --host string        Daemon socket to connect to
";

    const DOCKER_SEARCH_HELP: &str = "\
Usage:  docker search [OPTIONS] TERM

Search Docker Hub for images

Options:
  -f, --filter filter   Filter output based on conditions provided
      --format string   Pretty-print search using a Go template
      --limit int       Max number of search results
      --no-trunc        Don't truncate output
";

    // Excerpts of rclone 1.75's `rclone --help` and `rclone ls --help`.
    const RCLONE_HELP: &str = "\
Usage:
  rclone [flags]
  rclone [command]

Available commands:
  copy        Copy files from source to dest, skipping identical files.
  ls          List the objects in the path with size and path.
  lsd         List all directories/containers/buckets in the path.
  serve       Serve a remote over a protocol.

Use \"rclone [command] --help\" for more information about a command.
";

    const RCLONE_LS_HELP: &str = "\
Usage:
  rclone ls remote:path [flags]

Flags:
  -h, --help   help for ls

Filter Options:
      --max-age Duration                    Only transfer files younger than this in s or suffix ms|s|m|h|d|w|M|y (default off)
      --max-depth int                       If set limits the recursion depth to this (default -1)
";

    fn sub(name: &str, raw: &str) -> (String, HelpOutput) {
        (name.to_string(), HelpOutput::from_raw(raw))
    }

    #[test]
    fn skip_when_no_list_subcommand() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  audit    Run audits.\n  build    Build.\n",
        );
        match audit_limit_flag(&help, &[]).0 {
            AuditStatus::Skip(msg) => assert!(msg.contains("vacuous")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn pass_when_list_and_limit_present() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n\n\
             Options:\n      --limit <N>    Max items.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_limit_flag(&help, &[]).0, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_list_without_limit() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        match audit_limit_flag(&help, &[]).0 {
            AuditStatus::Warn(msg) => assert!(msg.contains("--limit")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn limit_flag_on_the_list_subcommand_passes() {
        let help = HelpOutput::from_raw(DOCKER_HELP);
        let subhelp = vec![sub("search", DOCKER_SEARCH_HELP)];
        let (status, evidence) = audit_limit_flag(&help, &subhelp);
        assert_eq!(status, AuditStatus::Pass);
        assert_eq!(
            evidence.as_deref(),
            Some("every list-style subcommand advertises a limit flag: search (--limit).")
        );
    }

    #[test]
    fn warn_names_the_list_subcommand_without_a_limit() {
        let help = HelpOutput::from_raw(RCLONE_HELP);
        let subhelp = vec![sub("ls", RCLONE_LS_HELP)];
        match audit_limit_flag(&help, &subhelp) {
            (AuditStatus::Warn(msg), None) => {
                assert!(msg.contains("in their --help: ls (looked for"), "{msg}");
                assert!(!msg.contains("Carries one"), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
