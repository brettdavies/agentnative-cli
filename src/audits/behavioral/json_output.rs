use crate::anc_toml::JSON_PROBE_KEY;
use crate::audit::Audit;
use crate::audits::behavioral::declared_probe::audit_declared_probe;
use crate::audits::behavioral::output_flags::{self, Declared};
use crate::audits::behavioral::subcommand_help::{dash_rule_notes, should_skip};
use crate::project::Project;
use crate::runner::{BinaryRunner, HelpOutput, RunStatus};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, Verdict};

/// What the help and the safe probes established.
enum Detected {
    /// The row's status, with nothing left to check.
    Settled(AuditStatus),
    /// An output flag is declared and no safe probe printed JSON, with the
    /// evidence that says so. A `[p2] json_probe` declaration takes over
    /// from here.
    Unverified(String),
}

pub struct JsonOutputAudit;

impl Audit for JsonOutputAudit {
    fn id(&self) -> &str {
        "p2-json-output"
    }

    fn label(&self) -> &'static str {
        "Structured output support"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P2
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-must-output-flag"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let runner = project.runner_ref();
        let declared = project
            .anc_config
            .config()
            .and_then(|cfg| cfg.p2.json_probe.as_ref());
        let verdict = match (detect_json_output(runner, project), declared) {
            (Detected::Unverified(_), Some(probe)) => audit_declared_probe(runner, probe),
            (Detected::Unverified(evidence), None) => AuditStatus::Skip(evidence).into(),
            (Detected::Settled(status @ AuditStatus::OptOut(_)), Some(probe)) => status
                .with_note(&format!(
                    "The probe declared via {} runs only for a tool whose help declares {}.",
                    probe.cite(JSON_PROBE_KEY),
                    output_flags::accepted_prose(),
                ))
                .into(),
            (Detected::Settled(status), _) => Verdict::from(status),
        };
        let status = match project.anc_config.void_note() {
            Some(note) => verdict.status.with_note(&note),
            None => verdict.status,
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: AuditGroup::P2,
            layer: AuditLayer::Behavioral,
            status,
            confidence: Confidence::High,
            mitigation: verdict.mitigation,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

/// Find an output flag in the help and validate JSON through the safe
/// probes, without any declaration.
fn detect_json_output(runner: &BinaryRunner, project: &Project) -> Detected {
    let help = match (runner.run(&["--help"], &[]).status, project.help_output()) {
        (RunStatus::Ok, Some(help)) => help,
        _ => {
            return Detected::Settled(AuditStatus::Skip(
                "could not run --help to detect output flags".into(),
            ));
        }
    };
    let declared = output_flags::declared(help);
    if declared.is_empty() {
        // Most CLIs (gh, kubectl, cargo) put --output on subcommands, not
        // top-level.
        probe_subcommands(runner, help)
    } else {
        validate_json_output(runner, &[], &declared)
    }
}

/// Probe each top-level subcommand from the shared help parse for an output flag.
///
/// Most CLI frameworks (clap, cobra, argparse) list subcommands under a "Commands:"
/// or "Subcommands:" section, and hand-written help under `... commands:`; the
/// shared parser reads all of them.
fn probe_subcommands(runner: &BinaryRunner, help: &HelpOutput) -> Detected {
    let accepted = output_flags::accepted();
    let subcommands = subcommands_to_probe(help);
    if subcommands.is_empty() {
        return Detected::Settled(AuditStatus::OptOut(help.noting_dash_rule(
            &accepted,
            &format!(
                "no option definition in --help declares {}, the flags this requirement \
                 accepts; usage lines are not read. The row is opt_out, and the \
                 schema-discovery requirements (p2-must-schema-print, p2-should-schema-file) \
                 collapse to n/a via antecedent propagation.",
                output_flags::accepted_prose(),
            ),
        )));
    }

    let mut read: Vec<(String, HelpOutput)> = Vec::new();
    for subcmd in &subcommands {
        let sub_help = runner.run(&[subcmd, "--help"], &[]);
        if sub_help.status != RunStatus::Ok {
            continue;
        }
        let sub_help = HelpOutput::from_raw(format!("{}{}", sub_help.stdout, sub_help.stderr));
        let declared = output_flags::declared(&sub_help);
        if !declared.is_empty() {
            return validate_json_output(runner, &[subcmd], &declared);
        }
        read.push((subcmd.to_string(), sub_help));
    }

    let message = help.noting_dash_rule(
        &accepted,
        &format!(
            "no option definition in --help, or in the --help of the {} subcommand{} read, \
             declares {}, the flags this requirement accepts; usage lines are not read.",
            read.len(),
            if read.len() == 1 { "" } else { "s" },
            output_flags::accepted_prose(),
        ),
    );
    let names: Vec<&str> = read.iter().map(|(name, _)| name.as_str()).collect();
    Detected::Settled(AuditStatus::OptOut(format!(
        "{message}{}",
        dash_rule_notes(&names, &read, &accepted, None)
    )))
}

/// Top-level subcommand names worth probing for an output flag: the shared
/// parser's names minus the built-ins (`help`, shell completions) that the
/// subcommand-help helper skips. `help` in particular echoes top-level help,
/// so probing it could move this row on a tool that has no output flag.
fn subcommands_to_probe(help: &HelpOutput) -> Vec<&str> {
    help.subcommands()
        .iter()
        .map(String::as_str)
        .filter(|name| !should_skip(name))
        .collect()
}

/// Try safe subcommands with the declared flags to validate actual JSON output.
///
/// `prefix` contains any subcommand path (e.g., ["audit"]) to prepend to the
/// probe commands. For top-level flags, prefix is empty. Each declared flag
/// is passed as the help spells it.
///
/// Strategy: try `[prefix...] --help --flag json` first (safe, --help always exits
/// without side effects). If that produces non-JSON (many CLIs ignore --output with
/// --help), fall back to `[prefix...] --version --flag json`. A JSON alias goes
/// alone in place of `--flag json`. Never run the binary bare with just
/// `--flag json`, as that could execute destructive commands.
fn validate_json_output(runner: &BinaryRunner, prefix: &[&str], declared: &Declared) -> Detected {
    // Safe suffixes: always probe with --help or --version, never bare invocation.
    // Bare subcommand probing (`&[]`) was removed because it is unsafe in the
    // general case — subcommands may have side effects (kubectl apply, docker rm,
    // terraform plan), and for agentnative itself it caused fork bombs.
    const SAFE_SUFFIXES: &[&str] = &["--help", "--version"];

    for value in declared.json_selectors() {
        for suffix in SAFE_SUFFIXES {
            let args: Vec<&str> = prefix
                .iter()
                .copied()
                .chain([*suffix])
                .chain(value.iter().map(String::as_str))
                .collect();
            if let Some(status) = try_json_probe(runner, &args) {
                return Detected::Settled(status);
            }
        }
    }

    // The safe probes reach only `--help` and `--version`, which most CLIs
    // answer in text whatever the output flag says, so a miss is anc's
    // limit, not the tool's: the row is not scored.
    let names: Vec<String> = declared.names().map(|flag| format!("`{flag}`")).collect();
    let place = match prefix {
        [] => "--help".to_string(),
        path => format!("`{} --help`", path.join(" ")),
    };
    Detected::Unverified(format!(
        "{} {} declared in {place}, but no safe probe printed JSON (--help and --version \
         override output flags in most CLIs)",
        names.join(" and "),
        if names.len() == 1 { "is" } else { "are" },
    ))
}

/// Run a single JSON probe and return Some(status) if valid JSON found.
fn try_json_probe(runner: &BinaryRunner, args: &[&str]) -> Option<AuditStatus> {
    let result = runner.run(args, &[]);

    match result.status {
        RunStatus::Ok => {
            let stdout = result.stdout.trim();
            if !stdout.is_empty() && serde_json::from_str::<serde_json::Value>(stdout).is_ok() {
                return Some(AuditStatus::Pass);
            }

            let stderr = result.stderr.trim();
            if !stderr.is_empty() && serde_json::from_str::<serde_json::Value>(stderr).is_ok() {
                if result.exit_code != Some(0) {
                    return Some(AuditStatus::Warn(
                        "binary exits non-zero but produces valid JSON on stderr".into(),
                    ));
                }
                return Some(AuditStatus::Pass);
            }

            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anc_toml::Sourced;
    use crate::audits::behavioral::tests::test_project_with_sh_script;
    use crate::types::Mitigation;

    /// Run the audit on `script` with `probe` declared in `.anc.toml`.
    fn run_with_probe(script: &str, probe: &[&str]) -> AuditResult {
        let mut project = test_project_with_sh_script(script);
        let mut cfg = crate::anc_toml::AncConfig::default();
        cfg.p2.json_probe = Some(Sourced {
            value: probe.iter().map(|arg| (*arg).to_string()).collect(),
            file: ".anc.toml".into(),
        });
        project.anc_config.load = crate::anc_toml::AncConfigLoad::Loaded(cfg);
        JsonOutputAudit.run(&project).expect("audit should run")
    }

    /// An `--output` flag the safe probes cannot validate, and read-only
    /// calls that print JSON, print text, or print JSON and then fail.
    const OUTPUT_FLAG_WITH_PROBES: &str = r#"
case "$*" in
  "version -o json") echo '{"version":"1.0"}';;
  "version --text") echo "version 1.0";;
  "status -o json") echo '{"status":"down"}'; exit 1;;
  *--help*)
    printf 'Usage: test [OPTIONS]\n\nOptions:\n  --output <FORMAT>  Output format: text or json\n';;
  *)
    echo "this is not json";;
