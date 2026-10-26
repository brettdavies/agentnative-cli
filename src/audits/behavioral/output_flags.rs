//! The flags that select a tool's output format, read from the option
//! definitions its help declares. `p2-json-output` and the JSON error
//! audits share these lookups, so a scorecard answers "does this tool have a
//! JSON mode" one way.

use crate::runner::HelpOutput;
use crate::runner::help_probe::Flag;

/// Flags that take a format value: `--output json`.
pub(crate) const VALUE_FLAGS: &[&str] = &["--output", "--format"];

/// Longer format flags, which count when their own definition names `json`:
/// claude-code's `--output-format` selects JSON, dust's picks size units.
pub(crate) const NAMED_VALUE_FLAGS: &[&str] = &["--output-format"];

/// The boolean aliases for `--output json` and `--output jsonl` that the
/// spec accepts for `p2-must-output-flag`. A `--json` that takes a value
/// (curl's request body, gh's field list) is a different flag.
pub(crate) const JSON_ALIASES: &[&str] = &["--json", "--jsonl"];

/// Every name `p2-must-output-flag` accepts, value flags first.
pub(crate) fn accepted() -> Vec<&'static str> {
    VALUE_FLAGS
        .iter()
        .chain(NAMED_VALUE_FLAGS)
        .chain(JSON_ALIASES)
        .copied()
        .collect()
}

