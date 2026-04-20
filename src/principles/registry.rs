//! Flat `&'static [Requirement]` registry covering every MUST, SHOULD, and
//! MAY across P1–P7. The registry is the single source of truth linking
//! spec requirements to the checks that verify them via `Check::covers()`.
//!
//! IDs follow the pattern `p{N}-{level}-{key}`. They are stable and must
//! not change once published — scorecards and the coverage matrix pin
//! against them.

use serde::Serialize;

/// Severity level of a spec requirement.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Must,
    Should,
    May,
}

/// Whether a requirement applies to every CLI or only when a condition holds.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "condition", rename_all = "lowercase")]
pub enum Applicability {
    Universal,
    Conditional(&'static str),
}

/// Categories under which a tool may be exempt from specific requirements.
/// Referenced by scorecard `audit_profile`.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[allow(dead_code)] // Reserved for v0.1.3 audit_profile consumption.
pub enum ExceptionCategory {
    /// TUI-by-design tools (lazygit, k9s, btop). Interactive-prompt MUSTs
    /// suppressed; TTY-driving-agent access is out-of-scope for verification.
    HumanTui,
    /// File-traversal utilities (fd, find). Subcommand-structure SHOULDs
    /// relaxed; these tools have no subcommands by design.
    FileTraversal,
    /// POSIX utilities (cat, sed, awk). Stdin-as-primary-input is their
    /// contract; P1 interactive-prompt MUSTs satisfied vacuously.
    PosixUtility,
    /// Diagnostic tools (nvidia-smi, vmstat). No write operations, so P5
    /// MUSTs do not apply.
    Diagnostic,
}

/// A single spec requirement. The flat registry below is iterated by the
/// matrix generator and cross-referenced against `Check::covers()`.
#[derive(Debug, Clone, Serialize)]
pub struct Requirement {
    pub id: &'static str,
    pub principle: u8,
    pub level: Level,
    pub summary: &'static str,
    pub applicability: Applicability,
}

/// Every MUST/SHOULD/MAY in the spec. Order groups by principle, then level
/// (MUST → SHOULD → MAY) so readers can scan down a principle cleanly.
pub static REQUIREMENTS: &[Requirement] = &[
    // --- P1: Non-Interactive by Default ---
    Requirement {
        id: "p1-must-env-var",
        principle: 1,
        level: Level::Must,
        summary: "Every flag settable via environment variable (falsey-value parser for booleans).",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p1-must-no-interactive",
        principle: 1,
        level: Level::Must,
        summary: "`--no-interactive` flag gates every prompt library call; when set or stdin is not a TTY, use defaults/stdin or exit with an actionable error.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p1-must-no-browser",
        principle: 1,
        level: Level::Must,
        summary: "Headless authentication path (`--no-browser` / OAuth Device Authorization Grant).",
        applicability: Applicability::Conditional("CLI authenticates against a remote service"),
    },
    Requirement {
        id: "p1-should-tty-detection",
        principle: 1,
        level: Level::Should,
        summary: "Auto-detect non-interactive context via TTY detection; suppress prompts when stderr is not a terminal.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p1-should-defaults-in-help",
        principle: 1,
        level: Level::Should,
        summary: "Document default values for prompted inputs in `--help` output.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p1-may-rich-tui",
        principle: 1,
        level: Level::May,
        summary: "Rich interactive experiences (spinners, progress bars, menus) when TTY is detected and `--no-interactive` is not set.",
        applicability: Applicability::Universal,
    },
    // --- P2: Structured Output ---
    Requirement {
        id: "p2-must-output-flag",
        principle: 2,
        level: Level::Must,
        summary: "`--output text|json|jsonl` flag selects output format; `OutputFormat` enum threaded through output paths.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p2-must-stdout-stderr-split",
        principle: 2,
        level: Level::Must,
        summary: "Data goes to stdout; diagnostics/progress/warnings go to stderr — never interleaved.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p2-must-exit-codes",
        principle: 2,
        level: Level::Must,
        summary: "Exit codes are structured and documented (0 success, 1 general, 2 usage, 77 auth, 78 config).",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p2-must-json-errors",
        principle: 2,
        level: Level::Must,
        summary: "When `--output json` is active, errors are emitted as JSON (to stderr) with at least `error`, `kind`, and `message` fields.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p2-should-consistent-envelope",
        principle: 2,
        level: Level::Should,
        summary: "JSON output uses a consistent envelope — a top-level object with predictable keys — across every command.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p2-may-more-formats",
        principle: 2,
        level: Level::May,
        summary: "Additional output formats (CSV, TSV, YAML) beyond the core three.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p2-may-raw-flag",
        principle: 2,
        level: Level::May,
        summary: "`--raw` flag for unformatted output suitable for piping to other tools.",
        applicability: Applicability::Universal,
    },
    // --- P3: Progressive Help Discovery ---
    Requirement {
        id: "p3-must-subcommand-examples",
        principle: 3,
        level: Level::Must,
        summary: "Every subcommand ships at least one concrete invocation example (`after_help` in clap).",
        applicability: Applicability::Conditional("CLI uses subcommands"),
    },
    Requirement {
        id: "p3-must-top-level-examples",
        principle: 3,
        level: Level::Must,
        summary: "The top-level command ships 2–3 examples covering the primary use cases.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p3-should-paired-examples",
        principle: 3,
        level: Level::Should,
        summary: "Examples show human and agent invocations side by side (text then `--output json` equivalent).",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p3-should-about-long-about",
        principle: 3,
        level: Level::Should,
        summary: "Short `about` for command-list summaries; `long_about` reserved for detailed descriptions visible with `--help`.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p3-may-examples-subcommand",
        principle: 3,
        level: Level::May,
        summary: "Dedicated `examples` subcommand or `--examples` flag for curated usage patterns.",
        applicability: Applicability::Universal,
    },
    // --- P4: Fail Fast, Actionable Errors ---
    Requirement {
        id: "p4-must-try-parse",
        principle: 4,
        level: Level::Must,
        summary: "Parse arguments with `try_parse()` instead of `parse()` so `--output json` can emit JSON parse errors.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p4-must-exit-code-mapping",
        principle: 4,
        level: Level::Must,
        summary: "Error types map to distinct exit codes (0, 1, 2, 77, 78).",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p4-must-actionable-errors",
        principle: 4,
        level: Level::Must,
        summary: "Every error message contains what failed, why, and what to do next.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p4-should-structured-enum",
        principle: 4,
        level: Level::Should,
        summary: "Error types use a structured enum (via `thiserror` in Rust) with variant-to-kind mapping for JSON serialization.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p4-should-gating-before-network",
        principle: 4,
        level: Level::Should,
        summary: "Config and auth validation happen before any network call (three-tier dependency gating).",
        applicability: Applicability::Conditional("CLI makes network calls"),
    },
    Requirement {
        id: "p4-should-json-error-output",
        principle: 4,
        level: Level::Should,
        summary: "Error output respects `--output json`: JSON-formatted errors go to stderr when JSON output is selected.",
        applicability: Applicability::Universal,
    },
    // --- P5: Safe Retries, Mutation Boundaries ---
    Requirement {
        id: "p5-must-force-yes",
        principle: 5,
        level: Level::Must,
        summary: "Destructive operations (delete, overwrite, bulk modify) require an explicit `--force` or `--yes` flag.",
        applicability: Applicability::Conditional("CLI has destructive operations"),
    },
    Requirement {
        id: "p5-must-read-write-distinction",
        principle: 5,
        level: Level::Must,
        summary: "The distinction between read and write commands is clear from the command name and help text alone.",
        applicability: Applicability::Conditional("CLI has both read and write operations"),
    },
    Requirement {
        id: "p5-must-dry-run",
        principle: 5,
        level: Level::Must,
        summary: "A `--dry-run` flag is present on every write command; dry-run output respects `--output json`.",
        applicability: Applicability::Conditional("CLI has write operations"),
    },
    Requirement {
        id: "p5-should-idempotency",
        principle: 5,
        level: Level::Should,
        summary: "Write operations are idempotent where the domain allows it — running the same command twice produces the same result.",
        applicability: Applicability::Conditional("CLI has write operations"),
    },
    // --- P6: Composable, Predictable Command Structure ---
    Requirement {
        id: "p6-must-sigpipe",
        principle: 6,
        level: Level::Must,
        summary: "SIGPIPE fix is the first executable statement in `main()` — piping output to `head`/`tail` must not panic.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p6-must-no-color",
        principle: 6,
        level: Level::Must,
        summary: "TTY detection plus support for `NO_COLOR` and `TERM=dumb` — color codes suppressed when stdout/stderr is not a terminal.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p6-must-completions",
        principle: 6,
        level: Level::Must,
        summary: "Shell completions available via a `completions` subcommand (Tier 1 meta-command — needs no config/auth/network).",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p6-must-timeout-network",
        principle: 6,
        level: Level::Must,
        summary: "Network CLIs ship a `--timeout` flag with a sensible default (e.g., 30 seconds).",
        applicability: Applicability::Conditional("CLI makes network calls"),
    },
    Requirement {
        id: "p6-must-no-pager",
        principle: 6,
        level: Level::Must,
        summary: "If the CLI uses a pager (`less`, `more`, `$PAGER`), it supports `--no-pager` or respects `PAGER=\"\"`.",
        applicability: Applicability::Conditional("CLI invokes a pager for output"),
    },
    Requirement {
        id: "p6-must-global-flags",
        principle: 6,
        level: Level::Must,
        summary: "Agentic flags (`--output`, `--quiet`, `--no-interactive`, `--timeout`) are `global = true` so they propagate to every subcommand.",
        applicability: Applicability::Conditional("CLI uses subcommands"),
    },
    Requirement {
        id: "p6-should-stdin-input",
        principle: 6,
        level: Level::Should,
        summary: "Commands that accept input read from stdin when no file argument is provided.",
        applicability: Applicability::Conditional("CLI has commands that accept input data"),
    },
    Requirement {
        id: "p6-should-consistent-naming",
        principle: 6,
        level: Level::Should,
        summary: "Subcommand naming follows a consistent `noun verb` or `verb noun` convention throughout the tool.",
        applicability: Applicability::Conditional("CLI uses subcommands"),
    },
    Requirement {
        id: "p6-should-tier-gating",
        principle: 6,
        level: Level::Should,
        summary: "Three-tier dependency gating: Tier 1 (meta) needs nothing, Tier 2 (local) needs config, Tier 3 (network) needs config + auth.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p6-should-subcommand-operations",
        principle: 6,
        level: Level::Should,
        summary: "Operations are modeled as subcommands, not flags (`tool search \"q\"`, not `tool --search \"q\"`).",
        applicability: Applicability::Conditional("CLI performs multiple distinct operations"),
    },
    Requirement {
        id: "p6-may-color-flag",
        principle: 6,
        level: Level::May,
        summary: "`--color auto|always|never` flag for explicit color control beyond TTY auto-detection.",
        applicability: Applicability::Universal,
    },
    // --- P7: Bounded, High-Signal Responses ---
    Requirement {
        id: "p7-must-quiet",
        principle: 7,
        level: Level::Must,
        summary: "A `--quiet` flag suppresses non-essential output; only requested data and errors appear.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p7-must-list-clamping",
        principle: 7,
        level: Level::Must,
        summary: "List operations clamp to a sensible default maximum; when truncated, indicate it (`\"truncated\": true` in JSON, stderr note in text).",
        applicability: Applicability::Conditional("CLI has list-style commands"),
    },
    Requirement {
        id: "p7-should-verbose",
        principle: 7,
        level: Level::Should,
        summary: "A `--verbose` flag (or `-v` / `-vv`) escalates diagnostic detail when agents need to debug failures.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p7-should-limit",
        principle: 7,
        level: Level::Should,
        summary: "A `--limit` or `--max-results` flag lets callers request exactly the number of items they want.",
        applicability: Applicability::Conditional("CLI has list-style commands"),
    },
    Requirement {
        id: "p7-should-timeout",
        principle: 7,
        level: Level::Should,
        summary: "A `--timeout` flag bounds execution time so agents are not blocked indefinitely.",
        applicability: Applicability::Universal,
    },
    Requirement {
        id: "p7-may-cursor-pagination",
        principle: 7,
        level: Level::May,
        summary: "Cursor-based pagination flags (`--after`, `--before`) for efficient traversal of large result sets.",
        applicability: Applicability::Conditional("CLI returns paginated results"),
    },
    Requirement {
        id: "p7-may-auto-verbosity",
        principle: 7,
        level: Level::May,
        summary: "Automatic verbosity reduction in non-TTY contexts (same behavior `--quiet` explicitly requests).",
        applicability: Applicability::Universal,
    },
];

/// Look up a requirement by ID. Returns `None` if the ID is not registered.
pub fn find(id: &str) -> Option<&'static Requirement> {
    REQUIREMENTS.iter().find(|r| r.id == id)
}