esac
"#;

    #[test]
    fn a_declared_probe_validates_what_the_safe_probes_cannot() {
        let without = JsonOutputAudit
            .run(&test_project_with_sh_script(OUTPUT_FLAG_WITH_PROBES))
            .expect("audit should run");
        assert!(
            matches!(without.status, AuditStatus::Skip(_)),
            "{:?}",
            without.status
        );

        let with = run_with_probe(OUTPUT_FLAG_WITH_PROBES, &["version", "-o", "json"]);

        assert_eq!(with.status, AuditStatus::Pass);
        assert_eq!(
            with.mitigation,
            Some(Mitigation::Config(
                "`test version -o json` printed JSON; probe declared via .anc.toml [p2].json_probe"
                    .into()
            ))
        );
    }

    #[test]
    fn a_declared_probe_that_prints_text_fails_and_says_so() {
        match run_with_probe(OUTPUT_FLAG_WITH_PROBES, &["version", "--text"]).status {
            AuditStatus::Fail(msg) => assert_eq!(
                msg,
                "`test version --text`, the probe declared via .anc.toml [p2].json_probe, printed \
                 no JSON on stdout. The declared probe must exit 0 and print JSON on stdout."
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn a_declared_probe_that_exits_nonzero_fails_even_with_json_on_stdout() {
        match run_with_probe(OUTPUT_FLAG_WITH_PROBES, &["status", "-o", "json"]).status {
            AuditStatus::Fail(msg) => assert!(msg.contains(", exited 1."), "{msg}"),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn a_declared_probe_does_not_stand_in_for_a_missing_output_flag() {
        let script = r#"
case "$*" in
  "version -o json") echo '{"version":"1.0"}';;
  *) echo 'just some help text';;
esac
"#;
        match run_with_probe(script, &["version", "-o", "json"]).status {
            AuditStatus::OptOut(msg) => assert!(
                msg.ends_with(
                    "The probe declared via .anc.toml [p2].json_probe runs only for a tool whose \
                     help declares --output, --format, --output-format, --json or --jsonl."
                ),
                "{msg}"
            ),
            other => panic!("expected OptOut, got {other:?}"),
        }
    }

    #[test]
    fn a_single_dash_format_flag_is_probed_as_the_help_spells_it() {
        // Prints JSON only for the spelling its help declares.
        let script = r#"
case "$*" in
  "--help -format json") echo '{"format":"json"}';;
  *--format*) echo "flag provided but not defined: -format" >&2; exit 2;;
  *--help*)
    printf 'Usage of tool:\n  -format string\n    \tOutput format: text or json (default "text")\n  -version\n    \tPrint version\n';;
  *) echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass, "got {:?}", result.status);
    }

    #[test]
    fn a_longer_flag_that_starts_with_output_does_not_trigger_the_probe() {
        // Would print JSON for `--output json`, which no line of its help
        // declares.
        let script = r#"
case "$*" in
  *--output\ json*|*--output=json*) echo '{"probed":true}';;
  *--help*)
    printf 'Usage: tool [OPTIONS]\n\nOptions:\n      --output-dir <DIR>    Where to write\n  -h, --help                Show help\n';;
  *) echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputAudit.run(&project).expect("audit should run");
        assert_eq!(
            result.status,
            AuditStatus::OptOut(
                "no option definition in --help declares --output, --format, --output-format, \
                 --json or --jsonl, the flags this requirement accepts; usage lines are not \
                 read. The row is opt_out, and the schema-discovery requirements \
                 (p2-must-schema-print, p2-should-schema-file) collapse to n/a via antecedent \
                 propagation."
                    .into()
            )
        );
    }

    #[test]
    fn output_format_is_probed_as_an_output_flag() {
        let script = r#"
case "$*" in
  "--help --output-format json") echo '{"format":"json"}';;
  *--help*)
    printf 'Usage: tool [OPTIONS]\n\nOptions:\n  -o, --output-format <FORMAT>    Output format: text or json\n  -h, --help                      Show help\n';;
  *) echo "hello";;
