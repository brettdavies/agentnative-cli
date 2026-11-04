//! Audit: `p3-should-unprefixed-command-list`.
//!
//! A command list whose entries repeat the binary name (`tool status`,
//! `tool server stop`) makes a reader or agent strip the prefix before the
//! command token is visible. The audit reads the command blocks the help
//! probe parsed: a block whose entries all led with the tool name is
//! prefixed, and every entry that names a command beyond that bare prefix
//! is an offender. The bare-invocation entry, which documents what the tool
//! does with no arguments, is not one, and neither is a default-command
//! entry that adds only argument placeholders (`opencode [project]`,
//! `tool FILE`): it documents the same bare invocation and names no
//! subcommand. The row's evidence names any such entry it did not count.
//!
//! Warn when any offender exists, Pass when every graded block is
//! unprefixed, NotApplicable when no subcommand names were parsed from the
//! help, the same gate `p3-subcommand-examples` and `p6-standard-names` use.
//! An examples block is parsed like any other command block but is not
//! graded; see [`is_graded`].

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::help_probe::is_placeholder;
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
        let (status, pass_evidence) = match project.help_output() {
            None => (AuditStatus::Skip("could not probe --help".into()), None),
            Some(help) => {
                let status = audit_unprefixed_command_list(help);
                let evidence = if status == AuditStatus::Pass {
                    default_line_note(help)
                } else {
                    None
                };
                (status, evidence)
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

pub(crate) fn audit_unprefixed_command_list(help: &HelpOutput) -> AuditStatus {
    if help.subcommands().is_empty() {
        return AuditStatus::NotApplicable(format!(
            "{}; the SHOULD applies to CLIs that list subcommands.",
            help.missing_subcommands_reason()
        ));
    }
    let offenders: Vec<(&str, &str)> = prefixed_commands(help)
        .filter(|(_, command)| !is_bare_invocation(command))
        .collect();
    let Some((prefix, _)) = offenders.first() else {
        return AuditStatus::Pass;
    };
    let note = default_line_note(help).map_or(String::new(), |note| format!(" {note}"));
    AuditStatus::Warn(format!(
        "command-list entries repeat the binary name `{prefix}`: {}. Drop the prefix \
         so a reader or agent scanning the block gets the command token directly instead \
         of reconstructing it from a repeated binary name.{note}",
        quoted(&offenders)
    ))
}

/// What the row says about the default-command entries it did not count:
/// what it found, what the requirement accepts, and the principle that says
/// so. `None` when no entry adds placeholders to the bare invocation.
fn default_line_note(help: &HelpOutput) -> Option<String> {
    let lines: Vec<(&str, &str)> = prefixed_commands(help)
        .filter(|(_, command)| !command.is_empty() && is_bare_invocation(command))
        .collect();
    (!lines.is_empty()).then(|| {
        format!(
            "Not counted, because P3 lets the bare invocation line add argument placeholders: {}.",
            quoted(&lines)
        )
    })
}

/// Up to [`EVIDENCE_ENTRY_LIMIT`] entries as written, then a count of the rest.
fn quoted(entries: &[(&str, &str)]) -> String {
    let listed: Vec<String> = entries
        .iter()
        .take(EVIDENCE_ENTRY_LIMIT)
        .map(|(prefix, command)| format!("`{prefix} {command}`"))
        .collect();
    let more = entries.len().saturating_sub(EVIDENCE_ENTRY_LIMIT);
    let rest = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    format!("{}{rest}", listed.join(", "))
}

/// Whether a block's entries are graded. An examples section whose header
/// ends in `commands:` (`Example commands:`) parses as a command block, but
/// examples are the sanctioned home for the binary name: P3's examples
/// requirement asks for full invocations, so grading them would penalise
/// exactly what it asks for.
fn is_graded(block: &CommandBlock) -> bool {
    !block.header.to_ascii_lowercase().contains("example")
}

/// The prefix and command text of each entry in a graded prefixed block, as
/// written up to the description gap (`tool`, `status [server|client]`).
fn prefixed_commands(help: &HelpOutput) -> impl Iterator<Item = (&str, &str)> {
    help.command_blocks()
        .iter()
        .filter(|block| is_graded(block))
        .flat_map(|block| {
            block.prefix.as_deref().into_iter().flat_map(move |prefix| {
                block
                    .entries
                    .iter()
                    .map(move |entry| (prefix, block.command_text(entry)))
            })
        })
}

/// Whether an entry's command text names no subcommand: empty, or only
/// argument placeholders.
fn is_bare_invocation(command: &str) -> bool {
    command.split_whitespace().all(is_placeholder)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audits::behavioral::tests::test_project_with_sh_script;

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

    #[test]
    fn default_command_line_with_only_placeholders_is_not_an_offender() {
        // Excerpt of opencode 1.18's `opencode --help`, which has no
        // `Usage:` line, so the binary name is the prefix signal.
        let help = HelpOutput::from_raw_for_binary(
            "\
Commands:
  opencode completion          generate shell completion script
  opencode acp                 start ACP (Agent Client Protocol) server
  opencode mcp                 manage MCP (Model Context Protocol) servers
  opencode [project]           start opencode tui                                          [default]
  opencode attach <url>        attach to a running opencode server
  opencode run [message..]     run opencode with a message

Positionals:
  project  path to start opencode in                                                        [string]
",
            "opencode",
        );
        let (offenders, note) = offenders_and_note(&help);
        assert!(
            offenders.contains("`opencode run [message..]`"),
            "{offenders}"
        );
        assert!(!offenders.contains("[project]"), "{offenders}");
        assert!(!offenders.contains(" more"), "{offenders}");
        assert!(note.contains("`opencode [project]`"), "{note}");
    }

    #[test]
    fn gemini_default_line_is_not_an_offender() {
        // Excerpt of gemini 0.66's `gemini --help`.
        let help = HelpOutput::from_raw_for_binary(
            "\
Usage: gemini [options] [command]

Commands:
  gemini mcp                   Manage MCP servers
  gemini gemma                 Manage local Gemma model routing
  gemini [query..]             Launch Gemini CLI  [default]
",
            "gemini",
        );
        let (offenders, note) = offenders_and_note(&help);
        assert!(offenders.contains("`gemini mcp`"), "{offenders}");
        assert!(!offenders.contains("[query..]"), "{offenders}");
        assert!(note.contains("`gemini [query..]`"), "{note}");
    }

    #[test]
    fn an_all_caps_metavariable_is_a_placeholder_but_a_command_word_is_not() {
        let help = HelpOutput::from_raw(
            "Usage: tool [options]\n\nCommands:\n  tool FILE       Open a file\n  \
             tool run FILE   Run a file\n",
        );
        let (offenders, note) = offenders_and_note(&help);
        assert!(offenders.contains("`tool run FILE`"), "{offenders}");
        assert!(!offenders.contains("`tool FILE`"), "{offenders}");
        assert!(note.contains("`tool FILE`"), "{note}");
        assert!(note.contains("P3"), "{note}");
    }

    #[test]
    fn a_pass_that_rests_on_a_default_line_says_so_in_its_evidence() {
        let project = test_project_with_sh_script(
            r#"printf 'Usage: tool [options]\n\nCommands:\n  status   Show status\n\nDefault commands:\n  tool FILE   Open a file\n'"#,
        );
        let result = UnprefixedCommandListAudit
            .run(&project)
            .expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass);
        let evidence = result.pass_evidence.expect("pass evidence");
        assert!(evidence.contains("`tool FILE`"), "{evidence}");
        assert!(evidence.contains("P3"), "{evidence}");
    }

    #[test]
    fn a_placeholder_is_bracketed_or_all_caps() {
        for command in [
            "",
            "[project]",
            "<path>",
            "[message..]",
            "FILE",
            "FILE...",
            "FILE_PATH",
            "KEY=VALUE",
            "[options] FILE",
            "FILE <dest>",
        ] {
            assert!(is_bare_invocation(command), "{command}");
        }
        for command in [
            "run", "run FILE", "FILE run", "File", "fILE", "2FA", "--init", "-",
        ] {
            assert!(!is_bare_invocation(command), "{command}");
        }
    }

    /// The Warn evidence split at its advice: the entries it lists, then the
    /// default-command note after them.
    fn offenders_and_note(help: &HelpOutput) -> (String, String) {
        match audit_unprefixed_command_list(help) {
            AuditStatus::Warn(msg) => {
                let (offenders, note) = msg.split_once("Drop the prefix").expect("advice");
                (offenders.to_string(), note.to_string())
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
