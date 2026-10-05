//! Audit: `p7-may-cursor-pagination`.
//!
//! Cursor-based pagination flags (`--after`, `--before`, `--cursor`, `--page`)
//! for efficient traversal of large result sets. MAY-tier; applicability
//! gated on the presence of a list-style subcommand.
//!
//! The flags belong on the list command (`helm list --offset`), so the audit
//! reads each list-style subcommand's own `--help`, plus the top-level help
//! for a flag every subcommand inherits. P7 says the flags MAY be offered,
//! with no claim on every list command, so one list-style subcommand that
//! carries one is a Pass.

use crate::audit::Audit;
use crate::audits::behavioral::list_style::list_style_subcommands;
use crate::audits::behavioral::subcommand_help::{coverage, first_flag, probe_named};
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const CURSOR_FLAGS: &[&str] = &["--after", "--before", "--cursor", "--page", "--offset"];

pub struct CursorPaginationAudit;

impl Audit for CursorPaginationAudit {
    fn id(&self) -> &str {
        "p7-cursor-pagination"
    }

    fn label(&self) -> &'static str {
        "Cursor-based pagination flags for list traversal"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P7
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-may-cursor-pagination"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let (status, pass_evidence) = match project.help_output() {
            None => (AuditStatus::Skip("could not probe --help".into()), None),
            Some(help) => {
                let subhelp = probe_named(project.runner_ref(), &list_style_subcommands(help));
                audit_cursor_pagination(help, &subhelp)
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

/// The status, plus the Pass evidence naming the list-style subcommands
/// that carry a cursor/page flag.
pub(crate) fn audit_cursor_pagination(
    help: &HelpOutput,
    subhelp: &[(String, HelpOutput)],
) -> (AuditStatus, Option<String>) {
    let triggers = list_style_subcommands(help);
    if triggers.is_empty() {
        return (
            AuditStatus::Skip(
                "no list-style subcommand detected; vacuous skip for the list-only MAY.".into(),
            ),
            None,
        );
    }

    let cov = coverage(&triggers, help, subhelp, |h| {
        first_flag(h, |f| CURSOR_FLAGS.iter().any(|name| f.matches(name)))
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
                "list-style subcommand(s) with a cursor/page flag: {}.{lacking}",
                cov.with_list()
            )),
        );
    }
    (
        AuditStatus::Warn(format!(
            "no list-style subcommand advertises a cursor/page flag in its --help: {} \
             (looked for {}). MAY-tier: cursor pagination lets agents traverse large \
             result sets without re-scanning earlier pages.",
            cov.without_list(),
            CURSOR_FLAGS.join(", "),
        )),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Excerpts of helm 4.3's `helm --help` and `helm list --help`.
    const HELM_HELP: &str = "\
Usage:
  helm [command]

Available Commands:
  get         download extended information of a named release
  install     install a chart
  list        list releases
  search      search for a keyword in charts
  show        show information of a chart

Flags:
      --burst-limit int                 client-side default throttling limit (default 100)
  -h, --help                            help for helm
  -n, --namespace string                namespace scope for this request
";

    const HELM_LIST_HELP: &str = "\
Usage:
  helm list [flags]

Aliases:
  list, ls

Flags:
  -A, --all-namespaces       list releases across all namespaces
  -h, --help                 help for list
  -m, --max int              maximum number of releases to fetch (default 256)
      --offset int           next release index in the list, used to offset from start value

Global Flags:
  -n, --namespace string                namespace scope for this request
";

    // Excerpts of ollama 0.35's `ollama --help` and `ollama list --help`.
    const OLLAMA_HELP: &str = "\
Large language model runner

Usage:
  ollama [flags]
  ollama [command]

Available Commands:
  serve        Start Ollama
  show         Show information for a model
  run          Run a model
  list         List models
  rm           Remove a model

Flags:
  -h, --help         help for ollama
      --verbose      Show timings for response
  -v, --version      Show version information
";

    const OLLAMA_LIST_HELP: &str = "\
List models

Usage:
  ollama list [flags]

Aliases:
  list, ls

Flags:
  -h, --help   help for list
";

    fn sub(name: &str, raw: &str) -> (String, HelpOutput) {
        (name.to_string(), HelpOutput::from_raw(raw))
    }

    #[test]
    fn skip_when_no_list_subcommand() {
        let help =
            HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  audit    Run audits.\n");
        match audit_cursor_pagination(&help, &[]).0 {
            AuditStatus::Skip(msg) => assert!(msg.contains("vacuous")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn pass_with_cursor_flag() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n\n\
             Options:\n      --cursor <C>    Pagination cursor.\n  -h, --help\n",
        );
        assert_eq!(audit_cursor_pagination(&help, &[]).0, AuditStatus::Pass);
    }

    #[test]
    fn pass_with_after_before_flags() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  search   Search items.\n\n\
             Options:\n      --after <ID>    Start after.\n      --before <ID>    End before.\n",
        );
        assert_eq!(audit_cursor_pagination(&help, &[]).0, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_list_without_cursor() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        match audit_cursor_pagination(&help, &[]).0 {
            AuditStatus::Warn(msg) => assert!(msg.contains("cursor")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn offset_flag_on_one_list_subcommand_passes() {
        let help = HelpOutput::from_raw(HELM_HELP);
        let subhelp = vec![sub("list", HELM_LIST_HELP)];
        let (status, evidence) = audit_cursor_pagination(&help, &subhelp);
        assert_eq!(status, AuditStatus::Pass);
        let evidence = evidence.expect("pass names the subcommand");
        assert!(evidence.contains("list (--offset)"), "{evidence}");
        assert!(evidence.contains("Without one: "), "{evidence}");
    }

    #[test]
    fn warn_names_the_list_subcommands_it_read() {
        let help = HelpOutput::from_raw(OLLAMA_HELP);
        let subhelp = vec![sub("list", OLLAMA_LIST_HELP)];
        match audit_cursor_pagination(&help, &subhelp) {
            (AuditStatus::Warn(msg), None) => {
                assert!(msg.contains("list (looked for"), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
