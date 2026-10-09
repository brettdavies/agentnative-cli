//! Flag definitions read from `--help`, and the lookup audits ask them
//! through.
//!
//! A definition line declares one flag under one or more names. Each name is
//! kept as the help spells it, and a lookup compares whole names: `-f`
//! answers only `-f`, and a word answers the same word with the same dash
//! count. The one exception is the Go `flag` convention, where one dash and
//! two name the same flag: in a help that declares no double-dash name, a
//! single-dash word also answers its double-dash spelling.

mod classify;
mod header;
mod normalize;
mod pieces;

use classify::Line;
pub(super) use classify::is_section_heading;

/// How a help spells one flag name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameForm {
    /// `-x`: one dash, one character.
    Letter,
    /// `-word`: one dash, several characters.
    DashWord,
    /// `--word`.
    DoubleDash,
    /// `+x`.
    Plus,
    /// `/x`.
    Slash,
}

/// One name a definition line declares.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FlagName {
    spelling: String,
    form: NameForm,
    /// A single-dash word in a help that declares no double-dash name.
    stands_for_double_dash: bool,
}

impl FlagName {
    fn new(spelling: impl Into<String>) -> Self {
        let spelling = spelling.into();
        let form = if spelling.starts_with("--") {
            NameForm::DoubleDash
        } else if spelling.starts_with('+') {
            NameForm::Plus
        } else if spelling.starts_with('/') {
            NameForm::Slash
        } else if spelling.chars().count() == 2 {
            NameForm::Letter
        } else {
            NameForm::DashWord
        };
        Self {
            spelling,
            form,
            stands_for_double_dash: false,
        }
    }

    /// Whether a lookup for `wanted` reaches this name.
    fn answers(&self, wanted: &str) -> bool {
        self.spelling == wanted
            || (self.stands_for_double_dash && wanted.strip_prefix("--") == Some(self.word()))
    }

    /// A double-dash name, or a single-dash word that stands for one.
    fn is_long(&self) -> bool {
        self.form == NameForm::DoubleDash || self.stands_for_double_dash
    }

    fn word(&self) -> &str {
        self.spelling.trim_start_matches(['-', '+', '/'])
    }
}

/// One flag a help declares: the names on its definition line in the order
/// read, and what the help says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    names: Vec<FlagName>,
    /// The first value placeholder on the definition line, as written
    /// (`<FILE>`, `=false`, `N`).
    placeholder: Option<String>,
    description: String,
    /// 1-based line of the definition in the help text.
    line: usize,
    /// The section heading the definition sits under, as written.
    section: Option<String>,
}

impl Flag {
    /// The spelling this flag declares for `name`, when it declares it.
    pub fn declares(&self, name: &str) -> Option<&str> {
        self.names
            .iter()
            .find(|declared| declared.answers(name))
            .map(|declared| declared.spelling.as_str())
    }

    /// The spelling this flag declares for the first of `names` it declares.
    pub fn declares_any(&self, names: &[&str]) -> Option<&str> {
        names.iter().find_map(|name| self.declares(name))
    }

    /// The flag's long names as the help spells them, in the order read: its
    /// double-dash names, and its single-dash words where those stand for
    /// double-dash names.
    pub fn long_names(&self) -> impl Iterator<Item = &str> {
        self.names
            .iter()
            .filter(|name| name.is_long())
            .map(|name| name.spelling.as_str())
    }

    /// The name that identifies this flag in evidence and across helps: its
    /// first long name, else its first name.
    pub fn name(&self) -> &str {
        self.long_names()
            .next()
            .or_else(|| self.names.first().map(|name| name.spelling.as_str()))
            .unwrap_or_default()
    }

    /// What the help says about the flag: the text after the header on its
    /// own line, joined with the wrapped and next-line text under it.
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// A lookup hit: the flag, and the name it answered with as the help spells
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlagMatch<'a> {
    pub flag: &'a Flag,
    pub spelling: &'a str,
}

/// The first flag, in help order, that declares any of `names`.
pub(super) fn find<'a>(flags: &'a [Flag], names: &[&str]) -> Option<FlagMatch<'a>> {
    flags.iter().find_map(|flag| {
        flag.declares_any(names)
            .map(|spelling| FlagMatch { flag, spelling })
    })
}

