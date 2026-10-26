//! The parts of a definition line's header, and what each part is: flag
//! names, a value placeholder, punctuation between groups, or prose.
//!
//! ```text
//! name        := "--[no-]" word | "--" word | "-" word | "-" char | "+" word | "/" word | "[" name "]"
//! word        := alnum (alnum | "." | "_" | "-" | "@")*
//! placeholder := "<..>" | "[..]" | "{..}" | "(..)" | quoted | "=" value | UPPER | type word
//! ```

/// Value words the Go `flag` package and pflag print after a name.
const TYPE_WORDS: &[&str] = &[
    "string",
    "strings",
    "stringArray",
    "stringToString",
    "int",
    "ints",
    "int32",
    "int64",
    "uint",
    "uint32",
    "uint64",
    "float",
    "float32",
    "float64",
    "duration",
    "value",
    "count",
    "bytes",
    "ip",
];

/// Punctuation a single-dash short name may be (`-?`, `-.`, `-#`, `-@`).
const PUNCTUATION_SHORTS: &[char] = &['?', '.', '#', '@'];

/// One whitespace-, comma- or pipe-separated part of a definition line.
#[derive(Clone, Copy)]
pub(super) struct Piece<'a> {
    pub text: &'a str,
    /// Byte offset of the piece in its line.
    pub start: usize,
    /// Two or more spaces, or a TAB, sit between the previous piece and this
    /// one: the gap that sets off a column or a description.
    pub after_gap: bool,
    /// A comma or pipe sits between the previous piece and this one.
    after_join: bool,
    /// That comma or pipe touches this piece, as inside a value list
    /// (`check1,check2..`).
    touches_join: bool,
    /// A comma or pipe sits between this piece and the next.
    before_join: bool,
}

/// What a piece of a header is.
pub(super) enum Kind<'a> {
    /// The names the piece declares, whole, and the placeholder text attached
    /// to them (`=false`, `[=WHEN]`, `<category>`; empty when none).
    Names(Vec<String>, &'a str),
    Placeholder,
    /// Punctuation between groups (` / `, `;`, `=`).
    Punctuation,
    Prose,
}

impl<'a> Piece<'a> {
    /// `first` is whether no name has been read yet on the line.
    pub(super) fn kind(&self, first: bool) -> Kind<'a> {
        if let Some((names, attached)) = self.names(first) {
            Kind::Names(names, attached)
        } else if self.is_placeholder() {
            Kind::Placeholder
        } else if self.text.contains(|c: char| c.is_alphanumeric()) {
            Kind::Prose
        } else {
            Kind::Punctuation
        }
    }

    /// `--[no-]x` declares `--x` and `--no-x`, and Thor's `[--x=N]` declares
    /// `--x`. A `/x` name is read only as an alias after a comma or pipe, and
    /// a `+x` name there or at the start of the line, so a path or a sum in
    /// prose is not a name.
    fn names(&self, first: bool) -> Option<(Vec<String>, &'a str)> {
        let text = self.text;
        if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']'))
            && inner.starts_with('-')
            && !inner.contains(|c: char| c.is_whitespace() || matches!(c, '[' | '|'))
        {
            let bare = Piece {
                text: inner,
                ..*self
            };
            return bare.names(first);
        }
        if let Some(body) = text.strip_prefix("--[no-]") {
            let (word, rest) = take_word(body)?;
            return Some((vec![format!("--{word}"), format!("--no-{word}")], rest));
        }
        let (prefix, body) = if let Some(body) = text.strip_prefix("--") {
            ("--", body)
        } else if let Some(body) = text.strip_prefix('-') {
            ("-", body)
        } else if let Some(body) = text.strip_prefix('+') {
            if !first && !self.after_join {
                return None;
            }
            ("+", body)
        } else if let Some(body) = text.strip_prefix('/') {
            if first || !self.after_join {
                return None;
            }
            ("/", body)
        } else {
            return None;
        };
        if let Some((word, rest)) = take_word(body) {
            if prefix == "/" && rest.starts_with('/') {
                return None;
            }
            return Some((vec![format!("{prefix}{word}")], rest));
        }
        let mut chars = body.chars();
        let short = chars.next()?;
        let rest = chars.as_str();
        let stands_alone = !rest.starts_with(|c: char| c.is_ascii_alphanumeric());
        let allowed = match prefix {
            "-" => PUNCTUATION_SHORTS.contains(&short),
            "/" => short == '?',
            _ => false,
        };
        (allowed && stands_alone).then(|| (vec![format!("{prefix}{short}")], rest))
    }

    /// Bracketed, quoted, `=`-led or path-like text, a word in capitals up to
    /// any bracket it carries (`FILE`, `KEY=VALUE`, `N[bcwkMG]`), a type
    /// word, any word a comma or pipe follows (sed's
    /// `-e script, --expression=script`), or one that touches the comma
    /// before it (shellcheck's `check1,check2..`).
    fn is_placeholder(&self) -> bool {
        let text = self.text;
        let head = text.split(['[', '<', '{', '(']).next().unwrap_or(text);
        text.starts_with(['<', '[', '{', '(', '\'', '"', '/'])
            || (text.starts_with('=') && text.len() > 1)
            || is_all_capitals(head)
            || TYPE_WORDS.contains(&text)
            || self.before_join
            || self.touches_join
    }
}

/// Split a line on whitespace, commas and pipes, keeping a bracketed group
/// whole so `<module path>` and `{auto,always,never}` stay one piece.
pub(super) fn pieces<'a>(line: &'a str) -> Vec<Piece<'a>> {
    let mut out: Vec<Piece<'_>> = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    let mut join_pending = false;
    let mut spaces = 0usize;
    let mut tab = false;
    let push = |out: &mut Vec<Piece<'a>>, s: usize, end: usize, join: bool, gap: bool| {
        out.push(Piece {
            text: &line[s..end],
            start: s,
            after_gap: gap,
            after_join: join,
            touches_join: join && line[..s].ends_with([',', '|']),
            before_join: false,
        });
    };
    for (i, c) in line.char_indices() {
        match c {
            '<' | '[' | '{' | '(' => depth += 1,
            '>' | ']' | '}' | ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        let is_join = depth == 0 && (c == ',' || c == '|');
        if depth == 0 && (c.is_whitespace() || is_join) {
            if let Some(s) = start.take() {
                push(&mut out, s, i, join_pending, spaces >= 2 || tab);
                (join_pending, spaces, tab) = (false, 0, false);
            }
            if is_join {
                join_pending = true;
                if let Some(last) = out.last_mut() {
                    last.before_join = true;
                }
            } else {
                spaces += 1;
                tab |= c == '\t';
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        push(&mut out, s, line.len(), join_pending, spaces >= 2 || tab);
    }
    out
}

/// The word at the start of `body` and the text after it. A word starts with
/// a letter or digit and runs through letters, digits, `.`, `_`, `-` and `@`
/// (java's `--disable-@files`); trailing dots and dashes (`--verbose...`,
/// `-Wno-<category>`) are not part of it.
fn take_word(body: &str) -> Option<(&str, &str)> {
    if !body.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return None;
    }
    let end = body
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@')))
        .unwrap_or(body.len());
    let word = body[..end].trim_end_matches(['.', '-']);
    Some((word, &body[end..]))
}

fn is_all_capitals(text: &str) -> bool {
    text.chars().any(|c| c.is_ascii_uppercase()) && !text.chars().any(|c| c.is_ascii_lowercase())
}
