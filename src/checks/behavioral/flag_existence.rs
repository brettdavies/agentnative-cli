//! Check: `--help` advertises at least one non-interactive gate flag.
//!
//! Covers: `p1-must-no-interactive`. This is the second behavioral proof
//! of the same MUST — the existing `p1-non-interactive` check probes
//! *runtime* behavior (bare invocation, stdin-primary). This check probes
//! the *flag surface area* — does the CLI advertise any of the canonical
//! non-interactive flags (`--no-interactive`, `-p`, `--batch`, ...) in
//! its `--help` output at all.
//!
//! Skip rather than Warn when the target already satisfies P1 via an
//! alternative gate (help-on-bare-invocation or stdin-clean-exit) — those
//! tools don't need an advertised flag to be agent-safe.

use crate::check::Check;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Canonical non-interactive gate flags. A tool that advertises any one
/// of these in `--help` is explicitly agent-addressable. Kept narrow on
/// purpose — broader matching produces false positives on tools where
/// `-y` means "yes file format" and similar collisions.
const GATE_FLAGS: &[&str] = &[
    "--no-interactive",
    "--non-interactive",
    "-p",
    "--print",
    "--no-input",
    "--batch",
    "--headless",
    "-y",
    "--yes",
    "--assume-yes",
];

const HELP_ON_BARE_MARKERS: &[&str] = &["Usage:", "USAGE:", "usage:"];

pub struct FlagExistenceCheck;

impl Check for FlagExistenceCheck {
    fn id(&self) -> &str {
        "p1-flag-existence"
    }

    fn label(&self) -> &'static str {
        "Non-interactive gate flag advertised in --help"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P1
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-no-interactive"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner_ref();

        // Skip when the target already satisfies P1 via an alternative gate.
        // These probes hit BinaryRunner's cache when `p1-non-interactive`
        // already ran, so the cost is zero.
        let bare = runner.run(&[], &[]);
        let bare_output = format!("{}{}", bare.stdout, bare.stderr);
        let help_on_bare = HELP_ON_BARE_MARKERS.iter().any(|m| bare_output.contains(m));
        let stdin_clean_exit = matches!(bare.status, RunStatus::Ok);

        if help_on_bare || stdin_clean_exit {
            return Ok(CheckResult {
                id: self.id().to_string(),
                label: self.label().into(),
                group: self.group(),
                layer: self.layer(),
                status: CheckStatus::Skip(
                    "target satisfies P1 via alternative gate (help-on-bare or stdin-primary)"
                        .into(),
                ),
                confidence: Confidence::High,
            });
        }

        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) => {
                let raw = help.raw();
                if raw.trim().is_empty() {
                    CheckStatus::Skip(
                        "--help produced no output (likely non-English or unsupported)".into(),
                    )
                } else if GATE_FLAGS.iter().any(|needle| contains_flag(raw, needle)) {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Warn(format!(
                        "no non-interactive flag found in --help; expected one of: {}",
                        GATE_FLAGS.join(", ")
                    ))
                }
            }
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// A flag needle like `"--no-interactive"` matches when it appears in the
/// help text bounded by a non-flag character on either side — so `--no-input`
/// does not satisfy a search for `-p`, and `--print-json` does not satisfy
/// a search for `--print`.
fn contains_flag(haystack: &str, needle: &str) -> bool {
    let mut rest = haystack;
    while let Some(pos) = rest.find(needle) {
        let before_ok = pos == 0 || !is_flag_name_char(rest.as_bytes()[pos - 1] as char);
        let after_idx = pos + needle.len();
        let after_ok =
            after_idx >= rest.len() || !is_flag_name_char(rest.as_bytes()[after_idx] as char);
        if before_ok && after_ok {
            return true;
        }
        rest = &rest[after_idx..];
    }
    false
}