/// The declared single-dash word that would answer one of `names` in a help
/// with no double-dash names, and the name it is kept apart from.
pub(super) fn kept_apart<'a>(flags: &'a [Flag], names: &[&'a str]) -> Option<(&'a str, &'a str)> {
    flags
        .iter()
        .flat_map(|flag| &flag.names)
        .filter(|name| name.form == NameForm::DashWord && !name.stands_for_double_dash)
        .find_map(|name| {
            names
                .iter()
                .find(|wanted| wanted.strip_prefix("--") == Some(name.word()))
                .map(|wanted| (name.spelling.as_str(), *wanted))
        })
}

/// Read every flag definition in `raw`.
pub(super) fn parse(raw: &str) -> Vec<Flag> {
    let mut flags: Vec<Flag> = Vec::new();
    let mut section: Option<String> = None;
    let text = normalize::normalize(raw);
    for (index, line) in classify::lines(&text).into_iter().enumerate() {
        match line {
            Line::Heading(heading) => section = Some(heading.to_string()),
            Line::Definition(header) => flags.push(Flag {
                names: header.names,
                placeholder: header.placeholder.map(str::to_string),
                description: header.description.to_string(),
                line: index + 1,
                section: section.clone(),
            }),
            Line::Continuation(text) => {
                if let Some(flag) = flags.last_mut() {
                    if !flag.description.is_empty() {
                        flag.description.push(' ');
                    }
                    flag.description.push_str(text);
                }
            }
            Line::Unnamed | Line::Other => {}
        }
    }
    resolve_dash_forms(&mut flags);
    flags
}

/// The index of every line of `raw` shaped like a definition, whether or not
/// it declares a name.
pub(super) fn definition_lines(raw: &str) -> Vec<usize> {
    classify::lines(&normalize::normalize(raw))
        .iter()
        .enumerate()
        .filter(|(_, line)| matches!(line, Line::Definition(_) | Line::Unnamed))
        .map(|(index, _)| index)
        .collect()
}

/// In a help that declares no double-dash name, every single-dash word
/// stands for its double-dash spelling.
fn resolve_dash_forms(flags: &mut [Flag]) {
    let declares_double_dash = flags
        .iter()
        .flat_map(|flag| &flag.names)
        .any(|name| name.form == NameForm::DoubleDash);
    if declares_double_dash {
        return;
    }
    for name in flags.iter_mut().flat_map(|flag| &mut flag.names) {
        name.stands_for_double_dash = name.form == NameForm::DashWord;
    }
}

