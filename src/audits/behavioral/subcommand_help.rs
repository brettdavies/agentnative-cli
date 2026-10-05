//! Shared subcommand-help recursion for the behavioral audits that read a
//! subcommand's own `--help`.
//!
//! The example audits probe `<bin> <subcmd> --help` for every top-level
//! subcommand; the gated audits (`p7-limit`, `p7-cursor-pagination`,
//! `p7-timeout-behavioral`, `p6-stdin-input`) probe only the subcommands
//! that trigger their gate, because a CLI puts `--limit` or `--timeout` on
//! the subcommand that needs it. Every probe goes through the runner cache,
//! so audits that read the same subcommand share one spawn, and the skip
//! rules (built-in `help`/`completions`, recursion cap) live in one place.
//!
//! Recursion cap: one level deep. Nested subcommands would multiply spawn
//! counts geometrically (`tool a b c --help`), and the SHOULDs/MUSTs all
//! attach to first-level subcommands.

use crate::runner::help_probe::Flag;
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
    let names: Vec<&str> = top_help.subcommands().iter().map(String::as_str).collect();
    probe_named(runner, &names)
}

/// Probe `<bin> <name> --help` for each of `names`, under the same skip and
/// drop rules as [`probe_subcommands`].
pub(crate) fn probe_named(runner: &BinaryRunner, names: &[&str]) -> Vec<(String, HelpOutput)> {
    let mut out = Vec::new();
    for &name in names {
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
                out.push((name.to_string(), HelpOutput::from_raw(raw)));
            }
            _ => continue,
        }
    }
    out
}

/// Whether `name` is a built-in the subcommand probes leave alone.
pub(crate) fn should_skip(name: &str) -> bool {
    SKIP_SUBCOMMANDS
        .iter()
        .any(|s| name.eq_ignore_ascii_case(s))
}

/// Which of a gate's triggering subcommands advertise what an audit looks
/// for, and where each one advertises it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Coverage<'a> {
    /// Subcommands that advertise it, each with what was found: the matched
    /// flag or phrase, suffixed `, top-level` when only the top-level help
    /// carries it.
    pub with: Vec<(&'a str, String)>,
    /// Subcommands whose own help and the top-level help both lack it,
    /// including any whose `--help` probe returned nothing.
    pub without: Vec<&'a str>,
}

impl Coverage<'_> {
    /// `list (--max), search (--limit, top-level)`.
    pub fn with_list(&self) -> String {
        self.with
            .iter()
            .map(|(name, found)| format!("{name} ({found})"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// `get, show`.
    pub fn without_list(&self) -> String {
        self.without.join(", ")
    }
}

/// Look for what `find` reports in each triggering subcommand's own help,
/// then in the top-level help. A top-level match counts for every
/// subcommand: a flag the top-level help lists (a global `--timeout`) is
/// accepted wherever the subcommand runs.
pub(crate) fn coverage<'a>(
    triggers: &[&'a str],
    top_help: &HelpOutput,
    subhelp: &[(String, HelpOutput)],
    find: impl Fn(&HelpOutput) -> Option<String>,
) -> Coverage<'a> {
    let top_level = find(top_help).map(|found| format!("{found}, top-level"));
    let mut with = Vec::new();
    let mut without = Vec::new();
    for &name in triggers {
        let own = subhelp
            .iter()
            .find(|(sub, _)| sub == name)
            .and_then(|(_, help)| find(help));
        match own.or_else(|| top_level.clone()) {
            Some(found) => with.push((name, found)),
            None => without.push(name),
        }
    }
    Coverage { with, without }
}

/// The first flag in `help` that `wanted` accepts, named by its long form
/// when it has one (`--max`, else `-n`).
pub(crate) fn first_flag(help: &HelpOutput, wanted: impl Fn(&Flag) -> bool) -> Option<String> {
    help.flags()
        .iter()
        .find(|f| wanted(f))
        .and_then(|f| f.long.clone().or_else(|| f.short.clone()))
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
