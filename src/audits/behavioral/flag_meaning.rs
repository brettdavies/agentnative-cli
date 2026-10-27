//! Whether a single-letter flag means what an audit asks for.
//!
//! One letter names different flags in different tools: `-v` is
//! `--verbose` in curl and `--version` in claude, `-q` is `--quiet` in fd
//! and `--disable` in curl. A single letter an audit accepts counts only
//! when its own definition means what the audit asks for: the definition
//! also declares a long name the audit accepts, or its description names
//! the meaning. A name of two or more characters (`-vv`, `--force`) counts
//! as declared.

use std::borrow::Cow;

use crate::runner::HelpOutput;
use crate::runner::help_probe::{Flag, FlagMatch, is_single_letter};

/// What an audit asks of a single letter it accepts.
pub(crate) struct Meaning {
    /// What the audit asks for, as a deny names it: "`-v` ... does not mean
    /// {what}".
    pub what: &'static str,
    /// Long names beyond the audit's own that give a letter on the same
    /// definition this meaning (`-n, --lines` for a limit).
    pub long_names: &'static [&'static str],
    /// Words and phrases that name the meaning in a description, matched
    /// as whole words and without case.
    pub words: &'static [&'static str],
}

impl Meaning {
    /// Whether `flag`'s definition means this, given the audit's accepted
    /// `names`.
    fn is_meant_by(&self, flag: &Flag, names: &[&str]) -> bool {
        names
            .iter()
            .chain(self.long_names)
            .any(|name| !is_single_letter(name) && flag.declares(name).is_some())
            || names_any(flag.description(), self.words)
    }
}

/// The spelling `flag` counts by for an audit that accepts `names`: the
/// first of `names` it declares, unless that is a single letter its
/// definition does not give `meaning`.
pub(crate) fn accepts<'f>(flag: &'f Flag, names: &[&str], meaning: &Meaning) -> Option<&'f str> {
    let spelling = flag.declares_any(names)?;
    (!is_single_letter(spelling) || meaning.is_meant_by(flag, names)).then_some(spelling)
}

/// The first flag, in help order, that counts for an audit that accepts
/// `names`.
pub(crate) fn find<'h>(
    help: &'h HelpOutput,
    names: &[&str],
    meaning: &Meaning,
) -> Option<FlagMatch<'h>> {
    help.flags()
        .iter()
        .find_map(|flag| accepts(flag, names, meaning).map(|spelling| FlagMatch { flag, spelling }))
}

/// When `help` declares a single letter from `names` whose definition does
/// not give `meaning`: the letter, its definition, and that it does not
/// count.
pub(crate) fn rejected_note(
    help: &HelpOutput,
    names: &[&str],
    meaning: &Meaning,
) -> Option<String> {
    help.flags().iter().find_map(|flag| {
        let letter = names
            .iter()
            .filter(|name| is_single_letter(name))
            .find_map(|name| flag.declares(name))?;
        (!meaning.is_meant_by(flag, names)).then(|| {
            format!(
                "`{letter}` is declared, but its definition (`{}`) does not mean {}.",
                first_sentence(&flag.definition()),
                meaning.what
            )
        })
    })
}

/// [`HelpOutput::dash_rule_note`] and [`rejected_note`], each that applies,
/// joined by a space.
pub(crate) fn notes(help: &HelpOutput, names: &[&str], meaning: &Meaning) -> Option<String> {
    let notes: Vec<String> = [
        help.dash_rule_note(names),
        rejected_note(help, names, meaning),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!notes.is_empty()).then(|| notes.join(" "))
}

/// `message`, closed with [`notes`] when any apply.
pub(crate) fn noting(
    help: &HelpOutput,
    names: &[&str],
    meaning: &Meaning,
    message: &str,
) -> String {
    match notes(help, names, meaning) {
        Some(notes) => format!("{message} {notes}"),
        None => message.to_string(),
    }
}

/// `definition` up to the end of its first sentence, marked ` …` where
/// more follows, so a deny quotes a long description by its opening.
fn first_sentence(definition: &str) -> Cow<'_, str> {
    match definition.find(". ") {
        Some(end) => Cow::Owned(format!("{} …", &definition[..=end])),
        None => Cow::Borrowed(definition),
    }
}