/// The accepted names as evidence prose: `--output, --format, ... or --jsonl`.
pub(crate) fn accepted_prose() -> String {
    match accepted().split_last() {
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The output flags a help declares, spelled as the help prints them.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Declared {
    /// Each of [`VALUE_FLAGS`] the help declares, and each of
    /// [`NAMED_VALUE_FLAGS`] whose definition names `json`: `-format` for a
    /// Go `flag` help that declares no double-dash name.
    pub(crate) value_flags: Vec<String>,
    /// Each of [`JSON_ALIASES`] the help declares without a value.
    pub(crate) aliases: Vec<String>,
}

impl Declared {
    pub(crate) fn is_empty(&self) -> bool {
        self.value_flags.is_empty() && self.aliases.is_empty()
    }

    /// Every declared name, value flags first.
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.value_flags
            .iter()
            .chain(&self.aliases)
            .map(String::as_str)
    }

    /// The arguments that select JSON through each declared flag:
    /// `--output json` and `--output=json` for a value flag, the alias alone.
    pub(crate) fn json_selectors(&self) -> Vec<Vec<String>> {
        let spaced = self
            .value_flags
            .iter()
            .map(|flag| vec![flag.clone(), "json".into()]);
        let joined = self
            .value_flags
            .iter()
            .map(|flag| vec![format!("{flag}=json")]);
        let aliases = self.aliases.iter().map(|alias| vec![alias.clone()]);
        spaced.chain(joined).chain(aliases).collect()
    }
}

/// The output flags `help` declares.
pub(crate) fn declared(help: &HelpOutput) -> Declared {
    let named = NAMED_VALUE_FLAGS
        .iter()
        .filter_map(|flag| help.find_flag(&[flag]))
        .filter(|found| names_json(found.flag));
    Declared {
        value_flags: VALUE_FLAGS
            .iter()
            .filter_map(|flag| help.find_flag(&[flag]))
            .chain(named)
            .map(|found| found.spelling.to_string())
            .collect(),
        aliases: JSON_ALIASES
            .iter()
            .filter_map(|alias| help.find_flag(&[alias]))
            .filter(|found| found.flag.placeholder().is_none())
            .map(|found| found.spelling.to_string())
            .collect(),
    }
}

/// Whether a definition's value or description names `json` or `jsonl` as a
/// word: `stream-json` does, `jsonnet` does not.
fn names_json(flag: &Flag) -> bool {
    [flag.placeholder().unwrap_or_default(), flag.description()]
        .iter()
        .flat_map(|text| text.split(|c: char| !c.is_ascii_alphanumeric()))
        .any(|word| word.eq_ignore_ascii_case("json") || word.eq_ignore_ascii_case("jsonl"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A help, and the value flags and aliases it declares as a probe must
    /// spell them.
    const DECLARED: &[(&str, &str, &[&str], &[&str])] = &[
        (
            "a long name with a short alias",
            "Options:\n  -o, --output <FORMAT>    Output format: text or json\n  -h, --help               Show help\n",
            &["--output"],
            &[],
        ),
        (
            "cobra's `--format string`",
            "Flags:\n      --format string   Output format (default \"text\")\n  -h, --help            help for tool\n",
            &["--format"],
            &[],
        ),
        (
            "both value flags",
            "Options:\n      --output <FILE>      Write to FILE\n      --format <FORMAT>    Output format\n",
            &["--output", "--format"],
            &[],
        ),
        (
            "Go flag: one dash, and no double-dash name in the help",
            "Usage of tool:\n  -format string\n    \tOutput format: text or json (default \"text\")\n  -version\n    \tPrint version\n",
            &["-format"],
            &[],
        ),
        (
            "--output-format that names json, and longer names that are not output flags",
            "Options:\n      --output-format <FORMAT>    Output format: text or json\n      --output-dir <DIR>          Where to write\n      --format-version <N>        Schema version\n  -h, --help                      Show help\n",
            &["--output-format"],
            &[],
        ),
        (
            "dust's --output-format, which picks size units",
            "Options:\n  -o, --output-format <FORMAT>  Changes output display size. si will print sizes in powers of 1000. b k m g t kb mb gb tb\n  -j, --output-json             Output the directory tree as json\n",
            &[],
            &[],
        ),
        (
            "boolean JSON aliases",
            "Options:\n      --json     Print JSON\n      --jsonl    Print JSON Lines\n  -h, --help     Show help\n",
            &[],
            &["--json", "--jsonl"],
        ),
        (
            "a --json that takes a value",
            "Options:\n      --json <data>    HTTP POST JSON\n      --json fields    Output JSON with the specified fields\n",
            &[],
            &[],
        ),
        (
            "a usage line",
            "Usage: tool [--output FORMAT] [--json] <file>\n\nOptions:\n  -h, --help    Show help\n",
            &[],
            &[],
        ),
        (
            "a sentence",
            "Pass --format json for machine-readable output.\n\nOptions:\n  -h, --help    Show help\n",
            &[],
            &[],
        ),
        (
            "one dash beside double-dash names",
            "Options:\n  -format <FORMAT>    Output format\n      --help          Show help\n",
            &[],
            &[],
        ),
    ];

    #[test]
    fn output_flags_are_the_ones_the_help_declares_spelled_as_printed() {
        let misread: Vec<String> = DECLARED
            .iter()
            .filter_map(|(what, help, value_flags, aliases)| {
                let found = declared(&HelpOutput::from_raw(*help));
                let expected = Declared {
                    value_flags: value_flags.iter().map(|f| (*f).to_string()).collect(),
                    aliases: aliases.iter().map(|f| (*f).to_string()).collect(),
                };
                (found != expected)
                    .then(|| format!("{what}: read {found:?}, declares {expected:?}"))
            })
            .collect();
        assert!(misread.is_empty(), "{misread:#?}");
    }

    #[test]
    fn json_selectors_pass_a_value_flag_json_and_an_alias_alone() {
        let declared = Declared {
            value_flags: vec!["-format".into()],
            aliases: vec!["--json".into()],
        };
        assert_eq!(
            declared.json_selectors(),
            vec![
                vec!["-format".to_string(), "json".to_string()],
                vec!["-format=json".to_string()],
                vec!["--json".to_string()],
            ]
        );
    }

    #[test]
    fn accepted_prose_lists_every_name() {
        assert_eq!(
            accepted_prose(),
            "--output, --format, --output-format, --json or --jsonl"
        );
    }
}
