//! Audit: `p3-should-unprefixed-command-list`.
//!
//! A command list whose entries repeat the binary name (`tool status`,
//! `tool server stop`) makes a reader or agent strip the prefix before the
//! command token is visible. The audit reads the command blocks the help
//! probe parsed: a block whose entries all led with the tool name is
//! prefixed, and every entry that names a command beyond that bare prefix
//! is an offender. The bare-invocation entry, which documents what the tool
//! does with no arguments, is not one.
//!
//! Warn when any offender exists, Pass when every graded block is
//! unprefixed, NotApplicable when no subcommand names were parsed from the
//! help, the same gate `p3-subcommand-examples` and `p6-standard-names` use.
//! An examples block is parsed like any other command block but is not
//! graded; see [`is_graded`].

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::{CommandBlock, HelpOutput};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Offending entries quoted in the evidence; the rest are summarised as a count.
const EVIDENCE_ENTRY_LIMIT: usize = 5;

pub struct UnprefixedCommandListAudit;

impl Audit for UnprefixedCommandListAudit {
    fn id(&self) -> &str {
        "p3-unprefixed-command-list"
    }

    fn label(&self) -> &'static str {
        "Command-list entries name the command without repeating the binary name"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P3
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-should-unprefixed-command-list"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_unprefixed_command_list(help),
        };
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: None,
        })
    }
}

pub(crate) fn audit_unprefixed_command_list(help: &HelpOutput) -> AuditStatus {
    if help.subcommands().is_empty() {
        return AuditStatus::NotApplicable(format!(
            "{}; the SHOULD applies to CLIs that list subcommands.",
            help.missing_subcommands_reason()
        ));
    }
    let offenders: Vec<(&str, &str)> = help
        .command_blocks()
        .iter()
        .filter(|block| is_graded(block))
        .flat_map(prefixed_commands)
        .collect();
    let Some((prefix, _)) = offenders.first() else {
        return AuditStatus::Pass;
    };
    let quoted: Vec<String> = offenders
        .iter()
        .take(EVIDENCE_ENTRY_LIMIT)
        .map(|(prefix, command)| format!("`{prefix} {command}`"))
        .collect();
    let more = offenders.len().saturating_sub(EVIDENCE_ENTRY_LIMIT);
    let rest = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    AuditStatus::Warn(format!(
        "command-list entries repeat the binary name `{prefix}`: {}{rest}. Drop the prefix \
         so a reader or agent scanning the block gets the command token directly instead \
         of reconstructing it from a repeated binary name.",
        quoted.join(", ")
    ))
}

/// Whether a block's entries are graded. An examples section whose header
/// ends in `commands:` (`Example commands:`) parses as a command block, but
/// examples are the sanctioned home for the binary name: P3's examples
/// requirement asks for full invocations, so grading them would penalise
/// exactly what it asks for.
fn is_graded(block: &CommandBlock) -> bool {
    !block.header.to_ascii_lowercase().contains("example")
}

