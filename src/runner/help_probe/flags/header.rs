//! The names a definition line declares, its placeholder, and the
//! description after them.
//!
//! ```text
//! header := group (sep group)*
//! group  := name placeholder*
//! sep    := "," | " " | "|" | punctuation
//! ```
//!
//! Every name is kept whole and in the order printed. A name ends where its
//! placeholder starts, and a word that is neither a name nor a placeholder is
//! prose, which ends the header.

use super::FlagName;
use super::pieces::{Kind, pieces};
use crate::runner::help_probe::before_description_gap;

/// What a definition line declares.
pub(super) struct Header<'a> {
    pub names: Vec<FlagName>,
    /// The first value placeholder on the line, as written.
    pub placeholder: Option<&'a str>,
    pub description: &'a str,
}

/// Read a definition line without its indentation. `None` when the line
/// declares no name.
pub(super) fn tokenize(line: &str) -> Option<Header<'_>> {
    let header = before_description_gap(line);
    let mut names: Vec<FlagName> = Vec::new();
    let mut placeholder: Option<&str> = None;
    for piece in pieces(header) {
        match piece.kind(names.is_empty()) {
            Kind::Names(declared, attached) => {
                for name in declared {
                    if !names.iter().any(|seen| seen.spelling == name) {
                        names.push(FlagName::new(name));
                    }
                }
                if !attached.is_empty() {
                    placeholder.get_or_insert(attached);
                }
            }
            Kind::Placeholder => {
                placeholder.get_or_insert(piece.text);
            }
            Kind::Punctuation => {}
            Kind::Prose => break,
        }
    }
    if names.is_empty() {
        return None;
    }
    Some(Header {
        names,
        placeholder,
        description: line[header.len()..].trim(),
    })
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
        (
            "pandoc 3.12",
            "-f FORMAT, -r FORMAT  --from=FORMAT, --read=FORMAT",
            &["-f", "-r"],
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

    #[test]
    fn the_description_is_the_text_after_the_gap() {
        let description = |line| tokenize(line).map(|header| header.description);
        assert_eq!(description("-q, --quiet    Say less."), Some("Say less."));
        assert_eq!(description("--null"), Some(""));
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
