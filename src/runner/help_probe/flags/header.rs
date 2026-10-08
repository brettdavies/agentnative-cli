//! The names a definition line declares, its placeholder, and the
//! description after them.
//!
//! ```text
//! definition := group (sep group)* (gap column)* description?
//! column     := group (sep group)*
//! group      := name placeholder*
//! sep        := "," | " " | "|" | punctuation
//! gap        := 2+ spaces | TAB
//! ```
//!
//! Every name is kept whole and in the order printed. A name ends where its
//! placeholder starts. After a gap, text that leads with a name and holds only
//! names and placeholders is another column of names (`-cd   --print-config-dir`);
//! anything else is the description. A word that is neither a name nor a
//! placeholder is prose: it ends the names, and the description starts at the
//! next gap, or at the prose itself when the line has no later gap.

use super::FlagName;
use super::classify::column_of;
use super::pieces::{Kind, Piece, pieces};

/// Punctuation that sets a description off from its header: Thor's `#`,
/// cmake's `=`, ffmpeg's `--`, qmd's `-`.
const MARKERS: &[&str] = &["#", "=", "--", "-"];

/// What a definition line declares.
pub(super) struct Header<'a> {
    pub names: Vec<FlagName>,
    /// The first value placeholder on the line, as written.
    pub placeholder: Option<&'a str>,
    pub description: &'a str,
    /// The column the description starts at, when the line carries one.
    pub description_column: Option<usize>,
    /// The column each name starts at.
    pub name_columns: Vec<usize>,
}

/// Read a definition line. `None` when the line declares no name.
pub(super) fn tokenize(line: &str) -> Option<Header<'_>> {
    let all = pieces(line);
    let mut names: Vec<FlagName> = Vec::new();
    let mut placeholder: Option<&str> = None;
    let mut name_columns: Vec<usize> = Vec::new();
    let mut prose: Option<usize> = None;
    let mut after_gap: Option<usize> = None;
    for (i, piece) in all.iter().enumerate() {
        if i > 0 && piece.after_gap && (prose.is_some() || !is_name_column(&all[i..])) {
            after_gap = Some(i);
            break;
        }
        if prose.is_some() {
            continue;
        }
        match piece.kind(names.is_empty()) {
            Kind::Names(declared, attached) => {
                name_columns.push(column_of(line, piece.start));
                for name in declared {
                    if !names.iter().any(|seen| seen.spelling == name) {
                        names.push(FlagName::new(name));
                    }
                }
                if !attached.is_empty() && !MARKERS.contains(&attached) {
                    placeholder.get_or_insert(attached);
                }
            }
            Kind::Placeholder => {
                placeholder.get_or_insert(piece.text);
            }
            Kind::Punctuation => {}
            Kind::Prose => prose = Some(i),
        }
    }
    if names.is_empty() {
        return None;
    }
    let described = after_gap
        .or(prose)
        .map(|i| {
            if MARKERS.contains(&all[i].text) {
                i + 1
            } else {
                i
            }
        })
        .and_then(|i| all.get(i));
    Some(Header {
        names,
        placeholder,
        description: described.map_or("", |first| line[first.start..].trim_end()),
        description_column: described.map(|first| column_of(line, first.start)),
        name_columns,
    })
}

