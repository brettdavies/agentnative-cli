//! The flags that select a tool's output format, read from the option
//! definitions its help declares. `p2-json-output` and the JSON error
//! audits share these lookups, so a scorecard answers "does this tool have a
//! JSON mode" one way.

use crate::audits::behavioral::subcommand_help::{probe_help, should_skip};
use crate::runner::help_probe::Flag;
use crate::runner::{BinaryRunner, HelpOutput};
use crate::types::AuditStatus;

/// The flag that takes a format value by name: `--output json`.
pub(crate) const VALUE_FLAGS: &[&str] = &["--output"];

/// Format flags under other names, which count when their own definition
/// names `json`: shellcheck's `--format` lists it, fd's takes a template;
/// claude-code's `--output-format` selects JSON, dust's picks size units.
pub(crate) const NAMED_VALUE_FLAGS: &[&str] = &["--format", "--output-format"];

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

/// The accepted names and the test the named ones meet, as evidence prose.
pub(crate) fn accepted_prose() -> String {
    format!(
        "{}, a {} whose definition names json, {}",
        VALUE_FLAGS.join(", "),
        NAMED_VALUE_FLAGS.join(" or "),
        JSON_ALIASES.join(" or "),
    )
}

/// When `help` declares a flag from [`NAMED_VALUE_FLAGS`] whose definition
/// does not name `json`: that flag, and why it does not count.
fn unnamed_json_note(help: &HelpOutput) -> Option<String> {
    NAMED_VALUE_FLAGS
        .iter()
        .filter_map(|flag| help.find_flag(&[flag]))
        .find(|found| !names_json(found.flag))
        .map(|found| {
            format!(
                "`{}` is declared, but its definition does not name json, and P2 counts it as an \
                 output flag only when it does.",
                found.spelling
            )
        })
}

/// Why a flag `help` declares under an accepted name did not count, each
/// reason that applies, joined by a space.
pub(crate) fn near_miss_note(help: &HelpOutput) -> Option<String> {
    let notes: Vec<String> = [help.dash_rule_note(&accepted()), unnamed_json_note(help)]
        .into_iter()
        .flatten()
        .collect();
    (!notes.is_empty()).then(|| notes.join(" "))
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

/// A JSON mode a tool's help declares, and where a probe selects it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct JsonMode {
    /// The subcommand whose help declares the mode, when the top-level help
    /// declares none.
    pub(crate) subcommand: Option<String>,
    /// The arguments that select JSON: `--output json`, or the alias alone.
    pub(crate) selector: Vec<String>,
}