#[cfg(test)]
impl Flag {
    /// `{line:>4} | {names} | {description}`, the description cut to 60
    /// characters: the line a fixture snapshot holds for this definition.
    pub(super) fn snapshot_line(&self) -> String {
        let names: Vec<&str> = self
            .names
            .iter()
            .map(|name| name.spelling.as_str())
            .collect();
        let description: String = self.description.chars().take(60).collect();
        format!("{:>4} | {} | {}", self.line, names.join(", "), description)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::help_probe::fixture_snapshots::fixture;

    /// A help's flags built from name lists, one flag per list, with the
    /// dash forms resolved as `parse` resolves them.
    fn help_declaring(flags: &[&[&str]]) -> Vec<Flag> {
        let mut built: Vec<Flag> = flags
            .iter()
            .enumerate()
            .map(|(index, names)| Flag {
                names: names.iter().map(|name| FlagName::new(*name)).collect(),
                placeholder: None,
                description: String::new(),
                line: index + 1,
                section: None,
            })
            .collect();
        resolve_dash_forms(&mut built);
        built
    }

    fn found<'a>(flags: &'a [Flag], name: &str) -> Option<&'a str> {
        find(flags, &[name]).map(|hit| hit.spelling)
    }

    #[test]
    fn a_name_answers_its_own_spelling() {
        let flags = help_declaring(&[&["-f", "--force"]]);
        assert_eq!(found(&flags, "-f"), Some("-f"));
        assert_eq!(found(&flags, "--force"), Some("--force"));
        assert_eq!(found(&flags, "--verbose"), None);
    }

    #[test]
    fn a_single_letter_answers_only_the_same_letter() {
        let flags = help_declaring(&[&["-force-copy"], &["-format"], &["-ffoo"]]);
        assert_eq!(found(&flags, "-f"), None);
    }

    #[test]
    fn a_single_dash_word_answers_its_double_dash_spelling_without_double_dash_names() {
        let flags = help_declaring(&[&["-force"], &["-h"]]);
        assert_eq!(found(&flags, "--force"), Some("-force"));
    }

    #[test]
    fn a_single_dash_word_is_its_own_name_beside_a_double_dash_name() {
        let flags = help_declaring(&[&["-force"], &["--help"]]);
        assert_eq!(found(&flags, "--force"), None);
        assert_eq!(found(&flags, "-force"), Some("-force"));
    }

    #[test]
    fn a_double_dash_name_does_not_answer_a_single_dash_lookup() {
        let flags = help_declaring(&[&["--force"]]);
        assert_eq!(found(&flags, "-force"), None);
    }

    #[test]
    fn letters_plus_and_slash_names_never_take_part_in_dash_equivalence() {
        let flags = help_declaring(&[&["+print"], &["/print"], &["-p"]]);
        assert_eq!(found(&flags, "--print"), None);
        assert_eq!(found(&flags, "--p"), None);
        assert_eq!(found(&flags, "+print"), Some("+print"));
        assert_eq!(found(&flags, "/print"), Some("/print"));
    }

    #[test]
    fn the_first_flag_in_help_order_wins() {
        let flags = help_declaring(&[&["--max"], &["--limit"]]);
        let hit = find(&flags, &["--limit", "--max"]).expect("a limit flag");
        assert_eq!(hit.spelling, "--max");
        assert_eq!(hit.flag.line, 1);
    }

    #[test]
    fn a_flag_is_named_by_its_first_long_name_else_its_first_name() {
        let flags = help_declaring(&[&["-n", "--lines"], &["-q"], &["-chdir"], &["--help"]]);
        assert_eq!(flags[0].name(), "--lines");
        assert_eq!(flags[1].name(), "-q");
        assert_eq!(flags[2].name(), "-chdir");
        assert_eq!(flags[2].long_names().count(), 0);
    }

    #[test]
    fn a_single_dash_word_is_a_long_name_without_double_dash_names() {
        let flags = help_declaring(&[&["-chdir"], &["-h"]]);
        let long: Vec<&str> = flags[0].long_names().collect();
        assert_eq!(long, ["-chdir"]);
        assert_eq!(flags[1].long_names().count(), 0);
    }

    // Definition lines from terraform 1.16.4 (`--help`, `init --help`),
    // lazygit 0.65.1 and cmake 4.4.4, each with the letter an audit asks for
    // that its single-dash word starts with.
    const FIRST_LETTER_TRAPS: &[(&str, &str)] = &[
        (
            "  -force-copy             Suppress prompts about copying state data when",
            "-f",
        ),
        (
            "  -no-color               If specified, output won't contain any color.",
            "-n",
        ),
        (
            "  -version      An alias for the \"version\" subcommand.",
            "-v",
        ),
        (
            "    -cd   --print-config-dir   Print the config directory",
            "-c",
        ),
        (
            "  -LR[A][H] <regex>            = Show cached variables that match the regex.",
            "-L",
        ),
        (
            "  -Werror=<category>           = Make the specified category of warnings",
            "-W",
        ),
    ];

    #[test]
    fn a_single_dash_word_does_not_answer_its_first_letter() {
        let answered: Vec<String> = FIRST_LETTER_TRAPS
            .iter()
            .filter(|(line, letter)| found(&parse(line), letter).is_some())
            .map(|(line, letter)| format!("{letter} answered by `{}`", line.trim()))
            .collect();
        assert!(answered.is_empty(), "{answered:#?}");
    }

    #[test]
    fn a_single_dash_word_answers_its_whole_name() {
        let whole = [
            "-force-copy",
            "-no-color",
            "-version",
            "-cd",
            "-LR",
            "-Werror",
        ];
        let unanswered: Vec<String> = FIRST_LETTER_TRAPS
            .iter()
            .zip(whole)
            .filter(|((line, _), name)| found(&parse(line), name) != Some(*name))
            .map(|((line, _), name)| format!("{name} not answered by `{}`", line.trim()))
            .collect();
        assert!(unanswered.is_empty(), "{unanswered:#?}");
    }

    // Excerpt of terraform 1.16.4's `terraform init --help`.
    const TERRAFORM_INIT_HELP: &str = "\
Usage: terraform [global options] init [options]

