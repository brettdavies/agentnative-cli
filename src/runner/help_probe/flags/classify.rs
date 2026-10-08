//! What kind of line a help line is. Every reader of flag definitions asks
//! here, so the flag parser and the env-hint proximity window agree on which
//! lines define a flag.
//!
//! A dash-led line is a definition unless it carries on the description of
//! the definition above it:
//!
//! ```text
//!   -n, --limit N         maximum number of results    definition; its description starts at column 24
//!                         -1 for no limit              at or past column 24: continuation
//!       --dry-run                                      left of column 24: definition, no description yet
//!           --dry-run prints the plan                  deeper, and at no column names start at: its description
//!
//! Advanced:                                            a heading starts over
//! ```
//!
//! A blank line changes nothing, because a description can run over several
//! paragraphs. A line left of the description column ends the description.

use super::header::{self, Header};

const TAB_STOP: usize = 8;

/// One line of a help text.
pub(super) enum Line<'a> {
    /// Declares a flag.
    Definition(Header<'a>),
    /// Shaped like a definition line, but declares no name (`  - a bullet`).
    Unnamed,
    /// Carries on the description of the definition above it.
    Continuation(&'a str),
    Heading(&'a str),
    Other,
}

/// Classify every line of `raw`, in order.
pub(super) fn lines(raw: &str) -> Vec<Line<'_>> {
    let mut layout = Layout::default();
    raw.lines().map(|line| layout.read(line)).collect()
}

/// The column a byte offset of `line` sits at. A TAB runs to the next stop.
pub(super) fn column_of(line: &str, byte: usize) -> usize {
    line[..byte].chars().fold(0, |column, c| match c {
        '\t' => (column / TAB_STOP + 1) * TAB_STOP,
        _ => column + 1,
    })
}

/// Where the definition being read keeps its description, and where the
/// definitions under the current heading start their names.
#[derive(Default)]
struct Layout {
    /// Column the current definition's description starts at.
    description: Option<usize>,
    /// Indent of the current definition while no line has described it.
    undescribed: Option<usize>,
    /// Columns a name starts at, in every definition under this heading.
    name_columns: Vec<usize>,
}

impl Layout {
    fn read<'a>(&mut self, line: &'a str) -> Line<'a> {
        if line.trim().is_empty() {
            return Line::Other;
        }
        if is_section_heading(line) {
            *self = Self::default();
            return Line::Heading(line.trim());
        }
        let indent = column_of(line, line.len() - line.trim_start().len());
        let dash_led = is_definition_line(line);
        if self.continues(indent, dash_led) {
            self.description.get_or_insert(indent);
            self.undescribed = None;
            return Line::Continuation(line.trim());
        }
        self.description = None;
        self.undescribed = None;
        if indent == 0 {
            self.name_columns.clear();
        }
        if !dash_led {
            return Line::Other;
        }
        let Some(header) = header::tokenize(line) else {
            return Line::Unnamed;
        };
        self.description = header.description_column;
        self.undescribed = header.description_column.is_none().then_some(indent);
        self.name_columns.extend(&header.name_columns);
        Line::Definition(header)
    }

    /// Whether a line at `indent` carries on the current definition's
    /// description: it sits at or past the description column, or it is the
    /// first line under a definition that has no description yet. A dash-led
    /// line at a column where names start is a definition instead: a GetOpt
    /// long-only row sits at the long column of the rows around it.
    fn continues(&self, indent: usize, dash_led: bool) -> bool {
        if let Some(column) = self.description {
            return indent >= column;
        }
        self.undescribed.is_some_and(|definition| {
            indent > definition && !(dash_led && self.name_columns.contains(&indent))
        })
    }
}

/// A line shaped like a definition: indented with a space, then a dash. A
/// `---` rule is not one.
fn is_definition_line(line: &str) -> bool {
    if !line.starts_with(' ') {
        return false;
    }
    let trimmed = line.trim_start();
    trimmed.starts_with('-') && !trimmed.starts_with("---")
}