/// Whether `text` holds any of `phrases` as whole words, without case.
fn names_any(text: &str, phrases: &[&str]) -> bool {
    let said = words(text);
    phrases.iter().any(|phrase| {
        let wanted = words(phrase);
        !wanted.is_empty()
            && said
                .windows(wanted.len())
                .any(|run| run == wanted.as_slice())
    })
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: &[&str] = &["--verbose", "-v", "-vv"];

    const MEANING: Meaning = Meaning {
        what: "verbose",
        long_names: &["--loud"],
        words: &["verbosity", "log level"],
    };

    fn found(help: &str) -> Option<String> {
        let help = HelpOutput::from_raw(help);
        find(&help, NAMES, &MEANING).map(|hit| hit.spelling.to_string())
    }

    fn note(help: &str) -> Option<String> {
        rejected_note(&HelpOutput::from_raw(help), NAMES, &MEANING)
    }

    #[test]
    fn a_letter_beside_an_accepted_long_name_counts() {
        assert_eq!(
            found("Options:\n  -v, --verbose    Talk more.\n").as_deref(),
            Some("--verbose")
        );
        assert_eq!(
            found("Options:\n  -v, --loud    Talk more.\n").as_deref(),
            Some("-v")
        );
    }

    #[test]
    fn a_letter_whose_description_names_the_meaning_counts() {
        assert_eq!(
            found("Options:\n  -v    Increase verbosity.\n").as_deref(),
            Some("-v")
        );
        assert_eq!(
            found("Options:\n  -v <N>    Set the log level.\n").as_deref(),
            Some("-v")
        );
    }

    #[test]
    fn a_meaning_word_counts_only_whole() {
        assert_eq!(
            found("Options:\n  -v    Show verbosityless output.\n"),
            None
        );
        assert_eq!(
            found("Options:\n  -v    Set the log-level.\n").as_deref(),
            Some("-v")
        );
    }

    #[test]
    fn a_letter_whose_definition_means_something_else_does_not_count() {
        let help = "Options:\n  -v, --version     Print version.\n  -h, --help        Show help.\n";
        assert_eq!(found(help), None);
        assert_eq!(
            note(help).as_deref(),
            Some(
                "`-v` is declared, but its definition (`-v, --version  Print version.`) does not mean verbose."
            )
        );
    }

    #[test]
    fn a_lone_letter_with_no_description_does_not_count() {
        let help = "Options:\n  -v\n  -h, --help    Show help.\n";
        assert_eq!(found(help), None);
        assert_eq!(
            note(help).as_deref(),
            Some("`-v` is declared, but its definition (`-v`) does not mean verbose.")
        );
    }

    #[test]
    fn a_longer_short_name_counts_as_declared() {
        assert_eq!(
            found("Options:\n  -vv    Print version.\n").as_deref(),
            Some("-vv")
        );
    }

    #[test]
    fn a_later_definition_that_counts_wins_over_an_earlier_letter() {
        let help = "Options:\n  -v, --version    Print version.\n      --verbose    Talk more.\n";
        assert_eq!(found(help).as_deref(), Some("--verbose"));
    }

    #[test]
    fn a_note_quotes_a_long_description_by_its_first_sentence() {
        // rg 15.1's `-v`.
        let help = "OPTIONS:\n    -v, --invert-match\n        This flag inverts matching. That is, instead of printing lines that\n        match, ripgrep will print lines that don't match.\n";
        assert_eq!(
            note(help).as_deref(),
            Some(
                "`-v` is declared, but its definition (`-v, --invert-match  This flag inverts \
                 matching. …`) does not mean verbose."
            )
        );
    }

    #[test]
    fn the_definition_collapses_column_gaps() {
        let help = HelpOutput::from_raw(
            "Flags:\n    -v    --version            Print the current version\n",
        );
        assert_eq!(
            help.flags()[0].definition(),
            "-v --version  Print the current version"
        );
    }
}
