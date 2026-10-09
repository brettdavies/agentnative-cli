//! The names a definition line declares, and the description after them.

use super::FlagName;
use crate::runner::help_probe::before_description_gap;

/// The names `line` declares and the description after its gap, for a
/// definition line without its indentation. The header is split on commas and
/// each piece's first token is read as a name; at most one short and one long
/// name are kept, the short first. `None` when no token reads as a name.
pub(super) fn tokenize(line: &str) -> Option<(Vec<FlagName>, &str)> {
    let header = before_description_gap(line);

    let mut short: Option<String> = None;
    let mut long: Option<String> = None;
    for piece in header.split(',') {
        let candidate = piece.split_whitespace().next().unwrap_or(piece.trim());
        if candidate.is_empty() {
            continue;
        }
        if let Some(name) = long_name(candidate) {
            long = Some(name);
        } else if let Some(name) = short_name(candidate) {
            short = Some(name);
        }
    }

    let names: Vec<FlagName> = short.into_iter().chain(long).map(FlagName::new).collect();
    if names.is_empty() {
        return None;
    }
    Some((names, line[header.len()..].trim()))
}

/// The `--long` name in a token like `--long`, `--long=<VAL>` or
/// `--long[=<VAL>]`.
fn long_name(candidate: &str) -> Option<String> {
    if !candidate.starts_with("--") || candidate.len() <= 2 {
        return None;
    }
    let end = candidate[2..]
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .map(|i| i + 2)
        .unwrap_or(candidate.len());
    if end <= 2 {
        return None;
    }
    Some(candidate[..end].to_string())
}

/// The `-s` name in a token like `-s`, `-s<VAL>` or `-s,`: the dash and the
/// character after it, a letter, digit or `?`.
fn short_name(candidate: &str) -> Option<String> {
    let bytes = candidate.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'-' {
        return None;
    }
    let c = bytes[1] as char;
    if c.is_ascii_alphanumeric() || c == '?' {
        Some(format!("-{c}"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(line: &str) -> Vec<String> {
        tokenize(line)
            .map(|(names, _)| names)
            .unwrap_or_default()
            .iter()
            .map(|name| name.spelling.clone())
            .collect()
    }

    #[test]
    fn a_short_and_a_long_name_are_read_short_first() {
        assert_eq!(names("-q, --quiet    Say less."), ["-q", "--quiet"]);
        assert_eq!(names("--version, -V  Print version"), ["-V", "--version"]);
    }

    #[test]
    fn a_value_shape_stays_out_of_the_long_name() {
        assert_eq!(
            names("-e, --regexp=PATTERN   A pattern."),
            ["-e", "--regexp"]
        );
        assert_eq!(names("--color[=<WHEN>]   When."), ["--color"]);
        assert_eq!(names("--session <name>    Named session"), ["--session"]);
    }

    #[test]
    fn a_short_name_is_a_letter_digit_or_question_mark() {
        assert_eq!(short_name("-q"), Some("-q".into()));
        assert_eq!(short_name("-1"), Some("-1".into()));
        assert_eq!(short_name("-?"), Some("-?".into()));
        assert_eq!(short_name("--long"), None);
        assert_eq!(short_name("-"), None);
        assert_eq!(short_name("-,"), None);
    }

    #[test]
    fn the_description_is_the_text_after_the_gap() {
        let (_, description) = tokenize("-q, --quiet    Say less.").expect("a definition");
        assert_eq!(description, "Say less.");
        let (_, description) = tokenize("--null").expect("a definition");
        assert_eq!(description, "");
    }

    #[test]
    fn a_line_with_no_name_declares_nothing() {
        assert!(tokenize("- a bullet, not a flag").is_none());
    }
}
