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

use crate::runner::{BinaryRunner, RunResult};

/// The bad flag used by every error-battery probe.
pub(crate) const BAD_FLAG: &str = "--this-flag-does-not-exist-anc";

/// Run the binary with [`BAD_FLAG`]. Cached by the runner across audits, so
/// the five error-battery audits share a single child process.
pub(crate) fn probe_bad_invocation(runner: &BinaryRunner) -> RunResult {
    runner.run(&[BAD_FLAG], &[])
}

/// Why a JSON error audit has nothing to probe: the opening of its Skip
/// evidence.
pub(crate) const NO_JSON_MODE: &str = "no option definition in --help declares a JSON mode (an \
     output flag whose definition names json, or a --json or --jsonl alias)";

/// Run the binary with the bad flag in JSON mode, `mode` being the arguments
/// [`json_mode`](crate::audits::behavioral::output_flags::json_mode)
/// returned. Used by the JSON-specific MUSTs to see whether the CLI honors
/// the JSON contract for error reporting.
pub(crate) fn probe_bad_invocation_json(runner: &BinaryRunner, mode: &[String]) -> RunResult {
    let args: Vec<&str> = std::iter::once(BAD_FLAG)
        .chain(mode.iter().map(String::as_str))
        .collect();
    runner.run(&args, &[])
}

/// Run `--help` in JSON mode, the success-envelope baseline for
/// `p2-should-consistent-envelope`. `--help` is the only universally-safe
/// command to pair with a JSON mode without risking side effects.
pub(crate) fn probe_help_json(runner: &BinaryRunner, mode: &[String]) -> RunResult {
    let args: Vec<&str> = std::iter::once("--help")
        .chain(mode.iter().map(String::as_str))
        .collect();
    runner.run(&args, &[])
}

/// `mode` as the evidence quotes it: `--output json`.
pub(crate) fn shown(mode: &[String]) -> String {
    mode.join(" ")
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

    #[test]
    fn bad_flag_constant_is_long_form() {
        assert!(BAD_FLAG.starts_with("--"));
        assert!(BAD_FLAG.len() > 5);
    }
}
