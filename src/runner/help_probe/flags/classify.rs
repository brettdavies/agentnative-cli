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
//!
//! A definition can sit at any indent and can lead with `-`, `+x` or a
//! bracketed `[--name]`. Dash-led text at column 0, and any line that leads
//! with a bracket, is a usage wrap or prose unless a gap sets a description
//! off from it, or its description starts at the column the definition above
//! it uses.

use super::header::{self, Header};
use super::pieces::column_of;

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

/// Where the definition being read keeps its description, and where the
/// definitions under the current heading start their names.
#[derive(Default)]
struct Layout {
    description: Description,
    /// Columns a name starts at, in every definition under this heading.
    name_columns: Vec<usize>,
}

/// What the definition being read has by way of a description.
#[derive(Default)]
enum Description {
    /// No definition is being read.
    #[default]
    None,
    /// One that starts at this column.
    At(usize),
    /// None yet, for a definition indented this far.
    Awaited(usize),
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
        let trimmed = line.trim_start();
        let indent = column_of(line, line.len() - trimmed.len());
        let shaped = is_definition_shaped(trimmed);
        if self.continues(indent, shaped) {
            if let Description::Awaited(_) = self.description {
                self.description = Description::At(indent);
            }
            return Line::Continuation(line.trim());
        }
        let above = match std::mem::take(&mut self.description) {
            Description::At(column) => Some(column),
            Description::Awaited(_) | Description::None => None,
        };
        if indent == 0 {
            self.name_columns.clear();
        }
        if !shaped {
            return Line::Other;
        }
        let needs_gap = indent == 0 || trimmed.starts_with("[-");
        let Some(header) = header::tokenize(line) else {
            return if needs_gap {
                Line::Other
            } else {
                Line::Unnamed
            };
        };
        let aligned = indent == 0 && above.is_some() && header.description_column == above;
        if needs_gap && !header.set_off && !aligned {
            return Line::Other;
        }
        self.description = match header.description_column {
            Some(column) => Description::At(column),
            None => Description::Awaited(indent),
        };
        self.name_columns.extend(&header.name_columns);
        Line::Definition(header)
    }

    /// Whether a line at `indent` carries on the current definition's
    /// description: it sits at or past the description column, or it is the
    /// first line under a definition that has no description yet. A dash-led
    /// line at a column where names start is a definition instead: a GetOpt
    /// long-only row sits at the long column of the rows around it.
    fn continues(&self, indent: usize, shaped: bool) -> bool {
        match self.description {
            Description::At(column) => indent >= column,
            Description::Awaited(definition) => {
                indent > definition && !(shaped && self.name_columns.contains(&indent))
            }
            Description::None => false,
        }
    }
}

