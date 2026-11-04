//! Shared bad-invocation probe for the P2/P4 error-battery audits.
//!
//! Five behavioral audits share the same probe shape — run the binary with a
//! deliberately-bad flag, then inspect exit code / stderr / JSON envelope.
//! Centralizing the probe argument and the JSON-applicability gate keeps the
//! five audits consistent and means a future spec edit only touches one file.
//! Whether a tool has a JSON mode, and how to select it, comes from
//! [`json_mode`](crate::audits::behavioral::output_flags::json_mode).
//!
//! Probe choice rationale: an unknown long flag (`--this-flag-does-not-exist-anc`)
//! is universally rejected — clap exits 2, argparse exits 2, click exits 2,
//! cobra exits 1. Long form avoids `-x` collisions; the suffix names the
//! probe author so accidental matches against real flags are vanishingly rare.
//!
//! A probe of a subcommand ends in `--help`, because anc never calls a
//! subcommand without it: clap, cobra and click stop at the bad flag, and a
//! parser that ignores it or defers it (argparse) prints help instead of
//! running the subcommand.

use crate::audits::behavioral::output_flags::JsonMode;
use crate::runner::{BinaryRunner, RunResult, RunStatus};
use crate::types::AuditStatus;

/// The bad flag used by every error-battery probe.
pub(crate) const BAD_FLAG: &str = "--this-flag-does-not-exist-anc";

/// Run the binary with [`BAD_FLAG`]. Cached by the runner across audits, so
/// the five error-battery audits share a single child process.
pub(crate) fn probe_bad_invocation(runner: &BinaryRunner) -> RunResult {
    runner.run(&[BAD_FLAG], &[])
}

/// Why a JSON error audit has nothing to probe: the opening of its Skip
/// evidence.
pub(crate) const NO_JSON_MODE: &str = "no option definition in --help, or in the --help of a \
     top-level subcommand, declares a JSON mode (an output flag whose definition names json, or a \
     --json or --jsonl alias)";

/// Run the binary with the bad flag in JSON mode. Used by the JSON-specific
/// audits to see whether the CLI honors the JSON contract for error
/// reporting. `Err` holds the Skip for a subcommand that did not reject the
/// flag, whose error path a `--help` probe cannot reach.
pub(crate) fn probe_bad_invocation_json(
    runner: &BinaryRunner,
    mode: &JsonMode,
) -> Result<RunResult, AuditStatus> {
    let mut args = mode.args_with(&[BAD_FLAG]);
    if mode.subcommand.is_none() {
        return Ok(runner.run(&args, &[]));
    }
    args.push("--help");
    let result = runner.run(&args, &[]);
    if result.status == RunStatus::Ok && result.exit_code != Some(0) {
        return Ok(result);
    }
    Err(AuditStatus::Skip(format!(
        "`{}` answered --help rather than rejecting the flag, and anc calls a subcommand only \
         with --help, so its JSON error path could not be measured.",
        args.join(" ")
    )))
}

/// Run `--help` in JSON mode, the success-envelope baseline for
/// `p2-should-consistent-envelope`. `--help` is the only universally-safe
/// command to pair with a JSON mode without risking side effects.
pub(crate) fn probe_help_json(runner: &BinaryRunner, mode: &JsonMode) -> RunResult {
    runner.run(&mode.args_with(&["--help"]), &[])
}