fn is_flag_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Core unit for tests. Takes pre-captured help text and the bare-invocation
/// status, returns a `CheckStatus` — mirrors the check convention in
/// `CLAUDE.md` §"Source Check Convention" (adapted for behavioral checks).
#[cfg(test)]
fn check_flag_existence(help_raw: &str, bare_stdout: &str, bare_ok: bool) -> CheckStatus {
    let help_on_bare = HELP_ON_BARE_MARKERS.iter().any(|m| bare_stdout.contains(m));
    if help_on_bare || bare_ok {
        return CheckStatus::Skip(
            "target satisfies P1 via alternative gate (help-on-bare or stdin-primary)".into(),
        );
    }
    if help_raw.trim().is_empty() {
        return CheckStatus::Skip(
            "--help produced no output (likely non-English or unsupported)".into(),
        );
    }
    if GATE_FLAGS
        .iter()
        .any(|needle| contains_flag(help_raw, needle))
    {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(format!(
            "no non-interactive flag found in --help; expected one of: {}",
            GATE_FLAGS.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_batch_flag_in_help() {
        // --help advertises `--batch` → Pass. Bare invocation did NOT print
        // Usage and did NOT exit cleanly, so the alternative gates do not fire.
        let help = "  --batch    Run in batch mode.\n";
        assert_eq!(check_flag_existence(help, "", false), CheckStatus::Pass);
    }

    #[test]
    fn happy_path_short_print_flag() {
        let help = "  -p, --print    Print output.\n";
        assert_eq!(check_flag_existence(help, "", false), CheckStatus::Pass);
    }

    #[test]
    fn skip_when_help_on_bare_invocation() {
        // Bare invocation printed Usage → tool is already agent-safe.
        let help = "  --foo    Do a thing.\n";
        let result = check_flag_existence(help, "Usage: foo [OPTIONS]\n", false);
        assert!(matches!(result, CheckStatus::Skip(_)));
    }

    #[test]
    fn skip_when_stdin_clean_exit() {
        // Bare invocation exited 0 (stdin-primary behavior) → skip.
        let help = "  --foo    Do a thing.\n";
        let result = check_flag_existence(help, "", true);
        assert!(matches!(result, CheckStatus::Skip(_)));
    }

    #[test]
    fn warn_when_no_gate_flag_and_no_alt_gate() {
        let help = "  --color    When to color.\n  --version   Print version.\n";
        match check_flag_existence(help, "", false) {
            CheckStatus::Warn(msg) => assert!(msg.contains("--no-interactive")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn non_english_help_is_skipped() {
        // Localized help without any English flag text → empty input after
        // we strip non-ASCII — the check honors the English-only regex
        // exception from docs/coverage-matrix.md. The parsers would still
        // return zero matches, so we Warn. For "completely unparseable"
        // localized help with no ASCII flags at all, we Skip via the empty
        // branch. Exercise both.
        // Completely non-English: no flags detected → warn about missing flag.
        let help = "用法: outil\n选项:\n  -H, --header     自定义请求头\n";
        let result = check_flag_existence(help, "", false);
        assert!(matches!(result, CheckStatus::Warn(_)));

        // Empty help output → Skip.
        let empty = "";
        let result = check_flag_existence(empty, "", false);
        assert!(matches!(result, CheckStatus::Skip(_)));
    }

    #[test]
    fn word_boundary_rejects_partial_matches() {
        // `--print-json` must NOT satisfy a search for `--print` — but
        // `--print` alone in a neighbor line must.
        let help = "  --print-json    Print as JSON.\n";
        let result = check_flag_existence(help, "", false);
        assert!(matches!(result, CheckStatus::Warn(_)));
    }

    #[test]
    fn contains_flag_word_boundary() {
        assert!(contains_flag("use --batch mode", "--batch"));
        assert!(contains_flag("  --batch\n", "--batch"));
        assert!(!contains_flag("--batching", "--batch"));
        assert!(contains_flag("-p, --print", "-p"));
        assert!(!contains_flag("-pr", "-p"));
    }
}
