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
//! subcommand or `--schema` flag.

use crate::anc_toml::{JSON_PROBE_KEY, Sourced};
use crate::audit::Audit;
use crate::audits::behavioral::json_output::{probe_invocation, run_declared_probe};
use crate::audits::behavioral::subcommand_help::probe_subcommands;
use crate::project::Project;
use crate::runner::{BinaryRunner, HelpOutput};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, Verdict};

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
                let declared = project
                    .anc_config
                    .config()
                    .and_then(|cfg| cfg.p2.json_probe.as_ref());
                let shown_by_probe = declared
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
                match shown_by_probe {
                    Some(probe) => credit_the_probe(status, runner, probe),
                    None => status.into(),
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

/// Name the declared probe that showed structured output when the help did
/// not: in a Pass's evidence, or at the end of any other status.
fn credit_the_probe(
    status: AuditStatus,
    runner: &BinaryRunner,
    probe: &Sourced<Vec<String>>,
) -> Verdict {
    let shown = format!(
        "structured output shown by `{}`, the probe declared via {}",
        probe_invocation(runner, &probe.value),
        probe.cite(JSON_PROBE_KEY)
    );
    let note = format!("The CLI has {shown}.");
    Verdict::from(status).crediting(shown, &note)
}

/// Whether the help names a structured-output flag or format.
fn has_structured_output_indicator(help: &HelpOutput) -> bool {
    let raw_lower = help.raw().to_lowercase();
    let has_structured_flag = help
        .flags()
        .iter()
        .any(|f| STRUCTURED_OUTPUT_FLAG_NAMES.iter().any(|n| f.matches(n)));
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

    let has_schema_flag = help.flags().iter().any(|f| f.matches("--schema"));
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

    AuditStatus::Fail(
        "CLI emits structured output but exposes no `schema` subcommand or \
         `--schema` flag. Agents need a runtime-discoverable schema to pin \
         against shape changes."
            .into(),
    )
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
        let has_schema_flag = help.flags().iter().any(|f| f.matches("--schema"));
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

    fn run_with_probe(probe: Option<&[&str]>) -> AuditResult {
        let mut project = crate::audits::behavioral::tests::test_project_with_sh_script(
            JSON_ONLY_ON_A_SUBCOMMAND,
        );
        let mut cfg = crate::anc_toml::AncConfig::default();
        cfg.p2.json_probe = probe.map(|args| Sourced {
            value: args.iter().map(|arg| (*arg).to_string()).collect(),
            file: ".anc.toml".into(),
        });
        project.anc_config.load = crate::anc_toml::AncConfigLoad::Loaded(cfg);
        SchemaPrintAudit.run(&project).expect("audit runs")
    }

    #[test]
    fn a_declared_probe_that_prints_json_shows_structured_output() {
        assert!(matches!(run_with_probe(None).status, AuditStatus::Skip(_)));

        match run_with_probe(Some(&["version", "-o", "json"])).status {
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
}
