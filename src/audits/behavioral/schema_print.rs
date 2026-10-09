//! Audit: `p2-must-schema-print`.
//!
//! When a CLI emits structured output, it MUST expose its output schema via a
//! `schema` subcommand or `--schema` flag. Runtime-discoverable schemas let
//! agents pin against shape changes across versions; without one, every
//! consumer infers the shape from sample output and breaks on every change.
//!
//! Applicability: gates on a help-text probe — only fires when the help
//! mentions any structured-output indicator (`--output`, `--format`, `--json`,
//! `--jsonl`, or the words "json"/"jsonl"), or when the `.anc.toml` chain's
//! `[p2] json_probe` prints JSON. When neither shows structured output the
//! audit Skips with evidence; otherwise it looks for either a `schema`
//! subcommand or `--schema` flag, then for the subcommand the chain names
//! in `[p2] schema_command`.

use crate::anc_toml::{JSON_PROBE_KEY, SCHEMA_COMMAND_KEY, Sourced};
use crate::audit::Audit;
use crate::audits::behavioral::declared_probe::{probe_invocation, run_declared_probe};
use crate::audits::behavioral::subcommand_help::{probe_help, probe_subcommands};
use crate::project::Project;
use crate::runner::{BinaryRunner, HelpOutput};
use crate::types::{
    AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, Mitigation, Verdict,
};

const STRUCTURED_OUTPUT_FLAG_NAMES: &[&str] =
    &["--output", "--format", "--json", "--jsonl", "--ndjson"];

const STRUCTURED_OUTPUT_TOKENS: &[&str] = &["json", "jsonl", "ndjson", "JSON Lines"];

pub struct SchemaPrintAudit;

impl Audit for SchemaPrintAudit {
    fn id(&self) -> &str {
        "p2-schema-print"
    }