/// Count requirements at a given level. Test helper + doc convenience.
#[allow(dead_code)]
pub fn count_at_level(level: Level) -> usize {
    REQUIREMENTS.iter().filter(|r| r.level == level).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for r in REQUIREMENTS {
            assert!(seen.insert(r.id), "duplicate requirement ID: {}", r.id);
        }
    }

    #[test]
    fn ids_follow_naming_convention() {
        for r in REQUIREMENTS {
            let prefix = format!("p{}-", r.principle);
            assert!(
                r.id.starts_with(&prefix),
                "requirement {} does not start with {}",
                r.id,
                prefix
            );
            let level_token = match r.level {
                Level::Must => "-must-",
                Level::Should => "-should-",
                Level::May => "-may-",
            };
            assert!(
                r.id.contains(level_token),
                "requirement {} level token {} missing",
                r.id,
                level_token
            );
        }
    }

    #[test]
    fn principle_range_is_valid() {
        for r in REQUIREMENTS {
            assert!(
                (1..=7).contains(&r.principle),
                "requirement {} has invalid principle {}",
                r.id,
                r.principle
            );
        }
    }

    #[test]
    fn summary_is_non_empty() {
        for r in REQUIREMENTS {
            assert!(
                !r.summary.trim().is_empty(),
                "requirement {} has empty summary",
                r.id
            );
        }
    }

    #[test]
    fn find_returns_registered_ids() {
        assert!(find("p1-must-no-interactive").is_some());
        assert!(find("p6-must-sigpipe").is_some());
        assert!(find("nonexistent-id").is_none());
    }

    #[test]
    fn registry_size_matches_spec() {
        // Spec snapshot 2026-04-20: 46 requirements across P1-P7.
        // Bumping this counter is a deliberate act; it means the spec grew.
        assert_eq!(REQUIREMENTS.len(), 46);
    }

    #[test]
    fn level_counts_match_spec() {
        assert_eq!(count_at_level(Level::Must), 23);
        assert_eq!(count_at_level(Level::Should), 16);
        assert_eq!(count_at_level(Level::May), 7);
    }
}