Options:

  -backend=false          Disable backend or HCP Terraform initialization
                          for this configuration and use what was previously
                          initialized instead.

  -force-copy             Suppress prompts about copying state data when
                          initializating a new state backend. This is
                          equivalent to providing a \"yes\" to all confirmation
                          prompts.

  -no-color               If specified, output won't contain any color.

  -var 'foo=bar'          Set a value for one of the input variables in the root
                          module of the configuration. Use this option more than
                          once to set more than one variable.
";

    // Excerpt of GNU findutils 4.10.0's `find --help`.
    const FIND_HELP: &str = "\
Actions:
      -delete -print0 -printf FORMAT -fprintf FILE FORMAT -print 
      -fprint0 FILE -fprint FILE -ls -fls FILE -prune -quit

Other common options:
      --help                   display this help and exit
      --version                output version information and exit
";

    #[test]
    fn a_go_flag_help_declares_whole_words_that_answer_double_dash_lookups() {
        let flags = parse(TERRAFORM_INIT_HELP);
        for name in ["-backend", "-force-copy", "-no-color", "-var"] {
            assert_eq!(found(&flags, name), Some(name));
        }
        assert_eq!(found(&flags, "-f"), None);
        assert_eq!(found(&flags, "-n"), None);
        assert_eq!(found(&flags, "--no-color"), Some("-no-color"));
        assert_eq!(found(&flags, "--force-copy"), Some("-force-copy"));
        assert_eq!(flags[0].placeholder.as_deref(), Some("=false"));
    }

    #[test]
    fn a_find_style_help_keeps_single_dash_words_apart_from_double_dash_names() {
        let flags = parse(FIND_HELP);
        assert_eq!(found(&flags, "-print"), Some("-print"));
        assert_eq!(found(&flags, "--print"), None);
        assert_eq!(found(&flags, "--help"), Some("--help"));
    }

    #[test]
    fn a_two_column_layout_keeps_every_long_name() {
        let flags = parse(&fixture("lazygit__--help.txt"));
        let long: Vec<&str> = flags.iter().flat_map(Flag::long_names).collect();
        assert_eq!(
            long,
            [
                "--help",
                "--path",
                "--filter",
                "--version",
                "--debug",
                "--logs",
                "--profile",
                "--config",
                "--print-config-dir",
                "--use-config-dir",
                "--work-tree",
                "--git-dir",
                "--use-config-file",
                "--screen-mode",
            ]
        );
        let config = find(&flags, &["-c"]).expect("lazygit declares -c");
        assert_eq!(config.flag.name(), "--config");
        let config_dir = find(&flags, &["-cd"]).expect("lazygit declares -cd");
        assert_eq!(config_dir.flag.name(), "--print-config-dir");
    }

    #[test]
    fn a_single_dash_word_kept_apart_from_its_double_dash_spelling_is_reported() {
        let mixed = help_declaring(&[&["-verbose"], &["--help"]]);
        assert_eq!(
            kept_apart(&mixed, &["--verbose", "-v"]),
            Some(("-verbose", "--verbose"))
        );
        assert_eq!(kept_apart(&mixed, &["--quiet"]), None);

        let single_dash_only = help_declaring(&[&["-verbose"], &["-h"]]);
        assert_eq!(kept_apart(&single_dash_only, &["--verbose"]), None);
    }

    #[test]
    fn a_definition_records_its_line_and_section() {
        let flags = parse("tool 1.0\n\nOptions:\n  -q, --quiet    Say less.\n");
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].line, 4);
        assert_eq!(flags[0].section.as_deref(), Some("Options:"));
        assert_eq!(flags[0].description(), "Say less.");
        assert_eq!(flags[0].placeholder, None);
    }
}