    fn label(&self) -> &'static str {
        "Structured-output CLI exposes its schema at runtime"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P2
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-must-schema-print"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let verdict = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()).into(),
            Some(help) => {
                let runner = project.runner_ref();
                let p2 = project.anc_config.config().map(|cfg| &cfg.p2);
                let shown_by_probe = p2
                    .and_then(|p2| p2.json_probe.as_ref())
                    .filter(|_| !has_structured_output_indicator(help))
                    .filter(|probe| run_declared_probe(runner, &probe.value).is_ok());
                // First try the top-level help only. If that's inconclusive,
                // walk one level into each top-level subcommand to find
                // `schema` exposed as a nested subcommand (e.g.,
                // `anc emit schema`). One-level walk matches how an agent
                // would discover the surface via `--help` chaining.
                let status = match audit_schema_print(help, shown_by_probe.is_some()) {
                    AuditStatus::Fail(_) => {
                        let subhelp = probe_subcommands(runner, help);
                        audit_schema_print_with_subhelp(help, shown_by_probe.is_some(), &subhelp)
                    }
                    other => other,
                };
                let verdict = match (status, p2.and_then(|p2| p2.schema_command.as_ref())) {
                    (status @ AuditStatus::Fail(_), Some(path)) => {
                        audit_declared_schema_command(status, runner, help, path)
                    }
                    (status, _) => Verdict::from(status),
                };
                match shown_by_probe {
                    Some(probe) => credit_the_probe(verdict, runner, probe),
                    None => verdict,
                }
            }
        };
        let status = match project.anc_config.void_note() {
            Some(note) => verdict.status.with_note(&note),
            None => verdict.status,
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: verdict.mitigation,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

/// Pass when the help lists the declared schema command, naming it and the
/// file that declared it; otherwise keep the built-in `fail` and say the
/// declared command was not found.
fn audit_declared_schema_command(
    fail: AuditStatus,
    runner: &BinaryRunner,
    help: &HelpOutput,
    path: &Sourced<Vec<String>>,
) -> Verdict {
    let shown = probe_invocation(runner, &path.value);
    let cited = path.cite(SCHEMA_COMMAND_KEY);
    if lists_command_path(runner, help, &path.value) {
        return Verdict {
            status: AuditStatus::Pass,
            mitigation: Some(Mitigation::Config(format!(
                "`{shown}` is the schema command declared via {cited}"
            ))),
        };
    }
    fail.with_note(&format!(
        "The schema command `{shown}`, declared via {cited}, is not listed in --help."
    ))
    .into()
}

/// Whether each token of `path` is listed in its parent's `--help`: the
/// first in `top_help`, each later one in `<bin> <path so far> --help`.
fn lists_command_path(runner: &BinaryRunner, top_help: &HelpOutput, path: &[String]) -> bool {
    let Some(first) = path.first() else {
        return false;
    };
    lists_subcommand(top_help, first)
        && (1..path.len()).all(|depth| {
            let parent: Vec<&str> = path[..depth].iter().map(String::as_str).collect();
            probe_help(runner, &parent).is_some_and(|help| lists_subcommand(&help, &path[depth]))
        })
}

/// Whether `help` lists `name` as a subcommand: a parsed name, or an
/// indented line whose text before the description gap is `name`, for a
/// command block whose heading the parser does not read (kubectl's `Basic
/// Commands (Beginner):`).
fn lists_subcommand(help: &HelpOutput, name: &str) -> bool {
    help.subcommands()
        .iter()
        .any(|parsed| parsed.to_lowercase() == name)
        || help.raw().lines().any(|line| {
            line.starts_with(char::is_whitespace) && line.trim().split("  ").next() == Some(name)
        })
}

/// Name the declared probe that showed structured output when the help did
/// not: in a Pass's evidence beside any other setting it needed, or at the
/// end of any other status.
fn credit_the_probe(
    verdict: Verdict,
    runner: &BinaryRunner,
    probe: &Sourced<Vec<String>>,
) -> Verdict {
    let shown = format!(
        "structured output shown by `{}`, the probe declared via {}",
        probe_invocation(runner, &probe.value),
        probe.cite(JSON_PROBE_KEY)
    );
    let note = format!("The CLI has {shown}.");
    verdict.crediting(shown, &note)
}

/// Whether the help names a structured-output flag or format.
fn has_structured_output_indicator(help: &HelpOutput) -> bool {
    let raw_lower = help.raw().to_lowercase();
    let has_structured_flag = help.find_flag(STRUCTURED_OUTPUT_FLAG_NAMES).is_some();
    has_structured_flag
        || STRUCTURED_OUTPUT_TOKENS
            .iter()
            .any(|t| raw_lower.contains(&t.to_lowercase()))
}

/// Core unit for tests. Returns Skip when neither the help nor a declared
/// probe (`json_shown`) shows structured output (vacuous applicability),
/// Pass when a schema surface is advertised, Fail when structured output is
/// shown without a schema surface.
pub(crate) fn audit_schema_print(help: &HelpOutput, json_shown: bool) -> AuditStatus {
    let raw = help.raw();
    if !json_shown && !has_structured_output_indicator(help) {
        return AuditStatus::Skip(
            "no structured-output indicator (--output / --format / json / jsonl) in --help".into(),
        );
    }

    let has_schema_flag = help.find_flag(&["--schema"]).is_some();
    if has_schema_flag {
        return AuditStatus::Pass;
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
        return AuditStatus::Pass;
    }

    AuditStatus::Fail(help.noting_dash_rule(
        &["--schema"],
        "CLI emits structured output but exposes no `schema` subcommand or \
         `--schema` flag. Agents need a runtime-discoverable schema to pin \
         against shape changes.",
    ))
}

/// Extended audit that also walks one level into each top-level subcommand
/// to find `schema` exposed as a nested verb (e.g., `anc emit schema`,
/// `anc emit schema`). Mirrors how an agent discovers the surface by
/// chaining `--help` calls — depth-1 walks are the realistic discovery
/// bound for an agent that does not have prior knowledge of the CLI.
pub(crate) fn audit_schema_print_with_subhelp(
    top_help: &HelpOutput,
    json_shown: bool,
    subhelp: &[(String, HelpOutput)],
) -> AuditStatus {
    // Re-run the top-level audit first so the applicability gate and
    // top-level positives short-circuit before we inspect nested help.
    match audit_schema_print(top_help, json_shown) {
        AuditStatus::Fail(_) => {}
        other => return other,
    }

    for (_name, help) in subhelp {
        let has_schema_flag = help.find_flag(&["--schema"]).is_some();
        if has_schema_flag {
            return AuditStatus::Pass;
        }
        let schema_in_subs = help
            .subcommands()
            .iter()
            .any(|s| s.eq_ignore_ascii_case("schema"));
        if schema_in_subs {
            return AuditStatus::Pass;
        }
        let schema_section_match = help
            .raw()
            .lines()
            .any(|line| line.starts_with("  ") && line.trim_start().starts_with("schema"));
        if schema_section_match {
            return AuditStatus::Pass;
        }
    }

    AuditStatus::Fail(
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
  audit    Run audits
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

    /// Help with no structured-output indicator, an `--output` flag on
    /// `get`, and a read-only call that prints JSON.
    const JSON_ONLY_ON_A_SUBCOMMAND: &str = r#"case "$*" in
  "version -o json") echo '{"version":"1.0"}' ;;
  "get --help") printf 'Usage: test get\n\nOptions:\n  -o, --output <FORMAT>  One of: text, yaml\n' ;;
  *) printf 'Usage: test <COMMAND>\n\nCommands:\n  get      Show a resource\n  version  Print the version\n' ;;