esac
"#;
        let result = JsonOutputAudit
            .run(&test_project_with_sh_script(script))
            .expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass, "got {:?}", result.status);
    }

    #[test]
    fn a_boolean_json_alias_is_probed_alone() {
        let script = r#"
case "$*" in
  "--help --json") echo '{"help":true}';;
  *--help*)
    printf 'Usage: tool [OPTIONS]\n\nOptions:\n      --json    Print JSON\n  -h, --help    Show help\n';;
  *) echo "hello";;
esac
"#;
        let result = JsonOutputAudit
            .run(&test_project_with_sh_script(script))
            .expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass, "got {:?}", result.status);
    }

    #[test]
    fn a_json_flag_that_takes_a_value_is_not_the_alias() {
        let script = r#"
case "$*" in
  *--json*--help*|*--help*--json*) echo '{"probed":true}';;
  *--help*)
    printf 'Usage: tool [OPTIONS]\n\nOptions:\n      --json fields    Output JSON with the specified fields\n  -h, --help           Show help\n';;
  *) echo "hello";;
esac
"#;
        let result = JsonOutputAudit
            .run(&test_project_with_sh_script(script))
            .expect("audit should run");
        assert!(
            matches!(result.status, AuditStatus::OptOut(_)),
            "got {:?}",
            result.status
        );
    }

    #[test]
    fn an_unverified_flag_is_named_as_declared_with_the_help_that_declares_it() {
        let top = JsonOutputAudit
            .run(&test_project_with_sh_script(UNVERIFIABLE_OUTPUT_FLAG))
            .expect("audit should run");
        assert_eq!(
            top.status,
            AuditStatus::Skip(
                "`--output` is declared in --help, but no safe probe printed JSON (--help and \
                 --version override output flags in most CLIs)"
                    .into()
            )
        );

        let script = r#"
case "$*" in
  *export*--help*)
    printf 'Usage: tool export [OPTIONS]\n\nOptions:\n      --output <FILE>      Write to FILE\n      --format <FORMAT>    Output format\n';;
  *--help*)
    printf 'Usage: tool [COMMAND]\n\nCommands:\n  list      List items\n  export    Export items\n\nOptions:\n  -h, --help    Show help\n';;
  *) echo "hello";;