/// Try to parse JSON from either stderr or stdout, preferring stderr (the
/// spec-mandated channel for error envelopes). Returns the parsed value
/// and which channel it came from for evidence text.
pub(crate) fn parse_error_envelope(
    result: &RunResult,
) -> Option<(serde_json::Value, &'static str)> {
    if !result.stderr.trim().is_empty()
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(result.stderr.trim())
    {
        return Some((v, "stderr"));
    }
    if !result.stdout.trim().is_empty()
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(result.stdout.trim())
    {
        return Some((v, "stdout"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::Audit;
    use crate::audits::behavioral::consistent_envelope::ConsistentEnvelopeAudit;
    use crate::audits::behavioral::json_error_output::JsonErrorOutputAudit;
    use crate::audits::behavioral::json_errors::JsonErrorsAudit;
    use crate::audits::behavioral::tests::test_project_with_sh_script;
    use crate::types::AuditStatus;

    #[test]
    fn bad_flag_constant_is_long_form() {
        assert!(BAD_FLAG.starts_with("--"));
        assert!(BAD_FLAG.len() > 5);
    }

    /// The statuses of the three JSON error audits on `script`: `p2-json-errors`,
    /// `p4-json-error-output`, `p2-consistent-envelope`.
    fn error_rows(script: &str) -> [AuditStatus; 3] {
        let project = test_project_with_sh_script(script);
        let audits: [&dyn Audit; 3] = [
            &JsonErrorsAudit,
            &JsonErrorOutputAudit,
            &ConsistentEnvelopeAudit,
        ];
        audits.map(|audit| audit.run(&project).expect("audit should run").status)
    }

    /// The top-level help and `list --help` of a tool whose JSON mode exists
    /// only on `list`, for a `case "$*"` to fall through to.
    const SUBCOMMAND_ONLY_HELP: &str = r#"
  "list --help")
    printf 'Usage: tool list [OPTIONS]\n\nOptions:\n  --output <FMT>  json|text\n';;
  "--help")
    printf 'Usage: tool [COMMAND]\n\nCommands:\n  list    List items\n\nOptions:\n  -h, --help    Show help\n';;
  *) echo "hello";;
esac
"#;

    fn subcommand_only(cases: &str) -> String {
        format!("case \"$*\" in\n{cases}{SUBCOMMAND_ONLY_HELP}")
    }

    #[test]
    fn a_json_mode_only_on_a_subcommand_puts_all_three_error_audits_on_it() {
        let script = subcommand_only(
            r#"  "list --this-flag-does-not-exist-anc --output json --help")
    echo '{"error":"BadFlag","kind":"usage","message":"unknown flag found."}' >&2; exit 2;;
  "list --help --output json") echo '{"kind":"list","data":[]}';;
"#,
        );
        assert_eq!(
            error_rows(&script),
            [AuditStatus::Pass, AuditStatus::Pass, AuditStatus::Pass]
        );
    }

    #[test]
    fn a_subcommand_error_in_plain_text_is_graded_and_names_the_subcommand() {
        let script = subcommand_only(
            r#"  "list --this-flag-does-not-exist-anc --output json --help")
    echo "error: unknown flag" >&2; exit 2;;
"#,
        );
        let note = "`list --help` declares this JSON mode, and the top-level --help declares \
                    none. P2 counts any flag that selects JSON as a JSON output mode, and this \
                    requirement applies under it.";
        let [errors, output, envelope] = error_rows(&script);
        match errors {
            AuditStatus::Fail(msg) => {
                assert!(msg.contains("under `list --output json`"), "{msg}");
                assert!(msg.ends_with(note), "{msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
        match output {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("under `list --output json`"), "{msg}");
                assert!(msg.ends_with(note), "{msg}");
            }
            other => panic!("expected Warn, got {other:?}"),
        }
        match envelope {
            AuditStatus::Skip(msg) => assert!(msg.contains("success-mode"), "{msg}"),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn a_subcommand_probe_never_runs_without_help_and_a_parser_that_answers_help_is_not_graded() {
        // Prints a passing error for any call on `list` without `--help`, the
        // one way a probe could reach the subcommand's own behavior.
        let script = r#"
case "$*" in
  "list --help"|list*--help*)
    printf 'Usage: tool list [OPTIONS]\n\nOptions:\n  --output <FMT>  json|text\n';;
  list*)
    echo '{"error":"BadFlag","kind":"usage","message":"ran without --help."}' >&2; exit 2;;
  "--help")
    printf 'Usage: tool [COMMAND]\n\nCommands:\n  list    List items\n';;
  *) echo "hello";;
esac
"#;
        for status in error_rows(script) {
            match status {
                AuditStatus::Skip(msg) => assert!(
                    msg.contains("answered --help rather than rejecting the flag"),
                    "{msg}"
                ),
                other => panic!("expected Skip, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_top_level_output_flag_that_writes_a_file_falls_through_to_the_subcommand() {
        let script = r#"
case "$*" in
  "list --this-flag-does-not-exist-anc --format json --help")
    echo '{"error":"BadFlag","kind":"usage","message":"unknown flag found."}' >&2; exit 2;;
  "list --help")
    printf 'Usage: tool list [OPTIONS]\n\nOptions:\n  --format <F>  Output format (text, json)\n';;
  "--help")
    printf 'Usage: tool [COMMAND]\n\nCommands:\n  list    List items\n\nOptions:\n  -o, --output <FILE>    Write to FILE\n';;
  *) echo "hello";;
esac
"#;
        let [errors, ..] = error_rows(script);
        assert_eq!(errors, AuditStatus::Pass);
    }

    #[test]
    fn a_top_level_json_alias_keeps_the_probes_at_the_top_level() {
        // rg's shape: the subcommand search never runs.
        let script = r#"
case "$*" in
  "--this-flag-does-not-exist-anc --json")
    echo '{"error":"BadFlag","kind":"usage","message":"unknown flag found."}' >&2; exit 2;;
  "--help") printf 'Usage: tool [OPTIONS]\n\nOptions:\n      --json    Print JSON\n';;
  *) echo "hello";;
esac
"#;
        let [errors, output, _] = error_rows(script);
        assert_eq!(errors, AuditStatus::Pass);
        assert_eq!(output, AuditStatus::Pass);
    }

    #[test]
    fn no_json_mode_anywhere_skips_and_says_where_it_looked() {
        let script = r#"
case "$*" in
  "list --help") printf 'Usage: tool list [OPTIONS]\n\nOptions:\n  -h, --help    Show help\n';;
  "--help") printf 'Usage: tool [COMMAND]\n\nCommands:\n  list    List items\n';;
  *) echo "hello";;
esac
"#;
        for status in error_rows(script) {
            match status {
                AuditStatus::Skip(msg) => assert!(
                    msg.starts_with(
                        "no option definition in --help, or in the --help of a top-level \
                         subcommand, declares a JSON mode"
                    ),
                    "{msg}"
                ),
                other => panic!("expected Skip, got {other:?}"),
            }
        }
    }
}
