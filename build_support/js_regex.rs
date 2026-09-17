//! JavaScript `RegExp` source to Rust `regex` syntax, for the web-audit
//! registry patterns the site evaluates with `new RegExp(pattern, flags)`
//! (`agentnative-site:src/worker/audit-web/assert.ts`).
//!
//! The translation is exact where the two engines can agree and names the
//! divergences it cannot close:
//!
//! - Flags: `i` becomes `(?i)`; `im` becomes `(?imR)`. CRLF mode (`R`) makes
//!   `^` and `$` treat `\r`, `\n` and `\r\n` as line ends the way JavaScript
//!   does, except that JavaScript also matches `^` between the two bytes of a
//!   `\r\n` pair and after U+2028/U+2029; no registry pattern anchors there.
//! - `\d`, `\w`, `\b` and their negations are ASCII in JavaScript (no `u`
//!   flag) and Unicode in Rust, so they are rewritten to explicit ASCII
//!   classes and `(?-u:\b)`.
//! - `\s` is rewritten to JavaScript's exact WhiteSpace + LineTerminator set,
//!   which differs from Rust's `\s` by U+0085 (Rust only) and U+FEFF (JS only).
//! - `.` is rewritten to exclude every JavaScript line terminator, including
//!   U+2028 and U+2029, which Rust's `.` matches.
//! - Lookaround and backreferences have no linear-time equivalent and are
//!   rejected; the caller names the check id.
//! - Case folding: Rust's `(?i)` uses Unicode simple folding, so `s` matches
//!   U+017F (long s) and `k` matches U+212A (Kelvin); JavaScript without `u`
//!   folds neither. The regex-parity fixture measures this; it is the one
//!   divergence a pattern flag cannot close.

/// JavaScript `\s` (WhiteSpace and LineTerminator), as a Rust class body.
const JS_SPACE: &str = r"\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}";
/// JavaScript `.` without the `s` flag: anything but a LineTerminator.
const JS_DOT: &str = r"[^\n\r\u{2028}\u{2029}]";

/// Translate a JavaScript pattern evaluated under `flags` (`i` or `im`, the
/// two combinations `assert.ts` uses) to a Rust `regex` pattern with the
/// equivalent inline flags. Returns a message naming the construct on any
/// input Rust cannot run under the same semantics.
pub fn translate(pattern: &str, flags: &str) -> Result<String, String> {
    let prefix = match flags {
        "i" => "(?i)",
        "im" => "(?imR)",
        other => {
            return Err(format!(
                "unsupported RegExp flag set {other:?} (want \"i\" or \"im\")"
            ));
        }
    };
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    out.push_str(prefix);
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                let Some(&next) = chars.get(i + 1) else {
                    return Err("pattern ends with a lone backslash".to_string());
                };
                let consumed = translate_escape(&chars, i + 1, next, in_class, &mut out)?;
                i += 1 + consumed;
                continue;
            }
            '[' if !in_class => {
                in_class = true;
                out.push('[');
                if chars.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                if chars.get(i + 1) == Some(&']') {
                    return Err("empty character class `[]` has no Rust equivalent".to_string());
                }
            }
            '[' if in_class => out.push_str(r"\["),
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '&' | '~' if in_class => out.push_str(&format!("\\x{:02X}", c as u32)),
            '-' if in_class && chars.get(i + 1) == Some(&'-') => out.push_str(r"\x2D"),
            '.' if !in_class => out.push_str(JS_DOT),
            '(' if !in_class => {
                if chars.get(i + 1) == Some(&'?') {
                    let tail: String = chars[i + 2..].iter().take(2).collect();
                    if tail.starts_with('=')
                        || tail.starts_with('!')
                        || tail == "<="
                        || tail == "<!"
                    {
                        return Err(format!("lookaround `(?{tail}` is not supported"));
                    }
                }
                out.push('(');
            }
            _ => out.push(c),
        }
        i += 1;
    }
    if in_class {
        return Err("unterminated character class".to_string());
    }
    Ok(out)
}

/// Translate the escape whose escaped character sits at `chars[at]`.
/// Returns how many characters after the backslash were consumed.
fn translate_escape(
    chars: &[char],
    at: usize,
    escaped: char,
    in_class: bool,
    out: &mut String,
) -> Result<usize, String> {
    let class = |body: &str, negated: bool| -> String {
        if in_class {
            if negated {
                format!("[^{body}]")
            } else {
                body.to_string()
            }
        } else if negated {
            format!("[^{body}]")
        } else {
            format!("[{body}]")
        }
    };
    match escaped {
        'd' => out.push_str(&class("0-9", false)),
        'D' => out.push_str(&class("0-9", true)),
        'w' => out.push_str(&class("0-9A-Za-z_", false)),
        'W' => out.push_str(&class("0-9A-Za-z_", true)),
        's' => out.push_str(&class(JS_SPACE, false)),
        'S' => out.push_str(&class(JS_SPACE, true)),
        'b' if in_class => out.push_str(r"\x08"),
        'b' => out.push_str(r"(?-u:\b)"),
        'B' if in_class => return Err("`\\B` inside a character class".to_string()),
        'B' => out.push_str(r"(?-u:\B)"),
        '1'..='9' => return Err(format!("backreference `\\{escaped}` is not supported")),
        'k' if chars.get(at + 1) == Some(&'<') => {
            return Err("named backreference `\\k<...>` is not supported".to_string());
        }
        '0' => out.push_str(r"\x00"),
        '/' => out.push('/'),
        'n' | 'r' | 't' | 'f' | 'v' => {
            out.push('\\');
            out.push(escaped);
        }
        'x' => {
            let hex: String = chars[at + 1..].iter().take(2).collect();
            if hex.len() != 2 || !hex.chars().all(|h| h.is_ascii_hexdigit()) {
                return Err(format!("malformed hex escape `\\x{hex}`"));
            }
            out.push_str(&format!("\\x{hex}"));
            return Ok(3);
        }
        'u' => {
            if chars.get(at + 1) == Some(&'{') {
                let close = chars[at..]
                    .iter()
                    .position(|&h| h == '}')
                    .ok_or_else(|| "unterminated `\\u{` escape".to_string())?;
                let body: String = chars[at + 2..at + close].iter().collect();
                out.push_str(&format!("\\u{{{body}}}"));
                return Ok(close + 1);
            }
            let hex: String = chars[at + 1..].iter().take(4).collect();
            if hex.len() != 4 || !hex.chars().all(|h| h.is_ascii_hexdigit()) {
                return Err(format!("malformed unicode escape `\\u{hex}`"));
            }
            out.push_str(&format!("\\u{{{hex}}}"));
            return Ok(5);
        }
        'c' => {
            let letter = chars
                .get(at + 1)
                .filter(|l| l.is_ascii_alphabetic())
                .ok_or_else(|| "malformed control escape `\\c`".to_string())?;
            out.push_str(&format!(
                "\\x{:02X}",
                (letter.to_ascii_uppercase() as u32) % 32
            ));
            return Ok(2);
        }
        c if c.is_ascii_punctuation() => {
            out.push('\\');
            out.push(c);
        }
        other => return Err(format!("unsupported escape `\\{other}`")),
    }
    Ok(1)
}