esac
"#;
        let sub = JsonOutputAudit
            .run(&test_project_with_sh_script(script))
            .expect("audit should run");
        assert_eq!(
            sub.status,
            AuditStatus::Skip(
                "`--output` and `--format` are declared in `export --help`, but no safe probe \
                 printed JSON (--help and --version override output flags in most CLIs)"
                    .into()
            )
        );
    }

    #[test]
    fn an_opt_out_after_reading_subcommands_says_how_many_and_notes_a_near_miss() {
        let script = r#"
case "$*" in
  *list*--help*)
    printf 'Usage: tool list [OPTIONS]\n\nOptions:\n  -format <FORMAT>    Output format\n      --help          Show help\n';;
  *sync*--help*)
    printf 'Usage: tool sync [OPTIONS]\n\nOptions:\n      --help    Show help\n';;
  *--help*)
    printf 'Usage: tool [COMMAND]\n\nCommands:\n  list    List items\n  sync    Sync items\n\nOptions:\n  -h, --help    Show help\n';;
  *) echo "hello";;
esac
"#;
        let result = JsonOutputAudit
            .run(&test_project_with_sh_script(script))
            .expect("audit should run");
        assert_eq!(
            result.status,
            AuditStatus::OptOut(
                "no option definition in --help, or in the --help of the 2 subcommands read, \
                 declares --output, --format, --output-format, --json or --jsonl, the flags this \
                 requirement accepts; usage lines are not read. In `list`, `-format` is \
                 declared, but this help also declares double-dash names, so it does not count \
                 as `--format`."
                    .into()
            )
        );
    }

    #[test]
    fn no_probe_is_sent_without_a_help_or_version_suffix() {
        // Prints JSON for any call that carries the output flag and neither
        // safe suffix, at the top level and under the subcommand. Only such
        // a call can make this row pass.
        let script = r#"
case "$*" in
  *--help*|*--version*)
    case "$*" in
      *export*) printf 'Usage: tool export [OPTIONS]\n\nOptions:\n      --format <FORMAT>    Output format\n';;
      *) printf 'Usage: tool [COMMAND]\n\nCommands:\n  export    Export items\n\nOptions:\n  -h, --help    Show help\n';;
    esac;;
  *--format*) echo '{"unsafe":"ran without a safe suffix"}';;
  *) echo "hello";;