/// The prefix and command text of each entry in a prefixed block that names
/// a command beyond the bare prefix, as written up to the description gap
/// (`tool`, `status [server|client]`).
fn prefixed_commands(block: &CommandBlock) -> impl Iterator<Item = (&str, &str)> {
    block.prefix.as_deref().into_iter().flat_map(move |prefix| {
        block
            .entries
            .iter()
            .map(|entry| block.command_text(entry))
            .filter(|command| !command.is_empty())
            .map(move |command| (prefix, command))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIXED_HELP: &str = "\
Usage: herdr [options]

Common commands:
  herdr                            Launch or attach to the persistent session
  herdr status [server|client]     Show local client and running server status
  herdr server stop                Stop the running server via the API socket
  herdr machine <subcommand>       Manage saved SSH machines

Advanced commands:
  herdr server                     Run as headless server
";

    const CLAP_HELP: &str = "\
Usage: anc <COMMAND>

Commands:
  audit        Run audits against a CLI project or binary
  completions  Generate shell completions
  help         Print this message or the help of the given subcommand
";

    #[test]
    fn prefixed_block_warns_and_names_the_offending_lines() {
        let help = HelpOutput::from_raw(PREFIXED_HELP);
        match audit_unprefixed_command_list(&help) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("`herdr status [server|client]`"), "{msg}");
                assert!(msg.contains("`herdr server stop`"), "{msg}");
                assert!(msg.contains("`herdr server`"), "{msg}");
                assert!(
                    !msg.contains("`herdr `"),
                    "bare invocation must not be listed: {msg}"
                );
                assert!(
                    !msg.contains(" more"),
                    "four offenders fit within the quote limit, so no count suffix: {msg}"
                );
                assert!(!msg.contains("Launch"), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn evidence_caps_the_quoted_entries_and_counts_the_rest() {
        let mut raw = String::from("Usage: tool\n\nCommands:\n");
        for i in 0..8 {
            raw.push_str(&format!("  tool cmd{i}   Do thing {i}\n"));
        }
        let help = HelpOutput::from_raw(raw);
        match audit_unprefixed_command_list(&help) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("`tool cmd4`"), "{msg}");
                assert!(!msg.contains("`tool cmd5`"), "{msg}");
                assert!(msg.contains("and 3 more"), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn clap_block_passes() {
        let help = HelpOutput::from_raw(CLAP_HELP);
        assert_eq!(audit_unprefixed_command_list(&help), AuditStatus::Pass);
    }

    #[test]
    fn no_command_block_is_not_applicable() {
        let help =
            HelpOutput::from_raw("Usage: tool [OPTIONS]\n\nOptions:\n  -h, --help  Print help\n");
        match audit_unprefixed_command_list(&help) {
            AuditStatus::NotApplicable(msg) => {
                assert!(msg.contains("no command block found"), "{msg}");
                assert!(
                    msg.contains("the SHOULD applies to CLIs that list subcommands"),
                    "{msg}"
                );
            }
            other => panic!("expected NotApplicable, got {other:?}"),
        }
    }

    #[test]
    fn block_with_no_parseable_names_is_not_applicable() {
        let help = HelpOutput::from_raw(
            "Usage: modes [options]\n\nCommands:\n  modes --init    Initialize\n  modes --build   Build\n",
        );
        match audit_unprefixed_command_list(&help) {
            AuditStatus::NotApplicable(msg) => {
                assert!(msg.contains("but no subcommand names"), "{msg}");
            }
            other => panic!("expected NotApplicable, got {other:?}"),
        }
    }

    #[test]
    fn example_block_is_not_graded() {
        let help = HelpOutput::from_raw(
            "Usage: exa <COMMAND>\n\nCommands:\n  init   Create\n  build  Compile\n\n\
             Example commands:\n  exa init --name foo\n  exa build --release\n",
        );
        assert_eq!(audit_unprefixed_command_list(&help), AuditStatus::Pass);
    }

    #[test]
    fn mixed_blocks_grade_only_the_prefixed_one() {
        let help = HelpOutput::from_raw(
            "Usage: tool [options]\n\nCommands:\n  status   Show status\n\n\
             Advanced commands:\n  tool server   Run the server\n  tool debug    Debug it\n",
        );
        match audit_unprefixed_command_list(&help) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("`tool server`"), "{msg}");
                assert!(msg.contains("`tool debug`"), "{msg}");
                assert!(!msg.contains("status"), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn entry_that_merely_starts_with_the_tool_name_does_not_warn() {
        let help = HelpOutput::from_raw(
            "Usage: tool <COMMAND>\n\nCommands:\n  tool     Run the tool\n  status   Show status\n",
        );
        assert_eq!(audit_unprefixed_command_list(&help), AuditStatus::Pass);
    }

    #[test]
    fn bare_invocation_alone_does_not_warn() {
        let help = HelpOutput::from_raw(
            "Usage: tool [options]\n\nCommands:\n  tool     Launch the interactive session\n",
        );
        assert!(matches!(
            audit_unprefixed_command_list(&help),
            AuditStatus::NotApplicable(_)
        ));
    }
}