/// Whether text leads the way a definition does: with a dash (`-x`, `-word`,
/// `--word`), fzf's `+x`, or Thor's bracketed `[--name]`. A `---` rule does
/// not.
fn is_definition_shaped(trimmed: &str) -> bool {
    let plus_short = trimmed.starts_with('+')
        && trimmed
            .chars()
            .nth(1)
            .is_some_and(|c| c.is_ascii_alphabetic());
    trimmed.starts_with("[-")
        || plus_short
        || (trimmed.starts_with('-') && !trimmed.starts_with("---"))
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
    use crate::runner::help_probe::fixture_snapshots::fixture;
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

    /// The names each definition of `raw` declares, in help order.
    fn declared(raw: &str) -> Vec<Vec<String>> {
        parse(raw)
            .iter()
            .map(|flag| {
                flag.names
                    .iter()
                    .map(|name| name.spelling.clone())
                    .collect()
            })
            .collect()
    }

    /// Definition lines in layouts that do not indent with spaces, with the
    /// names each line declares. Each row names the tool and version.
    const LAYOUTS: &[(&str, &str, &[&[&str]])] = &[
        (
            "rsync 3.5.1: column 0, long name first",
            "Options\n--verbose, -v            increase verbosity\n--info=FLAGS             fine-grained informational verbosity\n",
            &[&["--verbose", "-v"], &["--info"]],
        ),
        (
            "ffmpeg 9.0.2: column 0, single-dash words",
            "Global options (affect whole program instead of just one file):\n-v <loglevel>       set logging level\n-y                  overwrite output files\n-n                  never overwrite output files\n-print_graphs_file <filename>  write execution graph data to the specified file\n",
            &[&["-v"], &["-y"], &["-n"], &["-print_graphs_file"]],
        ),
        (
            "ffmpeg before its placeholders gained brackets: a bare value word, then the gap",
            "Global options (affect whole program instead of just one file):\n-loglevel loglevel  set logging level\n-report            generate a report\n",
            &[&["-loglevel"], &["-report"]],
        ),
        (
            "Miller 6.22.0 sort: column 0, a description after one space at the shared column",
            "Options:\n-nr {a,b,c}     Numerical descending sort on the specified field names; nulls\n                sort first.\n-t {a,b,c}      Natural ascending sort on the specified field names.\n-tr|-rt {a,b,c} Natural descending sort on the specified field names.\n-h|--help       Show this message.\n",
            &[&["-nr"], &["-t"], &["-tr", "-rt"], &["-h", "--help"]],
        ),
        (
            "Thor 1.5.0: bracketed long names, and rows that start with one",
            "Options:\n  -f,        [--force]                                      # skip confirmation prompts\n  -n,        [--limit=N]                                    # maximum number of results\n             [--dry-run], [--no-dry-run], [--skip-dry-run]  # print what would change\n  -o, --out, [--output=FILE]                                # write output here\n",
            &[
                &["-f", "--force"],
                &["-n", "--limit"],
                &["--dry-run", "--no-dry-run", "--skip-dry-run"],
                &["-o", "--out", "--output"],
            ],
        ),
        (
            "typer 0.27.2: a box table, long name first and the short in its own cell",
            "╭─ Options ────────────────────────────────────────────╮\n│ --force             -f              skip confirmation prompts                │\n│ --limit             -n       <int>  maximum number of results to return, a   │\n│                                     deliberately long description that wraps │\n│ --dry-run                           print what would change                  │\n╰──────────────────────────────────────────────────────╯\n",
            &[&["--force", "-f"], &["--limit", "-n"], &["--dry-run"]],
        ),
        (
            "broot 1.56: a box table whose cells touch their edges",
            "│  -d    │--dates                      │Show the last modified date of files   │\n│        │--conf <paths>               │Semicolon separated paths to specific  │\n│        │                             │config files                           │\n",
            &[&["-d", "--dates"], &["--conf"]],
        ),
        (
            "fzf 0.74.4: a plus-prefixed short",
            "    -x, --extended           Extended-search mode\n    +x, --no-extended        Disable extended-search mode\n",
            &[&["-x", "--extended"], &["+x", "--no-extended"]],
        ),
        (
            "Go flag 1.27.1, indented with a TAB",
            "Usage of tool:\n\t-force\n\t\tskip confirmation prompts\n\t-n count\n\t\tmaximum number of results (default 10)\n",
            &[&["-force"], &["-n"]],
        ),
        (
            "aws-cli 2: groff overstrike",
            "       -\u{8}--\u{8}-r\u{8}re\u{8}ec\u{8}cu\u{8}ur\u{8}rs\u{8}si\u{8}iv\u{8}ve\u{8}e (boolean) Command is performed on all files or objects\n",
            &[&["--recursive"]],
        ),
        (
            "broot 1.56: ANSI escapes despite NO_COLOR",
            "\u{1b}[m│\u{1b}[m  -d \u{1b}[3m\u{1b}[0m   \u{1b}[m│\u{1b}[m--dates \u{1b}[3m\u{1b}[0m                     \u{1b}[m│\u{1b}[mShow the last modified date of files   \u{1b}[m│\u{1b}[m\n",
            &[&["-d", "--dates"]],
        ),
    ];

    #[test]
    fn a_definition_is_found_wherever_a_layout_prints_one() {
        let wrong: Vec<String> = LAYOUTS
            .iter()
            .filter(|(_, raw, expected)| {
                let read = declared(raw);
                read.len() != expected.len()
                    || read
                        .iter()
                        .zip(expected.iter())
                        .any(|(a, b)| a.as_slice() != *b)
            })
            .map(|(what, raw, expected)| {
                format!("{what}: read {:?}, declares {expected:?}", declared(raw))
            })
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// Dash-led text that declares nothing. Each row names its source.
    const NOT_DEFINITIONS: &[(&str, &str)] = &[
        (
            "biome 2: a usage line that wraps to column 0",
            "Usage: biome check [--write] [--unsafe] [--staged] [--changed] [--since=REF] [\n--watch] [PATH]...\n",
        ),
        (
            "helm 4.3.0 install: prose at column 0",
            "the --dry-run flag will output all generated chart manifests, including Secrets\n--hide-secret flag. Please carefully consider how and when these flags are used.\n",
        ),
        (
            "GNU tar 1.35: its defaults, at column 0",
            "*This* tar defaults to:\n--format=gnu -f- -b20 --quoting-style=escape --rmt-command=/usr/sbin/rmt\n--rsh-command=/usr/bin/rsh\n",
        ),
        (
            "tmux 3.7c: a synopsis and nothing else",
            "usage: tmux [-2CDhlNuVv] [-c shell-command] [-f file] [-L socket-name]\n            [-S socket-path] [-T features] [command [flags]]\n",
        ),
        (
            "aws-cli 2 s3 ls: a synopsis that lists one bracketed flag per line",
            "SYNOPSIS\n            ls\n          <S3Uri> or NONE\n          [--recursive]\n          [--page-size <value>]\n          [--human-readable]\n",
        ),
        (
            "git 2.56.0 commit -h: a usage line that wraps onto bracketed flags",
            "usage: git commit [-a | --interactive | --patch] [-s] [-v] [-u[<mode>]] [--amend]\n                  [--dry-run] [(-c | -C | --squash) <commit> | --fixup [(amend|reword):]<commit>]\n                  [-F <file> | -m <msg>] [--reset-author] [--allow-empty]\n",
        ),
        (
            "rclone 1.75 mount: `ls -l` output in a console example",
            "$ ls -l /mnt/\ntotal 1048577\n-rw-rw-r-- 1 user user 1073741824 Mar  3 16:03 1G\n-rw-rw-r-- 1 user user        185 Mar  3 16:03 1G.metadata\n",
        ),
        (
            "pixi 0.81.0 add: a bullet list at column 0",
            "- `pixi add python=3.9`: This will select the latest minor version that\n  complies with 3.9.*, i.e., python version 3.9.0, 3.9.1, 3.9.2, etc.\n",
        ),
    ];

    #[test]
    fn dash_led_text_that_is_not_a_definition_declares_nothing() {
        let wrong: Vec<String> = NOT_DEFINITIONS
            .iter()
            .filter(|(_, raw)| !declared(raw).is_empty())
            .map(|(what, raw)| format!("{what}: read {:?}", declared(raw)))
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn column_0_and_box_table_fixtures_yield_their_definitions() {
        let rsync = parse(&fixture("rsync__--help.txt"));
        assert_eq!(rsync.len(), 154);
        assert_eq!(rsync[0].declares("-v"), Some("-v"));

        let mlr = declared(&fixture("mlr__sort_--help.txt"));
        assert_eq!(mlr.len(), 11);
        assert!(mlr.contains(&vec!["-tr".to_string(), "-rt".to_string()]));

        let broot = parse(&fixture("broot__--help.txt"));
        assert_eq!(broot.len(), 46);
        assert!(
            broot
                .iter()
                .any(|flag| flag.declares("--dates") == Some("--dates"))
        );

        assert!(parse(&fixture("tmux__-h.txt")).is_empty());

        let typer = declared(&fixture("probe-typer__--help.txt"));
        assert!(typer.contains(&vec!["--limit".to_string(), "-n".to_string()]));
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
