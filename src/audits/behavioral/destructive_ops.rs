//! Shared destructive-operation classification for the P5 audits.
//!
//! Two audits need the same list of "destructive" subcommand verbs and the
//! complementary "read" verbs. Centralizing keeps the rubric consistent: if
//! a new verb is added (or a false-positive trimmed), both audits update at
//! once.

use crate::runner::HelpOutput;

/// Subcommand names that imply destructive intent — irreversible writes
/// targeted at the agent-managed resource. Case-insensitive substring on
/// the subcommand name (so `delete-key` and `force-delete` both match).
const DESTRUCTIVE_VERBS: &[&str] = &[
    "delete", "remove", "rm", "destroy", "purge", "wipe", "reset", "drop", "clean", "force-",
];

/// Subcommand names that imply read-only operations. Used by the read/write
/// distinction audit to confirm the CLI ships both surfaces. Case-insensitive
/// exact match (we want `list`, not `flush-list`).
const READ_VERBS: &[&str] = &["list", "ls", "get", "show", "query", "find", "search"];

/// Write-pattern verbs that are not strictly destructive. Used to decide
/// whether the CLI has a write surface for the read/write distinction.
const NON_DESTRUCTIVE_WRITE_VERBS: &[&str] = &["create", "add", "update", "set", "put"];

/// Return the destructive subcommand names from the parsed help output.
/// Ordering follows the parser's order, which mirrors the binary's
/// `Commands:` section.
pub(crate) fn destructive_subcommands(help: &HelpOutput) -> Vec<&String> {
    help.subcommands()
        .iter()
        .filter(|s| is_destructive(s))
        .collect()
}

pub(crate) fn is_destructive(name: &str) -> bool {
    let lower = name.to_lowercase();
    DESTRUCTIVE_VERBS.iter().any(|v| lower.contains(v))
}

pub(crate) fn is_read_verb(name: &str) -> bool {
    let lower = name.to_lowercase();
    READ_VERBS.iter().any(|v| lower == *v)
}

pub(crate) fn is_write_verb(name: &str) -> bool {
    if is_destructive(name) {
        return true;
    }
    let lower = name.to_lowercase();
    NON_DESTRUCTIVE_WRITE_VERBS.iter().any(|v| lower == *v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_canonical_destructive_verbs() {
        for verb in &[
            "delete", "remove", "rm", "destroy", "purge", "wipe", "reset", "drop", "clean",
        ] {
            assert!(is_destructive(verb), "{verb} should be destructive");
        }
    }

    #[test]
    fn detects_force_prefix() {
        assert!(is_destructive("force-delete"));
        assert!(is_destructive("force-rm"));
    }

    #[test]
    fn case_insensitive() {
        assert!(is_destructive("Delete"));
        assert!(is_destructive("PURGE"));
    }

    #[test]
    fn substring_match_for_compound_names() {
        // `delete-all` should match — substring is correct here because
        // the destructive intent of `delete` carries over.
        assert!(is_destructive("delete-all"));
        assert!(is_destructive("dropdb"));
    }

    #[test]
    fn does_not_flag_safe_verbs() {
        for verb in &["list", "get", "show", "create", "add", "init", "build"] {
            assert!(!is_destructive(verb), "{verb} should not be destructive");
        }
    }

    #[test]
    fn read_verbs_match_exactly() {
        assert!(is_read_verb("list"));
        assert!(is_read_verb("LS"));
        assert!(!is_read_verb("listen")); // partial match must not count
    }

    #[test]
    fn write_verbs_include_destructive_and_constructive() {
        assert!(is_write_verb("delete"));
        assert!(is_write_verb("create"));
        assert!(is_write_verb("set"));
        assert!(!is_write_verb("list"));
        assert!(!is_write_verb("show"));
    }

    #[test]
    fn destructive_subcommands_returns_filtered_list() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\n\
             Commands:\n  list    List items.\n  delete  Delete item.\n  purge   Purge cache.\n  add    Add item.\n",
        );
        let destructive = destructive_subcommands(&help);
        assert_eq!(destructive.len(), 2);
        assert_eq!(destructive[0], "delete");
        assert_eq!(destructive[1], "purge");
    }
}