esac
"#;
        let result = JsonOutputAudit
            .run(&test_project_with_sh_script(script))
            .expect("audit should run");
        assert!(
            matches!(result.status, AuditStatus::Skip(_)),
            "got {:?}",
            result.status
        );
    }

    #[test]
    fn json_output_pass_with_valid_json() {
        let script = r#"
case "$*" in
  *--help*--output*json*|*--output*json*--help*)
    echo '{"help":true,"format":"json"}';;
  *--help*)
    printf 'Usage: test [OPTIONS]\n\nOptions:\n      --output <FORMAT>    Output format\n';;
  *--output\ json*|*--output=json*)
    echo '{"version":"1.0"}';;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass, "got {:?}", result.status);
    }

    #[test]
    fn json_output_pass_with_format_flag() {
        let script = r#"
case "$*" in
  *--help*--format*json*|*--format*json*--help*)
    echo '{"help":true}';;
  *--help*)
    printf 'Usage: test [OPTIONS]\n\nOptions:\n      --format <FORMAT>    Output format\n';;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass, "got {:?}", result.status);
    }

    /// Advertises `--output` but answers every safe probe in text.
    const UNVERIFIABLE_OUTPUT_FLAG: &str = r#"
case "$*" in
  *--help*)
    printf 'Usage: test [OPTIONS]\n\nOptions:\n  --output <FORMAT>  Output format: text or json\n';;
  *--output*)
    echo "this is not json";;
  *)
    echo "hello";;
esac
"#;

    #[test]
    fn json_output_skips_when_no_safe_probe_can_validate() {
        let project = test_project_with_sh_script(UNVERIFIABLE_OUTPUT_FLAG);
        let result = JsonOutputAudit.run(&project).expect("audit should run");
        match &result.status {
            AuditStatus::Skip(msg) => {
                assert!(msg.contains("no safe probe printed JSON"), "got: {msg}")
            }
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn an_unverifiable_output_flag_leaves_the_schema_rows_unmeasured() {
        use crate::audit::Audit;
        use crate::audits::behavioral::schema_print::SchemaPrintAudit;

        let project = test_project_with_sh_script(UNVERIFIABLE_OUTPUT_FLAG);
        let catalog: Vec<Box<dyn Audit>> =
            vec![Box::new(JsonOutputAudit), Box::new(SchemaPrintAudit)];
        let raw: Vec<AuditResult> = catalog
            .iter()
            .map(|audit| audit.run(&project).expect("audit should run"))
            .collect();

        let rows = crate::scorecard::build_row_results(&raw, &catalog);
        let schema_print = rows
            .iter()
            .find(|(row, _)| row.id == "p2-must-schema-print")
            .map(|(row, _)| &row.status)
            .expect("schema-print row");

        match schema_print {
            AuditStatus::Skip(msg) => assert!(
                msg.starts_with("antecedent `p2-json-output` could not be measured:"),
                "got: {msg}"
            ),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn json_output_opt_out_no_flag() {
        // Schema 0.6: "no flag at all" is opt_out (deliberate non-adoption),
        // not Skip (probe limitation). Distinguishes "tool doesn't ship
        // structured output" from "we couldn't measure it".
        let project = test_project_with_sh_script("echo 'just some help text'");
        let result = JsonOutputAudit.run(&project).expect("audit should run");
        match &result.status {
            AuditStatus::OptOut(msg) => assert!(
                msg.starts_with("no option definition in --help declares --output, --format,"),
                "{msg}"
            ),
            other => panic!("expected OptOut, got {other:?}"),
        }
    }

    #[test]
    fn json_output_fallback_to_version() {
        let script = r#"
case "$*" in
  *--version*--output*json*|*--output*json*--version*|*--version*--output=json*|*--output=json*--version*)
    echo '{"version":"2.0"}';;
  *--help*)
    printf 'Usage: test [OPTIONS]\n\nOptions:\n      --output <FORMAT>    Output format\n';;
  *--version*)
    echo "test 2.0";;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass, "got {:?}", result.status);
    }

    #[test]
    fn json_output_handles_crash() {
        let project = test_project_with_sh_script("kill -11 $$");
        let result = JsonOutputAudit
            .run(&project)
            .expect("audit should not panic on crash");
        assert!(matches!(result.status, AuditStatus::Skip(_)));
    }

    #[test]
    fn json_output_probes_subcommands() {
        // Simulates a CLI with subcommands where --output is on the subcommand.
        // More specific patterns must come before *--help* catch-all.
        let script = r#"
case "$*" in
  *audit*--output*json*|*audit*--output=json*)
    echo '{"audits":"passed"}';;
  *audit*--help*)
    printf 'Usage: test audit [OPTIONS]\n\nOptions:\n      --output <FORMAT>    Output format\n';;
  *--help*)
    echo "Usage: test [COMMAND]

