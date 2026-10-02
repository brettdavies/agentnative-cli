//! Shared destructive-operation classification for the P5 audits.
//!
//! Two audits need the same list of "destructive" subcommand verbs and the
//! complementary "read" verbs. Centralizing keeps the rubric consistent: if
//! a new verb is added (or a false-positive trimmed), both audits update at
//! once.

use crate::runner::HelpOutput;

/// Subcommand names that imply destructive intent — irreversible writes
/// targeted at the agent-managed resource. Matched case-insensitively at
/// the start of the name or of a `-`/`_` segment, so `delete-key`,
/// `force-delete` and `dropdb` match while `format` and `confirm` do not.
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
    segment_starts(&lower).any(|segment| DESTRUCTIVE_VERBS.iter().any(|v| segment.starts_with(v)))
}

/// Every suffix of `name` that begins at the name's start or immediately
/// after a `-` or `_` delimiter: `reset-keys` yields `reset-keys` and `keys`.
fn segment_starts(name: &str) -> impl Iterator<Item = &str> {
    std::iter::once(name).chain(
        name.match_indices(['-', '_'])
            .map(|(i, delimiter)| &name[i + delimiter.len()..]),
    )
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
    fn segment_prefix_keeps_compound_names_destructive() {
        for name in &[
            "delete-all",
            "dropdb",
            "rmdir",
            "cleanup",
            "purgeall",
            "reset-keys",
            "force-push",
            "remove_all",
        ] {
            assert!(is_destructive(name), "{name} should be destructive");
        }
    }

    #[test]
    fn verb_after_a_delimiter_is_destructive() {
        for name in &[
            "queue-purge",
            "config-reset",
            "session_wipe",
            "db-drop",
            "cache_clean",
        ] {
            assert!(is_destructive(name), "{name} should be destructive");
        }
        assert!(!is_destructive("config-firmware"));
    }

    #[test]
    fn does_not_flag_safe_verbs() {
        for verb in &["list", "get", "show", "create", "add", "init", "build"] {
            assert!(!is_destructive(verb), "{verb} should not be destructive");
        }
    }

    #[test]
    fn verb_inside_a_word_is_not_destructive() {
        for name in &["format", "transform", "perform", "confirm", "firmware"] {
            assert!(!is_destructive(name), "{name} should not be destructive");
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
