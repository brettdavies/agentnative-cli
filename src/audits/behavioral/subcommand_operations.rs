//! Audit: `p6-should-subcommand-operations`.
//!
//! Operations are modeled as subcommands, not flags — `tool search "q"` not
//! `tool --search "q"`. The flag-as-verb pattern collides with the option
//! namespace and makes the operation set harder to discover via the top-level
//! `Commands:` block agents already grep.
//!
//! Rubric: scan the top-level help flag list for verb-shaped long names
//! (`--search`, `--list`, `--delete`, `--create`, `--update`, …) and Warn
//! when any are present. Pass when no verb-flag is found. Vacuous Skip when
//! the help surface is unavailable.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::runner::help_probe::Flag;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Verb fragments that, when used as the long form of a top-level flag,
/// indicate a flag-as-operation anti-pattern. Match is exact-long-name:
/// `--search` triggers, but `--search-path` (a parameter) does not.
const VERB_FLAGS: &[&str] = &[
    "--search",
    "--list",
    "--delete",
    "--remove",
    "--create",
    "--add",
    "--update",
    "--set",
    "--get",
    "--show",
    "--find",
    "--query",
    "--destroy",
    "--purge",
    "--reset",
    "--drop",
    "--clean",
    "--install",
    "--uninstall",
    "--upgrade",
    "--build",
    "--run",
    "--exec",
];

pub struct SubcommandOperationsAudit;

impl Audit for SubcommandOperationsAudit {
    fn id(&self) -> &str {
        "p6-subcommand-operations"
    }

    fn label(&self) -> &'static str {
        "Operations are subcommands, not verb-shaped flags"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-should-subcommand-operations"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_subcommand_operations(help),
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
            pass_evidence: None,
        })
    }
}

pub(crate) fn audit_subcommand_operations(help: &HelpOutput) -> AuditStatus {
    let verb_flags: Vec<&str> = help
        .flags()
        .iter()
        .flat_map(Flag::long_names)
        .filter(|long| {
            VERB_FLAGS
                .iter()
                .any(|verb| names_the_same_word(long, verb))
        })
        .collect();

    if verb_flags.is_empty() {
        return AuditStatus::Pass;
    }
    AuditStatus::Warn(format!(
        "top-level verb-shaped flag(s) found: {}. Operations belong under \
         the `Commands:` block (`tool search \"q\"`), not on the flag \
         namespace where they fight the `--help` filtering agents rely on.",
        verb_flags.join(", ")
    ))
}

/// Whether a long name and a verb flag are the same word, whatever their
/// case or dash count: a long name is a single-dash word only where one dash
/// and two name the same flag.
fn names_the_same_word(long: &str, verb: &str) -> bool {
    long.trim_start_matches('-')
        .eq_ignore_ascii_case(verb.trim_start_matches('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_dash_verb_flag_counts_in_a_help_without_double_dash_names() {
        let help = HelpOutput::from_raw(
            "Usage of tool:\n  -search string\n    \tFind entries that match\n  -v\tverbose\n",
        );
        match audit_subcommand_operations(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("found: -search."), "{msg}"),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn a_single_dash_verb_word_is_not_a_long_flag_beside_double_dash_names() {
        let help = HelpOutput::from_raw(
            "Options:\n  -search <pattern>  Find entries that match\n      --help         Show help\n",
        );
        assert_eq!(audit_subcommand_operations(&help), AuditStatus::Pass);
    }

    #[test]
    fn pass_when_no_verb_flags() {
        let help = HelpOutput::from_raw(
            "Usage: tool [OPTIONS]\n\n\
             Options:\n  --output <FMT>    Output format.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_subcommand_operations(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_on_search_flag() {
        let help = HelpOutput::from_raw(
            "Usage: tool [OPTIONS]\n\n\
             Options:\n      --search <Q>    Search items.\n  -h, --help    Show help.\n",
        );
        match audit_subcommand_operations(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("--search")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn warn_on_multiple_verb_flags() {
        let help = HelpOutput::from_raw(
            "Usage: tool [OPTIONS]\n\n\
             Options:\n      --list    List items.\n      --delete <ID>    Delete an item.\n  -h, --help    Show help.\n",
        );
        match audit_subcommand_operations(&help) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("--list"));
                assert!(msg.contains("--delete"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn pass_with_search_path_parameter() {
        // `--search-path` is a parameter, not a verb flag. We match exact
        // long names, so it must not trigger.
        let help = HelpOutput::from_raw(
            "Usage: tool [OPTIONS]\n\n\
             Options:\n      --search-path <DIR>    Path to search.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_subcommand_operations(&help), AuditStatus::Pass);
    }
}