Commands:
  audit   Run audits
  list    List items
  help    Print help";;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputAudit.run(&project).expect("audit should run");
        // The audit should find --output in the "audit" subcommand help
        // and validate JSON output
        assert!(
            matches!(result.status, AuditStatus::Pass | AuditStatus::Fail(_)),
            "expected Pass or Fail (not Skip), got {:?}",
            result.status
        );
    }

    #[test]
    fn json_output_probes_hand_written_command_block() {
        // herdr-shaped help: a `Common commands:` block whose entries all lead
        // with the tool name. The `count` subcommand carries the output flag.
        let script = r#"
case "$*" in
  *count*--output*json*|*count*--output=json*)
    echo '{"count":3}';;
  *count*--help*)
    printf 'Usage: test count [OPTIONS]\n\nOptions:\n      --output <FORMAT>    Output format\n';;
  *--help*)
    echo "Usage: test [options]

Common commands:
  test            Launch interactively
  test count      Count things
  test list       List things";;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputAudit.run(&project).expect("audit should run");
        assert_eq!(result.status, AuditStatus::Pass, "got {:?}", result.status);
    }

    #[test]
    fn json_output_does_not_probe_the_help_subcommand() {
        // `help --help` advertises --output and would validate as JSON, but
        // `help` is a built-in the audit never probes, so the tool opts out.
        let script = r#"
case "$*" in
  *help*--output*json*|*help*--output=json*)
    echo '{"help":true}';;
  help*--help*)
    printf 'Usage: test help [OPTIONS]\n\nOptions:\n      --output <FORMAT>    Output format\n';;
  *--help*)
    echo "Usage: test [COMMAND]

Commands:
  audit   Run audits
  help    Print help";;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputAudit.run(&project).expect("audit should run");
        assert!(
            matches!(result.status, AuditStatus::OptOut(_)),
            "expected OptOut, got {:?}",
            result.status
        );
    }

    #[test]
    fn subcommands_to_probe_clap_format_drops_help() {
        let help = HelpOutput::from_raw(
            "Usage: mycli [COMMAND]\n\nCommands:\n  audit   Run audits\n  list    List items\n  help    Print help\n\nOptions:\n  -h, --help  Print help\n",
        );
        assert_eq!(subcommands_to_probe(&help), ["audit", "list"]);
    }

    #[test]
    fn subcommands_to_probe_empty_without_a_command_block() {
        let help =
            HelpOutput::from_raw("Usage: mycli [OPTIONS]\n\nOptions:\n  -h, --help  Print help\n");
        assert!(subcommands_to_probe(&help).is_empty());
    }

    #[test]
    fn subcommands_to_probe_hand_written_block() {
        let help = HelpOutput::from_raw(
            "Usage: tool [options]\n\nCommon commands:\n  tool          Launch\n  tool run      Execute\n  tool build    Compile\n  tool completion zsh  Shell completions\n",
        );
        assert_eq!(subcommands_to_probe(&help), ["run", "build"]);
    }
}
