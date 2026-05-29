//! Shared bad-invocation probe for the P2/P4 error-battery audits.
//!
//! Five behavioral audits share the same probe shape — run the binary with a
//! deliberately-bad flag, then inspect exit code / stderr / JSON envelope.
//! Centralizing the probe argument and the JSON-applicability gate keeps the
//! five audits consistent and means a future spec edit only touches one file.
//!
//! Probe choice rationale: an unknown long flag (`--this-flag-does-not-exist-anc`)
//! is universally rejected — clap exits 2, argparse exits 2, click exits 2,
//! cobra exits 1. Long form avoids `-x` collisions; the suffix names the
//! probe author so accidental matches against real flags are vanishingly rare.

use crate::runner::{BinaryRunner, HelpOutput, RunResult};

/// The bad flag used by every error-battery probe.
pub(crate) const BAD_FLAG: &str = "--this-flag-does-not-exist-anc";

/// Run the binary with [`BAD_FLAG`]. Cached by the runner across audits, so
/// the five error-battery audits share a single child process.
pub(crate) fn probe_bad_invocation(runner: &BinaryRunner) -> RunResult {
    runner.run(&[BAD_FLAG], &[])
}

/// Run the binary with the bad flag plus `--output json`. Used by the
/// JSON-specific MUSTs to see whether the CLI honors the JSON contract
/// for error reporting.
pub(crate) fn probe_bad_invocation_json(runner: &BinaryRunner) -> RunResult {
    runner.run(&[BAD_FLAG, "--output", "json"], &[])
}

/// Run `--help --output json` — used as the success-envelope baseline for
/// `p2-should-consistent-envelope`. `--help` is the only universally-safe
/// command we can pair with `--output json` without risking side effects.
pub(crate) fn probe_help_json(runner: &BinaryRunner) -> RunResult {
    runner.run(&["--help", "--output", "json"], &[])
}

/// True iff the help surface advertises `--output json` (or the canonical
/// `--output ... json` value-list shape). Gates the JSON-specific MUSTs to
/// CLIs that opt into the JSON contract.
///
/// The audit is intentionally lenient: any co-occurrence of `--output` and
/// `json` in the help text counts. False positives are recoverable (the
/// downstream probe simply observes a non-JSON error) and false negatives
/// (the more painful direction) are rare because every JSON-supporting CLI
/// surfaces both tokens in `--help`.
pub(crate) fn advertises_json_output(help: &HelpOutput) -> bool {
    let raw = help.raw().to_lowercase();
    raw.contains("--output") && (raw.contains("json") || raw.contains("jsonl"))
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
    use crate::runner::HelpOutput;

    #[test]
    fn advertises_json_when_output_and_json_present() {
        let help = HelpOutput::from_raw(
            "Options:\n  --output <FMT>    Output format [possible values: text, json]\n",
        );
        assert!(advertises_json_output(&help));
    }

    #[test]
    fn advertises_json_when_jsonl_mentioned() {
        let help = HelpOutput::from_raw("Options:\n  --output <FMT>    text, json, jsonl\n");
        assert!(advertises_json_output(&help));
    }

    #[test]
    fn does_not_advertise_when_output_missing() {
        let help = HelpOutput::from_raw("Options:\n  --format <FMT>    output as json\n");
        assert!(!advertises_json_output(&help));
    }

    #[test]
    fn does_not_advertise_when_json_missing() {
        let help = HelpOutput::from_raw("Options:\n  --output <FMT>    Output format\n");
        assert!(!advertises_json_output(&help));
    }

    #[test]
    fn bad_flag_constant_is_long_form() {
        assert!(BAD_FLAG.starts_with("--"));
        assert!(BAD_FLAG.len() > 5);
    }
}
