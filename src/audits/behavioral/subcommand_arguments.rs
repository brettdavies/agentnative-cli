//! Whether a subcommand's `--help` shows arguments of its own.
//!
//! A subcommand takes arguments of its own when its help shows any of:
//!
//! - an operand after the subcommand's name in a `Usage:` form (`<PATH>`,
//!   `[hostname]`, `FILE`, `[-- <ARGS>...]`). Option placeholders
//!   (`[OPTIONS]`, `[flags]`, `[global options]`) and option groups
//!   (`[--write]`) are not operands.
//! - a command list of its own (`Subcommands:`); the nested command is an
//!   argument.
//! - a flag other than `-h` / `--help` that the top-level help does not
//!   list. A flag the top-level help lists is the tool's global option.
//!
//! Help with no readable `Usage:` form naming the subcommand counts as
//! taking arguments: the exemption holds only where the help shows the
//! whole call shape.

use crate::runner::HelpOutput;
use crate::runner::help_probe::Flag;

pub(crate) fn takes_own_arguments(name: &str, help: &HelpOutput, global_flags: &[Flag]) -> bool {
    !help.subcommands().is_empty()
        || help
            .flags()
            .iter()
            .any(|flag| !is_help_flag(flag) && !is_global(flag, global_flags))
        || usage_shows_operand(help.raw(), name)
}

fn is_help_flag(flag: &Flag) -> bool {
    if flag.long_names().next().is_some() {
        flag.declares("--help").is_some()
    } else {
        flag.declares("-h").is_some()
    }
}

/// Whether the top-level help declares the name that identifies `flag`.
fn is_global(flag: &Flag, global_flags: &[Flag]) -> bool {
    global_flags
        .iter()
        .any(|global| global.declares(flag.name()).is_some())
}

/// True when a usage form naming `name` shows an operand after it, or when
/// no usage form names `name` at all.
fn usage_shows_operand(raw: &str, name: &str) -> bool {
    let mut named = false;
    for form in usage_forms(raw) {
        let elements = split_elements(&form);
        let Some(at) = elements.iter().position(|element| *element == name) else {
            continue;
        };
        named = true;
        if elements[at + 1..].iter().any(|element| is_operand(element)) {
            return true;
        }
    }
    !named
}

/// The usage block, from the `Usage:` label to the first blank line, split
/// into forms. A line that leads with the first form's tool token starts a
/// new form (cobra lists one per line); any other line continues the
/// current one, because some parsers wrap a long usage line at column 0.
fn usage_forms(raw: &str) -> Vec<String> {
    let mut lines = raw.lines();
    let Some(head) = lines.by_ref().find_map(usage_label_rest) else {
        return Vec::new();
    };
    let mut forms: Vec<String> = Vec::new();
    let mut tool: Option<String> = None;
    for line in std::iter::once(head).chain(lines) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if forms.is_empty() {
                continue;
            }
            break;
        }
        let first = trimmed.split_whitespace().next().unwrap_or_default();
        let tool = tool.get_or_insert_with(|| first.to_string());
        match forms.last_mut() {
            Some(form) if first != tool.as_str() => {
                form.push(' ');
                form.push_str(trimmed);
            }
            _ => forms.push(trimmed.to_string()),
        }
    }
    forms
}

/// The text after a `Usage:` label, or the empty string for a header that
/// stands alone (`USAGE`).
fn usage_label_rest(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("usage") {
        return Some("");
    }
    let (label, rest) = trimmed.split_once(':')?;
    label.eq_ignore_ascii_case("usage").then_some(rest)
}