/// A top-level section heading: not indented, ends in `:`, and carries an
/// uppercase letter (`OPTIONS:`, `Global Flags:`, `ENVIRONMENT:`).
pub(in crate::runner::help_probe) fn is_section_heading(line: &str) -> bool {
    !line.is_empty()
        && !line.starts_with(' ')
        && line.trim().ends_with(':')
        && line.chars().any(|c| c.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::help_probe::flags::parse;

    /// The 1-based lines of `raw` that declare a flag.
    fn definition_lines(raw: &str) -> Vec<usize> {
        parse(raw).iter().map(|flag| flag.line).collect()
    }

    fn description_of(raw: &str, name: &str) -> String {
        parse(raw)
            .iter()
            .find(|flag| flag.declares(name).is_some())
            .unwrap_or_else(|| panic!("{name} is not declared"))
            .description()
            .to_string()
    }

    /// Help excerpts whose wrapped description lines start with a dash, with
    /// the lines that declare a flag. Each names the tool and version.
    const WRAPPED: &[(&str, &str, &[usize])] = &[
        (
            "pixi 0.81.0, at a terminal wider than 80 columns",
            "  -v, --verbose...     Increase logging verbosity (-v for warnings, -vv for info, -vvv for debug,
                       -vvvv for trace)
  -q, --quiet...       Decrease logging verbosity (quiet mode)
",
            &[1, 3],
        ),
        (
            "docker 29.8.2 run",
            "      --pid string                       PID namespace to use
      --pids-limit int                   Tune container pids limit (set
                                         -1 for unlimited)
      --platform string                  Set platform if server is
",
            &[1, 2, 4],
        ),
        (
            "claude 2.1.289",
            "  --bare                                Minimal mode: skip hooks (those defined
                                        via: --system-prompt[-file],
                                        --append-system-prompt[-file], --add-dir
                                        (CLAUDE.md dirs), --mcp-config,
                                        --settings, --agents, --plugin-dir.
  --betas <betas...>                    Beta headers to include in API requests
",
            &[1, 6],
        ),
        (
            "terraform 1.16.4 init",
            "  -state-provider-lock-file [EXPERIMENTAL]
                          Specifies a lock file Terraform should use to establish trust in
                          a provider before initializing a state store for the first time.
                          Only usable with experiments enabled and the
                          -enable-pluggable-state-storage-experiment flag present.

  -test-directory=path    Set the Terraform test directory, defaults to \"tests\".
",
            &[1, 7],
        ),
        (
            "kingpin 2.4.0",
            "Flags:
      --[no-]help     Show context-sensitive help (also try --help-long and
                      --help-man).
  -f, --[no-]force    skip confirmation prompts
",
            &[2, 4],
        ),
        (
            "delta, a next-line description that runs over several paragraphs",
            "      --raw
          Do not alter the input in any way.

          Whether to examine ANSI color escape sequences in raw lines
          received from Git. This is on by default: it is how Delta supports Git's
          --color-moved feature. Set this to \"false\" to disable this
          behavior.

      --relative-paths
          Output all file paths relative to the current directory.
",
            &[1, 9],
        ),
        (
            "ripgrep 15.2.0",
            "    -., --hidden
        Search hidden files and directories. By default, hidden files and
        directories are skipped.

        Note that -./--hidden will include files and folders like .git
        regardless of --no-ignore-vcs. To exclude such paths when using
        -./--hidden, you must explicitly ignore them using another flag or
        ignore file.

    --iglob=GLOB
        Include or exclude files and directories for searching that match the
",
            &[1, 10],
        ),
        (
            "picocli 4.7.7, whose wrapped lines sit two columns past the description",
            "  -n, --limit=N              maximum number of results to return, a
                               -1 for no limit, deliberately wrapped
  -o, /out, --output=<out>   three names
",
            &[1, 3],
        ),
    ];

    #[test]
    fn a_wrapped_description_line_declares_nothing() {
        let wrong: Vec<String> = WRAPPED
            .iter()
            .filter(|(_, raw, expected)| definition_lines(raw) != *expected)
            .map(|(tool, raw, expected)| {
                format!(
                    "{tool}: definitions on lines {:?}, not {expected:?}",
                    definition_lines(raw)
                )
            })
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// Dash-led lines indented more deeply than the definition above them
    /// that are definitions all the same.
    const DEEPER_DEFINITIONS: &[(&str, &str, &[usize])] = &[
        (
            "clap 4.6.7: a long-only row at indent 6 after a short row at indent 2",
            "Options:
  -q, --quiet    Say less
      --null     Print a NUL byte after file paths
  -h, --help     Print help
",
            &[2, 3, 4],
        ),
        (
            "lazygit 0.65.1: --profile at indent 10 against 4",
            "    -l    --logs               Tail lazygit logs
          --profile            Start the profiler and serve it on http port 6060.
    -c    --config             Print the default config
",
            &[1, 2, 3],
        ),
        (
            "pandoc 3.12: a long-only row at the long column, after rows with no description",
            "  -o FILE               --output=FILE
                        --data-dir=DIRECTORY
  -M KEY[=VALUE]        --metadata=KEY[=VALUE]
                        --metadata-file=FILE
",
            &[1, 2, 3, 4],
        ),
        (
            "a dash-led line after a blank line and a new heading",
            "Options:
  -a, --all    Show everything, including entries that would
               otherwise be hidden

Advanced:
               --deep    A definition under a new heading
",
            &[2, 6],
        ),
    ];

    #[test]
    fn a_deeper_definition_stays_a_definition() {
        let wrong: Vec<String> = DEEPER_DEFINITIONS
            .iter()
            .filter(|(_, raw, expected)| definition_lines(raw) != *expected)
            .map(|(what, raw, expected)| {
                format!(
                    "{what}: definitions on lines {:?}, not {expected:?}",
                    definition_lines(raw)
                )
            })
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn a_description_runs_on_through_its_wrapped_and_next_lines() {
        // actionlint 1.7.12: the Go `flag` package's four spaces and a TAB.
        let go_flag = "Flags:\n  -format string\n    \tCustom template to format error messages in Go template syntax.\n  -verbose\n    \tEnable verbose output\n";
        assert_eq!(
            description_of(go_flag, "-format"),
            "Custom template to format error messages in Go template syntax."
        );
        assert_eq!(description_of(go_flag, "-verbose"), "Enable verbose output");

        // kubectl 1.37.1 `get`: a TAB-indented description under a deeper definition.
        let kubectl = "Options:\n    -A, --all-namespaces=false:\n\tIf present, list the requested object(s) across all namespaces.\n\n    --chunk-size=500:\n\tReturn large lists in chunks rather than all at once.\n";
        assert_eq!(
            description_of(kubectl, "--all-namespaces"),
            "If present, list the requested object(s) across all namespaces."
        );

        // argparse (Python 3.13): wrapped at the description column.
        let argparse = "options:\n  -n, --limit N         maximum number of results to return, a deliberately\n                        long description so that argparse has to wrap it onto\n                        continuation lines\n  --dry-run             print what would change\n";
        assert_eq!(
            description_of(argparse, "--limit"),
            "maximum number of results to return, a deliberately long description so that argparse has to wrap it onto continuation lines"
        );
        assert_eq!(
            description_of(argparse, "--dry-run"),
            "print what would change"
        );
    }

    #[test]
    fn a_definition_line_is_space_led_and_dash_led() {
        assert!(is_definition_line("  -q, --quiet    Say less."));
        assert!(is_definition_line("      --null"));
        assert!(!is_definition_line("-q, --quiet    Say less."));
        assert!(!is_definition_line("\t-q, --quiet    Say less."));
        assert!(!is_definition_line("  quiet mode"));
        assert!(!is_definition_line("  ---"));
        assert!(!is_definition_line(""));
    }

    #[test]
    fn a_section_heading_is_unindented_and_ends_in_a_colon() {
        assert!(is_section_heading("Options:"));
        assert!(is_section_heading("ENVIRONMENT:"));
        assert!(!is_section_heading("  Options:"));
        assert!(!is_section_heading("options:"));
        assert!(!is_section_heading("Usage: tool [OPTIONS]"));
        assert!(!is_section_heading(""));
    }
}