esac"#;

    fn sourced(args: &[&str]) -> Sourced<Vec<String>> {
        Sourced {
            value: args.iter().map(|arg| (*arg).to_string()).collect(),
            file: ".anc.toml".into(),
        }
    }

    /// Run the audit on `script` with the given `[p2]` declarations.
    fn run_declared(
        script: &str,
        json_probe: Option<&[&str]>,
        schema: Option<&[&str]>,
    ) -> AuditResult {
        let mut project = crate::audits::behavioral::tests::test_project_with_sh_script(script);
        let mut cfg = crate::anc_toml::AncConfig::default();
        cfg.p2.json_probe = json_probe.map(sourced);
        cfg.p2.schema_command = schema.map(sourced);
        project.anc_config.load = crate::anc_toml::AncConfigLoad::Loaded(cfg);
        SchemaPrintAudit.run(&project).expect("audit runs")
    }

    /// Structured output in the help, no `schema` surface, and an `explain`
    /// command plus an `emit shape` command.
    const EXPLAIN_CLI: &str = r#"case "$*" in
  "emit --help") printf 'Usage: test emit <COMMAND>\n\nCommands:\n  shape  Print the output shape\n' ;;
  *--help*) printf 'Usage: test <COMMAND>\n\nCommands:\n  get      Show a resource\n  explain  Describe a resource type\n  emit     Emit artifacts\n\nOptions:\n  --output <FORMAT>  text or json\n' ;;
esac"#;

    #[test]
    fn a_declared_schema_command_the_help_lists_passes_and_names_its_file() {
        assert!(matches!(
            run_declared(EXPLAIN_CLI, None, None).status,
            AuditStatus::Fail(_)
        ));

        let result = run_declared(EXPLAIN_CLI, None, Some(&["explain"]));

        assert_eq!(result.status, AuditStatus::Pass);
        assert_eq!(
            result.mitigation,
            Some(Mitigation::Config(
                "`test explain` is the schema command declared via .anc.toml [p2].schema_command"
                    .into()
            ))
        );
    }

    #[test]
    fn a_nested_declared_schema_command_is_found_in_its_parent_help() {
        let result = run_declared(EXPLAIN_CLI, None, Some(&["emit", "shape"]));

        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn a_declared_schema_command_the_help_does_not_list_keeps_the_fail_and_says_so() {
        match run_declared(EXPLAIN_CLI, None, Some(&["emit", "schema"])).status {
            AuditStatus::Fail(msg) => assert!(
                msg.ends_with(
                    "The schema command `test emit schema`, declared via .anc.toml \
                     [p2].schema_command, is not listed in --help."
                ),
                "{msg}"
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn a_kubectl_shaped_cli_passes_on_its_declared_probe_and_schema_command() {
        let script = r#"case "$*" in
  "version --client -o json") echo '{"clientVersion":{}}' ;;
  *--help*) printf 'tool controls things.\n\nBasic Commands (Intermediate):\n  explain         Get documentation for a resource\n  get             Display one or many resources\n' ;;
esac"#;

        let result = run_declared(
            script,
            Some(&["version", "--client", "-o", "json"]),
            Some(&["explain"]),
        );

        assert_eq!(result.status, AuditStatus::Pass);
        assert_eq!(
            result.mitigation,
            Some(Mitigation::Config(
                "`test explain` is the schema command declared via .anc.toml [p2].schema_command; \
                 structured output shown by `test version --client -o json`, the probe declared \
                 via .anc.toml [p2].json_probe"
                    .into()
            ))
        );
    }

    #[test]
    fn a_declared_probe_that_prints_json_shows_structured_output() {
        assert!(matches!(
            run_declared(JSON_ONLY_ON_A_SUBCOMMAND, None, None).status,
            AuditStatus::Skip(_)
        ));

        match run_declared(
            JSON_ONLY_ON_A_SUBCOMMAND,
            Some(&["version", "-o", "json"]),
            None,
        )
        .status
        {
            AuditStatus::Fail(msg) => assert!(
                msg.ends_with(
                    "The CLI has structured output shown by `test version -o json`, the probe \
                     declared via .anc.toml [p2].json_probe."
                ),
                "{msg}"
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn happy_path_schema_subcommand() {
        let help = HelpOutput::from_raw(HELP_WITH_SCHEMA_SUBCMD);
        assert_eq!(audit_schema_print(&help, false), AuditStatus::Pass);
    }

    #[test]
    fn happy_path_schema_flag() {
        let help = HelpOutput::from_raw(HELP_WITH_SCHEMA_FLAG);
        assert_eq!(audit_schema_print(&help, false), AuditStatus::Pass);
    }

    #[test]
    fn skip_no_structured_output_indicator() {
        let help = HelpOutput::from_raw(HELP_NO_STRUCTURED_OUTPUT);
        match audit_schema_print(&help, false) {
            AuditStatus::Skip(msg) => assert!(msg.contains("structured-output")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn fail_structured_output_no_schema() {
        let help = HelpOutput::from_raw(HELP_STRUCTURED_NO_SCHEMA);
        match audit_schema_print(&help, false) {
            AuditStatus::Fail(msg) => assert!(msg.contains("schema")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn a_single_dash_schema_beside_double_dash_names_fails_and_says_why() {
        let help = HelpOutput::from_raw(
            "Options:\n      --output <FORMAT>    Output format: text or json.\n  -schema                  Print the output schema.\n",
        );
        match audit_schema_print(&help, false) {
            AuditStatus::Fail(msg) => assert!(
                msg.ends_with("`-schema` is declared, but this help also declares double-dash names, so it does not count as `--schema`."),
                "{msg}"
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