/// Whitespace-separated elements, keeping a bracketed group whole so a
/// wrapped `[--level=\n<a|b>]` stays one element.
fn split_elements(form: &str) -> Vec<&str> {
    let mut elements = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    for (i, c) in form.char_indices() {
        match c {
            '[' | '<' | '(' | '{' => depth += 1,
            ']' | '>' | ')' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(s) = start.take() {
                elements.push(&form[s..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        elements.push(&form[s..]);
    }
    elements
}

fn is_operand(element: &str) -> bool {
    let element = element.trim_end_matches("...");
    if element.is_empty() || element == "|" || element.starts_with('-') {
        return false;
    }
    let text = strip_brackets(element).unwrap_or(element).trim();
    let is_option_group = text.starts_with('-') && !text.starts_with("-- ");
    !(text.is_empty() || is_option_group || is_option_placeholder(text))
}

fn strip_brackets(element: &str) -> Option<&str> {
    [('[', ']'), ('<', '>'), ('(', ')'), ('{', '}')]
        .iter()
        .find_map(|&(open, close)| element.strip_prefix(open)?.strip_suffix(close))
}

/// `OPTIONS`, `flags`, `global options`, `command options`.
fn is_option_placeholder(text: &str) -> bool {
    text.trim_end_matches("...")
        .split_whitespace()
        .last()
        .is_some_and(|word| {
            ["options", "option", "flags", "flag"]
                .iter()
                .any(|placeholder| word.eq_ignore_ascii_case(placeholder))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn takes(name: &str, sub_help: &str, top_help: &str) -> bool {
        let top = HelpOutput::from_raw(top_help);
        takes_own_arguments(name, &HelpOutput::from_raw(sub_help), top.flags())
    }

    const CLAP_TOP: &str = "Usage: tool [OPTIONS] <COMMAND>\n\n\
        Commands:\n  run   Run it\n  stop  Stop it\n\n\
        Options:\n  -q, --quiet    Less output\n  -h, --help     Print help\n  -V, --version  Print version\n";

    #[test]
    fn classifies_usage_shapes() {
        let cases: &[(&str, &str, bool)] = &[
            (
                "stop",
                "Usage: tool stop\n\nOptions:\n  -h, --help  Print help\n",
                false,
            ),
            (
                "stop",
                "Usage: tool stop [OPTIONS]\n\nOptions:\n  -h, --help  Print help\n",
                false,
            ),
            (
                "stop",
                "Usage: tool [global options] stop [options]\n",
                false,
            ),
            (
                "stop",
                "Usage: tool stop [OPTIONS]\n\nOptions:\n  -q, --quiet  Less output\n  -h, --help   Print help\n",
                false,
            ),
            ("run", "Usage: tool run [OPTIONS] <PATH>\n", true),
            ("run", "Usage: tool run [hostname]\n", true),
            ("run", "Usage: tool run NAME\n", true),
            ("run", "Usage: tool run [PATH]...\n", true),
            ("run", "Usage: tool run [OPTIONS] [-- <ARGS>...]\n", true),
            ("run", "Usage: tool run {start|stop}\n", true),
            (
                "run",
                "Usage: tool run [OPTIONS]\n\nOptions:\n  -j, --json  JSON output\n  -h, --help  Print help\n",
                true,
            ),
            (
                "run",
                "Usage: tool run\n\nSubcommands:\n    list    List them\n    show    Show one\n",
                true,
            ),
            (
                "run",
                "Runs it.\n\nOptions:\n  -h, --help  Print help\n",
                true,
            ),
            ("run", "Usage: tool [OPTIONS]\n", true),
        ];
        for (name, sub_help, expected) in cases {
            assert_eq!(
                takes(name, sub_help, CLAP_TOP),
                *expected,
                "{name}: {sub_help}"
            );
        }
    }

    #[test]
    fn reads_cobra_forms_and_wrapped_usage() {
        let cobra_top = "Usage:\n  tool [command]\n\nAvailable Commands:\n  serve  Serve\n  pull   Pull\n\n\
            Flags:\n  -c, --config string   config file\n  -h, --help            help for tool\n";
        let serve = "Start it\n\nUsage:\n  tool serve [flags]\n\nAliases:\n  serve, start\n\n\
            Flags:\n  -h, --help   help for serve\n\nGlobal Flags:\n  -c, --config string   config file\n";
        assert!(!takes("serve", serve, cobra_top));
        let pull = "Usage:\n  tool pull [flags]\n  tool pull [command]\n\nFlags:\n  -h, --help   help for pull\n";
        assert!(takes("pull", pull, cobra_top));
        let wrapped = "Usage: tool serve [--log-level=\n<none|debug>] [--log-kind=<pretty|json>]\n\n\
            Available options:\n    -h, --help  Prints help information\n";
        assert!(!takes("serve", wrapped, cobra_top));
    }
}
