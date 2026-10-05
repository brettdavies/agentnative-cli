//! Shared applicability gate for list-style CLI audits.
//!
//! `p7-should-limit` and `p7-may-cursor-pagination` only apply when the
//! target CLI has list-style subcommands. A behavioral-layer heuristic looks
//! for common list verbs in the top-level subcommand surface.
//!
//! Verbs covered: `list`, `ls`, `search`, `query`, `find`, `show`, `get`.
//! A CLI without any of these vacuously skips the audit.

use crate::runner::HelpOutput;

const LIST_VERBS: &[&str] = &["list", "ls", "search", "query", "find", "show", "get"];

/// The top-level subcommands whose name matches a list-style verb
/// (case-insensitive), in help order. Empty when the gate does not fire.
pub(crate) fn list_style_subcommands(help: &HelpOutput) -> Vec<&str> {
    help.subcommands()
        .iter()
        .map(String::as_str)
        .filter(|s| LIST_VERBS.iter().any(|verb| s.eq_ignore_ascii_case(verb)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_list_verb() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list    List items.\n  build   Build.\n",
        );
        assert!(!list_style_subcommands(&help).is_empty());
    }

    #[test]
    fn detects_search_verb() {
        let help =
            HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  search    Search items.\n");
        assert!(!list_style_subcommands(&help).is_empty());
    }

    #[test]
    fn case_insensitive_match() {
        let help =
            HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  LIST    UPPERCASE.\n");
        assert!(!list_style_subcommands(&help).is_empty());
    }

    #[test]
    fn rejects_non_list_verbs() {
        // Sanity check against the verb list — `audit`, `build`, `run` are
        // common non-list verbs that should fail the gate.
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  audit    Run audits.\n  build    Build.\n  run    Run.\n",
        );
        assert!(list_style_subcommands(&help).is_empty());
    }

    #[test]
    fn empty_subcommand_list_returns_false() {
        let help = HelpOutput::from_raw("Usage: tool [OPTIONS]\n\nOptions:\n  -h, --help\n");
        assert!(list_style_subcommands(&help).is_empty());
    }
}
