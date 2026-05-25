//! Check: `p2-must-schema-print`.
//!
//! When a CLI emits structured output, it MUST expose its output schema via a
//! `schema` subcommand or `--schema` flag. Runtime-discoverable schemas let
//! agents pin against shape changes across versions; without one, every
//! consumer infers the shape from sample output and breaks on every change.
//!
//! Applicability: gates on a help-text probe — only fires when the help
//! mentions any structured-output indicator (`--output`, `--format`, `--json`,
//! `--jsonl`, or the words "json"/"jsonl"). When the probe finds no such
//! indicator the check Skips with evidence; when it does, the check looks for
//! either a `schema` subcommand or `--schema` flag.

use crate::check::Check;
use crate::checks::behavioral::subcommand_help::probe_subcommands;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

const STRUCTURED_OUTPUT_FLAG_NAMES: &[&str] =
    &["--output", "--format", "--json", "--jsonl", "--ndjson"];

const STRUCTURED_OUTPUT_TOKENS: &[&str] = &["json", "jsonl", "ndjson", "JSON Lines"];

pub struct SchemaPrintCheck;

impl Check for SchemaPrintCheck {
    fn id(&self) -> &str {
        "p2-schema-print"
    }

    fn label(&self) -> &'static str {
        "Structured-output CLI exposes its schema at runtime"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P2
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-must-schema-print"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => {
                // First try the top-level help only. If that's inconclusive,
                // walk one level into each top-level subcommand to find
                // `schema` exposed as a nested subcommand (e.g.,
                // `anc emit schema`). One-level walk matches how an agent
                // would discover the surface via `--help` chaining.
                match check_schema_print(help) {
                    CheckStatus::Fail(_) => {
                        let runner = project.runner_ref();
                        let subhelp = probe_subcommands(runner, help);
                        check_schema_print_with_subhelp(help, &subhelp)
                    }
                    other => other,
                }
            }
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

/// Core unit for tests. Returns Skip when no structured-output indicator is
/// present (vacuous applicability), Pass when a schema surface is advertised,
/// Fail when structured output is advertised without a schema surface.
pub(crate) fn check_schema_print(help: &HelpOutput) -> CheckStatus {
    let raw = help.raw();
    let raw_lower = raw.to_lowercase();
    let has_structured_flag = help
        .flags()
        .iter()
        .any(|f| STRUCTURED_OUTPUT_FLAG_NAMES.iter().any(|n| f.matches(n)));
    let has_structured_token = STRUCTURED_OUTPUT_TOKENS
        .iter()
        .any(|t| raw_lower.contains(&t.to_lowercase()));

    if !has_structured_flag && !has_structured_token {
        return CheckStatus::Skip(
            "no structured-output indicator (--output / --format / json / jsonl) in --help".into(),
        );
    }

    let has_schema_flag = help.flags().iter().any(|f| f.matches("--schema"));
    if has_schema_flag {
        return CheckStatus::Pass;
    }

    // Look for `schema` as a subcommand. Accept either parsed subcommands or
    // a literal `^  schema  ` line that the parser may have skipped.
    let schema_in_subcommands = help
        .subcommands()
        .iter()
        .any(|s| s.eq_ignore_ascii_case("schema"));
    let schema_section_match = raw
        .lines()
        .any(|line| line.starts_with("  ") && line.trim_start().starts_with("schema"));
    if schema_in_subcommands || schema_section_match {
        return CheckStatus::Pass;
    }

    CheckStatus::Fail(
        "CLI emits structured output but exposes no `schema` subcommand or \
         `--schema` flag. Agents need a runtime-discoverable schema to pin \
         against shape changes."
            .into(),
    )
}

/// Extended check that also walks one level into each top-level subcommand
/// to find `schema` exposed as a nested verb (e.g., `anc emit schema`,
/// `anc emit schema`). Mirrors how an agent discovers the surface by
/// chaining `--help` calls — depth-1 walks are the realistic discovery
/// bound for an agent that does not have prior knowledge of the CLI.
pub(crate) fn check_schema_print_with_subhelp(
    top_help: &HelpOutput,
    subhelp: &[(String, HelpOutput)],
) -> CheckStatus {
    // Re-run the top-level check first so the applicability gate and
    // top-level positives short-circuit before we inspect nested help.
    match check_schema_print(top_help) {
        CheckStatus::Fail(_) => {}
        other => return other,
    }

    for (_name, help) in subhelp {
        let has_schema_flag = help.flags().iter().any(|f| f.matches("--schema"));
        if has_schema_flag {
            return CheckStatus::Pass;
        }
        let schema_in_subs = help
            .subcommands()
            .iter()
            .any(|s| s.eq_ignore_ascii_case("schema"));
        if schema_in_subs {
            return CheckStatus::Pass;
        }
        let schema_section_match = help
            .raw()
            .lines()
            .any(|line| line.starts_with("  ") && line.trim_start().starts_with("schema"));
        if schema_section_match {
            return CheckStatus::Pass;
        }
    }

    CheckStatus::Fail(
        "CLI emits structured output but exposes no `schema` subcommand or \
         `--schema` flag at top level or nested one level deep. Agents need \
         a runtime-discoverable schema to pin against shape changes."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELP_WITH_SCHEMA_SUBCMD: &str = r#"Usage: tool [OPTIONS] [COMMAND]

Commands:
  check    Run checks
  schema   Print the JSON output schema

Options:
      --output <FORMAT>   Output format (text or json)
  -h, --help              Show help
"#;

    const HELP_WITH_SCHEMA_FLAG: &str = r#"Usage: tool [OPTIONS]

Options:
      --output <FORMAT>   Output format
      --schema            Print the JSON output schema
  -h, --help              Show help
"#;

    const HELP_NO_STRUCTURED_OUTPUT: &str = r#"Usage: tool [OPTIONS]

Options:
  -q, --quiet     Suppress output
  -h, --help      Show help
"#;

    const HELP_STRUCTURED_NO_SCHEMA: &str = r#"Usage: tool [OPTIONS]

Outputs JSON when --json is set.

Options:
      --json          Emit JSON
  -h, --help          Show help
"#;

    #[test]
    fn happy_path_schema_subcommand() {
        let help = HelpOutput::from_raw(HELP_WITH_SCHEMA_SUBCMD);
        assert_eq!(check_schema_print(&help), CheckStatus::Pass);
    }

    #[test]
    fn happy_path_schema_flag() {
        let help = HelpOutput::from_raw(HELP_WITH_SCHEMA_FLAG);
        assert_eq!(check_schema_print(&help), CheckStatus::Pass);
    }

    #[test]
    fn skip_no_structured_output_indicator() {
        let help = HelpOutput::from_raw(HELP_NO_STRUCTURED_OUTPUT);
        match check_schema_print(&help) {
            CheckStatus::Skip(msg) => assert!(msg.contains("structured-output")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn fail_structured_output_no_schema() {
        let help = HelpOutput::from_raw(HELP_STRUCTURED_NO_SCHEMA);
        match check_schema_print(&help) {
            CheckStatus::Fail(msg) => assert!(msg.contains("schema")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
