//! The paragraph of prose a help opens with: the tool's own words for what it
//! is.

use super::flags::{is_section_heading, normalize};

/// The first paragraph of prose in `raw`, its wrapped lines joined by a space.
/// A usage line and the lines it indents are skipped wherever they stand: clap
/// prints the description above `Usage:`, docker and argparse below it, and GNU
/// tools on the next line. A section heading or a flag definition ends the
/// search, so a help that reaches one before any prose has no description.
pub(super) fn parse(raw: &str) -> Option<String> {
    let shown = normalize(raw);
    let mut paragraph: Vec<&str> = Vec::new();
    let mut in_usage = false;
    for line in shown.lines() {
        let text = line.trim();
        if text.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            in_usage = false;
        } else if is_usage(text) {
            in_usage = true;
        } else if !(in_usage && line.starts_with(char::is_whitespace)) {
            if is_section_heading(line) || text.starts_with('-') {
                break;
            }
            in_usage = false;
            paragraph.push(text);
        }
    }
    (!paragraph.is_empty()).then(|| paragraph.join(" "))
}

fn is_usage(text: &str) -> bool {
    text.get(..6)
        .is_some_and(|lead| lead.eq_ignore_ascii_case("usage:"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::help_probe::fixture_snapshots::fixture;

    #[test]
    fn the_description_is_found_wherever_the_layout_puts_it() {
        let cases = [
            // miniserve 0.35, clap: above `Usage:`.
            (
                "For when you really just want to serve some files over HTTP right now!\n\n\
                 Usage: miniserve [OPTIONS] [PATH]\n\nArguments:\n  [PATH]\n          Which path to serve\n",
                Some("For when you really just want to serve some files over HTTP right now!"),
            ),
            // helm 4.3, cobra: a second paragraph follows.
            (
                "The Kubernetes package manager\n\nCommon actions for Helm:\n\n- helm search:    search for charts\n",
                Some("The Kubernetes package manager"),
            ),
            // docker 29.8: below `Usage:`.
            (
                "Usage:  docker [OPTIONS] COMMAND\n\nA self-sufficient runtime for containers\n\nCommon Commands:\n  run         Create and run\n",
                Some("A self-sufficient runtime for containers"),
            ),
            // argparse: a wrapped usage, then the description.
            (
                "usage: tool [-h] [--foo FOO]\n            [--bar BAR]\n\nConvert a file.\n\noptions:\n  -h, --help  show this help\n",
                Some("Convert a file."),
            ),
            // GNU ls: on the line after the usage, wrapped.
            (
                "Usage: ls [OPTION]... [FILE]...\nList information about the FILEs (the current directory by default).\nSort entries alphabetically.\n\nMandatory arguments\n",
                Some(
                    "List information about the FILEs (the current directory by default). Sort entries alphabetically.",
                ),
            ),
            // ollama 0.35, cobra: the usage block is indented under its heading.
            (
                "Usage:\n  ollama [flags]\n  ollama [command]\n\nAvailable Commands:\n  serve       Start Ollama\n",
                None,
            ),
            // curl 8: flag definitions directly under the usage.
            (
                "Usage: curl [options...] <url>\n -d, --data <data>   HTTP POST data\n",
                None,
            ),
            // make 4: a section directly under the usage.
            (
                "Usage: make [options] [target] ...\nOptions:\n  -b, -m   Ignored for compatibility.\n",
                None,
            ),
            // git: the only prose introduces a list.
            (
                "usage: git [-v | --version] [-h | --help]\n           [--exec-path[=<path>]]\n\nThese are common Git commands used in various situations:\n",
                None,
            ),
            ("Options:\n  -h, --help    Show help.\n", None),
            ("", None),
        ];
        let read: Vec<(&str, Option<String>)> =
            cases.iter().map(|&(raw, _)| (raw, parse(raw))).collect();
        let expected: Vec<(&str, Option<String>)> = cases
            .iter()
            .map(|&(raw, description)| (raw, description.map(str::to_string)))
            .collect();
        assert_eq!(read, expected);
    }

    #[test]
    fn a_capture_with_ansi_styling_reads_as_a_terminal_shows_it() {
        assert_eq!(
            parse("\u{1b}[1mA fast server\u{1b}[0m\n\n\u{1b}[1mUsage:\u{1b}[0m tool\n").as_deref(),
            Some("A fast server")
        );
    }

    #[test]
    fn a_capture_of_a_real_help_reads_its_opening_prose() {
        let cases = [
            (
                "pixi__--help.txt",
                "Pixi [version 0.81.0] - Developer Workflow and Environment Management for \
                 Multi-Platform, Language-Agnostic Workspaces.",
            ),
            (
                "find__--help.txt",
                "Default path is the current directory; default expression is -print. \
                 Expression may consist of: operators, options, tests, and actions.",
            ),
        ];
        for (file, description) in cases {
            assert_eq!(
                parse(&fixture(file)).as_deref(),
                Some(description),
                "{file}"
            );
        }
    }
}