/// Whether the pieces up to the next gap are a column of names: they lead
/// with a name and hold no prose. `--ignore option only ignores files` leads
/// with a name and is a description.
fn is_name_column(rest: &[Piece<'_>]) -> bool {
    let mut kinds = rest
        .iter()
        .enumerate()
        .take_while(|(i, piece)| *i == 0 || !piece.after_gap)
        .map(|(_, piece)| piece.kind(false));
    matches!(kinds.next(), Some(Kind::Names(..))) && kinds.all(|kind| !matches!(kind, Kind::Prose))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(line: &str) -> Vec<String> {
        tokenize(line)
            .map(|header| header.names)
            .unwrap_or_default()
            .iter()
            .map(|name| name.spelling.clone())
            .collect()
    }

    /// One definition line per layout, with every name it declares in the
    /// order printed. Each row names the tool and version that prints it.
    const DECLARED_NAMES: &[(&str, &str, &[&str])] = &[
        // Single-dash words.
        (
            "terraform 1.16.4 init",
            "-backend-config=path    Configuration to be merged with what is in the",
            &["-backend-config"],
        ),
        (
            "terraform 1.16.4 init",
            "-var 'foo=bar'          Set a value for one of the input variables in the root",
            &["-var"],
        ),
        ("Go flag 1.27.1", "-chdir DIR", &["-chdir"]),
        (
            "GNU grep 3.11",
            "-NUM                      same as --context=NUM",
            &["-NUM"],
        ),
        ("synthetic", "-Alh  list all, long, human", &["-Alh"]),
        // Attached placeholders end the name.
        (
            "cmake 4.4.4",
            "-W<category>                 = Enable the specified category of warnings.",
            &["-W"],
        ),
        (
            "cmake 4.4.4",
            "-Wno-<category>              = Suppress the specified category of warnings.",
            &["-Wno"],
        ),
        (
            "cmake 4.4.4",
            "-Werror=<category>           = Make the specified category of warnings",
            &["-Werror"],
        ),
        (
            "cmake 4.4.4",
            "-LR[A][H] <regex>            = Show cached variables that match the regex.",
            &["-LR"],
        ),
        (
            "OpenJDK 21 java",
            "-verbose:[class|module|gc|jni]",
            &["-verbose"],
        ),
        ("OpenJDK 21 java", "-D<name>=<value>", &["-D"]),
        (
            "OpenJDK 21 java",
            "-cp <class search path of directories and zip/jar files>",
            &["-cp"],
        ),
        (
            "kubectl 1.37.1 get",
            "-A, --all-namespaces=false:",
            &["-A", "--all-namespaces"],
        ),
        // Every alias, whatever the separator.
        (
            "cmake 4.4.4",
            "-h,-H,--help,-help,-usage,/? = Print usage information and exit.",
            &["-h", "-H", "--help", "-help", "-usage", "/?"],
        ),
        (
            "cmake 4.4.4",
            "--version[=json-v1],-version[=json-v1],/V[=json-v1],/version[=json-v1] [<file>]",
            &["--version", "-version", "/V", "/version"],
        ),
        (
            "cmake 4.4.4",
            "--preset <preset>,--preset=<preset>",
            &["--preset"],
        ),
        (
            "GNU sed 4.9",
            "-n, --quiet, --silent",
            &["-n", "--quiet", "--silent"],
        ),
        (
            "GNU sed 4.9",
            "-e script, --expression=script",
            &["-e", "--expression"],
        ),
        (
            "GNU sed 4.9",
            "-i[SUFFIX], --in-place[=SUFFIX]",
            &["-i", "--in-place"],
        ),
        (
            "GNU sed 4.9",
            "-E, -r, --regexp-extended",
            &["-E", "-r", "--regexp-extended"],
        ),
        (
            "yargs 18.2.0",
            "-o, --output, --out           write output here                       [string]",
            &["-o", "--output", "--out"],
        ),
        (
            "urfave/cli 2.27.7",
            "--limit value, -n value   maximum number of results to return",
            &["--limit", "-n"],
        ),
        (
            "argparse (Python 3.12)",
            "-n N, --limit N       maximum number of results to return, a deliberately",
            &["-n", "--limit"],
        ),
        (
            "argparse (Python 3.13)",
            "-n, --limit N         maximum number of results to return, a deliberately",
            &["-n", "--limit"],
        ),
        (
            "argparse (Python 3.12)",
            "--color {auto,always,never}",
            &["--color"],
        ),
        (
            "docopt-ng 0.9.0",
            "-h --help          Show this screen.",
            &["-h", "--help"],
        ),
        (
            "docopt-ng 0.9.0",
            "-o FILE --output=FILE  Write output here.",
            &["-o", "--output"],
        ),
        (
            "commander 15.0.0",
            "-y --yes  skip prompts",
            &["-y", "--yes"],
        ),
        ("OpenJDK 21 java", "-? -h -help", &["-?", "-h", "-help"]),
        (
            "OpenJDK 21 java",
            "-esa | -enablesystemassertions",
            &["-esa", "-enablesystemassertions"],
        ),
        (
            "click 8.5.0",
            "--color / --no-color2    boolean pair",
            &["--color", "--no-color2"],
        ),
        (
            "picocli 4.7.7",
            "-o, /out, --output=<out>   three names",
            &["-o", "/out", "--output"],
        ),
        // Names in a second column, with no comma between the columns.
        (
            "pandoc 3.12",
            "-f FORMAT, -r FORMAT  --from=FORMAT, --read=FORMAT",
            &["-f", "-r", "--from", "--read"],
        ),
        (
            "lazygit 0.65.1",
            "-cd   --print-config-dir   Print the config directory",
            &["-cd", "--print-config-dir"],
        ),
        (
            "lazygit 0.65.1",
            "-v    --version            Print the current version",
            &["-v", "--version"],
        ),
        (
            "shellcheck 0.11.0",
            "-f FORMAT           --format=FORMAT            Output format (checkstyle, diff, gcc, json, json1, quiet, tty)",
            &["-f", "--format"],
        ),
        (
            "shellcheck 0.11.0",
            "-o check1,check2..  --enable=check1,check2..   List of optional checks to enable (or 'all')",
            &["-o", "--enable"],
        ),
        (
            "shellcheck 0.11.0",
            "-C[WHEN]            --color[=WHEN]             Use color (auto, always, never)",
            &["-C", "--color"],
        ),
        // A description that starts with a flag is not a second column.
        (
            "files-to-prompt 0.6",
            "--ignore-files-only   --ignore option only ignores files",
            &["--ignore-files-only"],
        ),
        (
            "GNU tar 1.35",
            "--null                 -T reads null-terminated names; implies",
            &["--null"],
        ),
        (
            "synthetic",
            "-q, --quiet     -qq for silence",
            &["-q", "--quiet"],
        ),
        // A comma-separated list in prose does not chain names.
        (
            "terraform 1.16.4 untaint",
            "-state, state-out, and -backup are legacy options supported for the local",
            &["-state"],
        ),
        (
            "GNU findutils 4.10.0",
            "-wholename PATTERN -size N[bcwkMG] -true -type [bcdpflsD] -uid N",
            &["-wholename", "-size", "-true", "-type", "-uid"],
        ),
        (
            "GNU findutils 4.10.0",
            "-exec COMMAND ; -exec COMMAND {} + -ok COMMAND ;",
            &["-exec", "-ok"],
        ),
        ("OpenJDK 21 java", "--disable-@files", &["--disable-@files"]),
        // Negation pairs.
        (
            "git 2.56.0 commit -h",
            "-q, --[no-]quiet      suppress summary after successful commit",
            &["-q", "--quiet", "--no-quiet"],
        ),
        (
            "kingpin 2.4.0",
            "--[no-]help     Show context-sensitive help (also try --help-long and",
            &["--help", "--no-help"],
        ),
        // Dots, digits and punctuation in names.
        (
            "curl 8.22.0",
            "-0, --http1.0                     Use HTTP/1.0",
            &["-0", "--http1.0"],
        ),
        (
            "curl 8.22.0",
            "--http1.1                     Use HTTP/1.1",
            &["--http1.1"],
        ),
        (
            "curl 8.22.0",
            "-#, --progress-bar                Display transfer progress as a bar",
            &["-#", "--progress-bar"],
        ),
        ("ripgrep 15.2.0", "-., --hidden", &["-.", "--hidden"]),
        (
            "eza 0.23.5",
            "-@, --extended             list each file's extended attributes and sizes",
            &["-@", "--extended"],
        ),
        ("eza 0.23.5", "-?, --help     Print help", &["-?", "--help"]),
        (
            "clap 4.6.7",
            "-v, --verbose...  Increase verbosity",
            &["-v", "--verbose"],
        ),
        // Prose after one space ends the header.
        (
            "fzf 0.74.4",
            "--preview-border[=STYLE] Short for --preview-window=border-STYLE",
            &["--preview-border"],
        ),
        (
            "jq 1.8.2",
            "--slurpfile name file     set $name to an array of JSON values read",
            &["--slurpfile"],
        ),
        (
            "OpenJDK 21 java",
            "-m or --module <module>/<mainclass> are passed as the arguments to",
            &["-m"],
        ),
        (
            "synthetic",
            "--config /etc/tool.conf, /usr/share/tool reads both",
            &["--config"],
        ),
    ];

    #[test]
    fn every_declared_name_is_read_whole_and_in_order() {
        let misread: Vec<String> = DECLARED_NAMES
            .iter()
            .filter(|(_, line, expected)| names(line) != *expected)
            .map(|(tool, line, expected)| {
                format!(
                    "{tool}: `{line}` read as {:?}, declares {expected:?}",
                    names(line)
                )
            })
            .collect();
        assert!(misread.is_empty(), "{misread:#?}");
    }

    #[test]
    fn names_keep_the_order_printed() {
        assert_eq!(names("--version, -V  Print version"), ["--version", "-V"]);
    }

    #[test]
    fn the_first_placeholder_is_recorded_as_written() {
        let placeholder = |line| tokenize(line).and_then(|header| header.placeholder);
        assert_eq!(placeholder("-W<category>"), Some("<category>"));
        assert_eq!(placeholder("-backend=false"), Some("=false"));
        assert_eq!(placeholder("-n N, --limit N"), Some("N"));
        assert_eq!(placeholder("-var 'foo=bar'"), Some("'foo=bar'"));
        assert_eq!(
            placeholder("-cp <class search path of directories and zip/jar files>"),
            Some("<class search path of directories and zip/jar files>")
        );
        assert_eq!(placeholder("-o, --output string"), Some("string"));
        assert_eq!(placeholder("-q, --quiet"), None);
    }

    /// A definition line and the description the help gives it on that line.
    const DESCRIPTIONS: &[(&str, &str, &str)] = &[
        ("clap 4.6.7", "-q, --quiet    Say less.", "Say less."),
        ("clap 4.6.7", "--null", ""),
        (
            "lazygit 0.65.1",
            "-cd   --print-config-dir   Print the config directory",
            "Print the config directory",
        ),
        (
            "pandoc 3.12",
            "-f FORMAT, -r FORMAT  --from=FORMAT, --read=FORMAT",
            "",
        ),
        (
            "files-to-prompt 0.6",
            "--ignore-files-only   --ignore option only ignores files",
            "--ignore option only ignores files",
        ),
        (
            "GNU tar 1.35",
            "--null                 -T reads null-terminated names; implies",
            "-T reads null-terminated names; implies",
        ),
        // Description markers.
        (
            "Go flag 1.27.1",
            "-f\tskip confirmation prompts",
            "skip confirmation prompts",
        ),
        (
            "cmake 4.4.4",
            "-S <path-to-source>          = Explicitly specify a source directory.",
            "Explicitly specify a source directory.",
        ),
        (
            "cmake 4.4.4",
            "--compile-no-warning-as-error= Ignore COMPILE_WARNING_AS_ERROR property and",
            "Ignore COMPILE_WARNING_AS_ERROR property and",
        ),
        (
            "cmake 4.4.4",
            "-h,-H,--help,-help,-usage,/? = Print usage information and exit.",
            "Print usage information and exit.",
        ),
        (
            "Thor 1.5.0",
            "-n, --limit=N  # maximum number of results",
            "maximum number of results",
        ),
        (
            "ffmpeg 9.0.2",
            "-h      -- print basic options",
            "print basic options",
        ),
        (
            "qmd",
            "--timeout <minutes>         - Embed session cap in minutes (0 = no limit; default 30)",
            "Embed session cap in minutes (0 = no limit; default 30)",
        ),
        // Prose after one space, with and without a later gap.
        (
            "fzf 0.74.4",
            "--preview-border[=STYLE] Short for --preview-window=border-STYLE",
            "Short for --preview-window=border-STYLE",
        ),
        (
            "jq 1.8.2",
            "--slurpfile name file     set $name to an array of JSON values read",
            "set $name to an array of JSON values read",
        ),
    ];

    #[test]
    fn the_description_starts_after_the_header_and_any_marker() {
        let wrong: Vec<String> = DESCRIPTIONS
            .iter()
            .filter_map(|(tool, line, expected)| {
                let read = tokenize(line).map(|header| header.description);
                (read != Some(*expected))
                    .then(|| format!("{tool}: `{line}` gave {read:?}, not {expected:?}"))
            })
            .collect();
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn a_line_with_no_name_declares_nothing() {
        for line in [
            "- a bullet, not a flag",
            "-- end of options",
            "---",
            "-> see below",
            "--<option>=<value>",
        ] {
            assert!(tokenize(line).is_none(), "{line}");
        }
    }

    #[test]
    fn multibyte_text_does_not_break_a_slice() {
        for line in [
            "-H, --header <HEADER>     自定义请求头",
            "-X 请求, --request <方法>  HTTP 方法",
            "--选项  说明",
            "-é, --été  saison",
            "--limit │ -n │ <int> │ desc │",
            "-q…, --quiet…  Say less.",
        ] {
            let _ = tokenize(line);
        }
        assert_eq!(
            names("-H, --header <HEADER>     自定义请求头"),
            ["-H", "--header"]
        );
    }
}
