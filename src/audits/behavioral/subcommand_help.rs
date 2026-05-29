//! Shared subcommand-help recursion for the P3/P6 example audits.
//!
//! Three behavioral audits need to probe `<bin> <subcmd> --help` for every
//! top-level subcommand and inspect the result. This module centralizes the
//! recursion so the three audits share probe results (via the runner cache)
//! and so the skip rules — built-in `help`/`completions`, recursion cap —
//! live in one place.
//!
//! Recursion cap: one level deep. Nested subcommands would multiply spawn
//! counts geometrically (`tool a b c --help`), and the SHOULDs/MUSTs all
//! attach to first-level subcommands.

use crate::runner::{BinaryRunner, HelpOutput, RunStatus};

/// Subcommand names skipped during recursion. `help` echoes top-level help;
/// `completions` and `complete` shells produce huge token-dumps unrelated
/// to the documented surface.
const SKIP_SUBCOMMANDS: &[&str] = &["help", "completions", "completion", "complete"];

/// Probe `<bin> <subcmd> --help` for every top-level subcommand surfaced by
/// the parsed top-level help. Returns `(name, help_output)` for each
/// subcommand whose probe succeeded; subcommands that refuse `--help` or
/// crash are silently dropped so callers can iterate without re-handling
/// probe failures.
///
/// The returned vector preserves the parser's order, which mirrors the
/// `Commands:` section in the binary's help text.
pub(crate) fn probe_subcommands(
    runner: &BinaryRunner,
    top_help: &HelpOutput,
) -> Vec<(String, HelpOutput)> {
    let mut out = Vec::new();
    for name in top_help.subcommands() {
        if should_skip(name) {
            continue;
        }
        let result = runner.run(&[name, "--help"], &[]);
        // Capture partial output from timeouts/crashes the same way HelpOutput::probe does.
        // Only NotFound / PermissionDenied / Error are dropped here — those mean we
        // couldn't even spawn the child, not that the subcommand misbehaved.
        match result.status {
            RunStatus::Ok | RunStatus::Timeout | RunStatus::Crash { .. } => {
                let mut raw = String::with_capacity(result.stdout.len() + result.stderr.len());
                raw.push_str(&result.stdout);
                raw.push_str(&result.stderr);
                if raw.trim().is_empty() {
                    continue;
                }
                out.push((name.clone(), HelpOutput::from_raw(raw)));
            }
            _ => continue,
        }
    }
    out
}

fn should_skip(name: &str) -> bool {
    SKIP_SUBCOMMANDS
        .iter()
        .any(|s| name.eq_ignore_ascii_case(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_help_and_completions() {
        assert!(should_skip("help"));
        assert!(should_skip("HELP"));
        assert!(should_skip("completions"));
        assert!(should_skip("completion"));
        assert!(should_skip("complete"));
    }

    #[test]
    fn does_not_skip_real_subcommands() {
        assert!(!should_skip("audit"));
        assert!(!should_skip("generate"));
        assert!(!should_skip("schema"));
    }
}