impl JsonMode {
    /// `args` under this mode, the subcommand first and the selector last:
    /// `list --help --output json`.
    pub(crate) fn args_with<'a>(&'a self, args: &[&'a str]) -> Vec<&'a str> {
        self.subcommand
            .iter()
            .map(String::as_str)
            .chain(args.iter().copied())
            .chain(self.selector.iter().map(String::as_str))
            .collect()
    }

    /// The mode as evidence quotes it: `list --output json`.
    pub(crate) fn shown(&self) -> String {
        self.args_with(&[]).join(" ")
    }

    /// `status`, saying where the mode came from when a subcommand declares
    /// it.
    pub(crate) fn noted(&self, status: AuditStatus) -> AuditStatus {
        match &self.subcommand {
            Some(name) => status.with_note(&format!(
                "`{name} --help` declares this JSON mode, and the top-level --help declares \
                 none. P2 counts any flag that selects JSON as a JSON output mode, and this \
                 requirement applies under it."
            )),
            None => status,
        }
    }
}

/// The JSON mode a probe selects: the one the top-level help declares, else
/// the first top-level subcommand's. Reads each subcommand's help through the
/// runner cache, so it shares those spawns with `p2-json-output`.
pub(crate) fn json_mode(runner: &BinaryRunner, help: &HelpOutput) -> Option<JsonMode> {
    if let Some(selector) = json_selector(help) {
        return Some(JsonMode {
            subcommand: None,
            selector,
        });
    }
    help.subcommands()
        .iter()
        .filter(|name| !should_skip(name))
        .find_map(|name| {
            let selector = json_selector(&probe_help(runner, &[name])?)?;
            Some(JsonMode {
                subcommand: Some(name.clone()),
                selector,
            })
        })
}

/// The arguments that select JSON under `help`: a value flag whose own
/// definition names `json` (`--output <FMT>  text or json`), else a JSON
/// alias.
fn json_selector(help: &HelpOutput) -> Option<Vec<String>> {
    VALUE_FLAGS
        .iter()
        .chain(NAMED_VALUE_FLAGS)
        .filter_map(|flag| help.find_flag(&[flag]))
        .find(|found| names_json(found.flag))
        .map(|found| vec![found.spelling.to_string(), "json".to_string()])
        .or_else(|| {
            declared(help)
                .aliases
                .into_iter()
                .next()
                .map(|alias| vec![alias])
        })
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
            "cobra's `--format string` that names json",
            "Flags:\n      --format string   Output format: text or json (default \"text\")\n  -h, --help            help for tool\n",
            &["--format"],
            &[],
        ),
        (
            "a --format that names no json, the flag's name alone",
            "Flags:\n      --format string   Output format (default \"text\")\n  -h, --help            help for tool\n",
            &[],
            &[],
        ),
        (
            "a --format that takes a Go template",
            "Options:\n      --format <TEMPLATE>    Go template for output\n  -h, --help             Show help\n",
            &[],
            &[],
        ),
        (
            "both value flags",
            "Options:\n      --output <FILE>      Write to FILE\n      --format <FORMAT>    Output format (text, json)\n",
            &["--output", "--format"],
            &[],
        ),
        (
            "clap's long help, with the values in a block below the definition",
            "Options:\n      --format <FORMAT>\n          Output format\n\n          Possible values:\n          - text: For people\n          - json: For programs\n\n  -h, --help\n          Show help\n",
            &["--format"],
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
            "--output, a --format or --output-format whose definition names json, --json or --jsonl"
        );
    }

    /// A help, and the JSON mode a probe of the whole tool selects.
    const JSON_MODES: &[(&str, &str, Option<&[&str]>)] = &[
        (
            "an output flag whose values name json",
            "Options:\n  -o, --output <FORMAT>    Output format [possible values: text, json]\n",
            Some(&["--output", "json"]),
        ),
        (
            "a --format whose values name json",
            "Options:\n      --format <F>  Output format (text, json)\n",
            Some(&["--format", "json"]),
        ),
        (
            "yq's --output-format",
            "Flags:\n  -o, --output-format string   [auto|a|yaml|y|json|j|props|p|xml|x] output format type. (default \"auto\")\n",
            Some(&["--output-format", "json"]),
        ),
        (
            "stream-json names json",
            "Options:\n  --output-format <format>  Output format: \"text\", \"json\", or \"stream-json\"\n",
            Some(&["--output-format", "json"]),
        ),
        (
            "an output flag that writes a file",
            "Options:\n  -o, --output <file>    Write to file instead of stdout\n      --json <data>      HTTP POST JSON\n",
            None,
        ),
        (
            "a Go template format flag",
            "Usage of tool:\n  -format string\n    \tCustom template to format error messages in Go template syntax\n",
            None,
        ),
        (
            "a boolean alias",
            "Options:\n      --output <FILE>    Write to FILE\n      --json             Print JSON\n",
            Some(&["--json"]),
        ),
        (
            "json named only in a sentence",
            "Pass --output json for machine output.\n\nOptions:\n  -h, --help    Show help\n",
            None,
        ),
    ];

    #[test]
    fn json_mode_is_a_declared_flag_whose_definition_names_json() {
        let misread: Vec<String> = JSON_MODES
            .iter()
            .filter_map(|(what, help, expected)| {
                let found = json_selector(&HelpOutput::from_raw(*help));
                let expected: Option<Vec<String>> =
                    expected.map(|args| args.iter().map(|arg| (*arg).to_string()).collect());
                (found != expected)
                    .then(|| format!("{what}: read {found:?}, expected {expected:?}"))
            })
            .collect();
        assert!(misread.is_empty(), "{misread:#?}");
    }
}
