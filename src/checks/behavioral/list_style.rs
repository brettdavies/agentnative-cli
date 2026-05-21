//! Shared applicability gate for list-style CLI checks.
//!
//! `p7-should-limit` and `p7-may-cursor-pagination` only apply when the
//! target CLI has list-style subcommands. A behavioral-layer heuristic looks
//! for common list verbs in the top-level subcommand surface.
//!
//! Verbs covered: `list`, `ls`, `search`, `query`, `find`, `show`, `get`.
//! A CLI without any of these vacuously skips the check.

use crate::runner::HelpOutput;

const LIST_VERBS: &[&str] = &["list", "ls", "search", "query", "find", "show", "get"];

/// True iff the help text advertises at least one top-level subcommand
/// whose name matches a list-style verb (case-insensitive).
pub(crate) fn has_list_style_subcommand(help: &HelpOutput) -> bool {
    help.subcommands()
        .iter()
        .any(|s| LIST_VERBS.iter().any(|verb| s.eq_ignore_ascii_case(verb)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_list_verb() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list    List items.\n  build   Build.\n",
        );
        assert!(has_list_style_subcommand(&help));
    }

    #[test]
    fn detects_search_verb() {
        let help =
            HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  search    Search items.\n");
        assert!(has_list_style_subcommand(&help));
    }

    #[test]
    fn case_insensitive_match() {
        let help =
            HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  LIST    UPPERCASE.\n");
        assert!(has_list_style_subcommand(&help));
    }

    #[test]
    fn rejects_non_list_verbs() {
        // Sanity check against the verb list — `check`, `build`, `run` are
        // common non-list verbs that should fail the gate.
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  check    Run checks.\n  build    Build.\n  run    Run.\n",
        );
        assert!(!has_list_style_subcommand(&help));
    }

    #[test]
    fn empty_subcommand_list_returns_false() {
        let help = HelpOutput::from_raw("Usage: tool [OPTIONS]\n\nOptions:\n  -h, --help\n");
        assert!(!has_list_style_subcommand(&help));
    }
}
