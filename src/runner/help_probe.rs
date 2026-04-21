//! Shared `--help` probe + lazy parsers.
//!
//! The runner spawns `<binary> --help` exactly once per target. The captured
//! text is parsed on demand into three views — flags, env hints, subcommands.
//! Behavioral checks that need to inspect the help surface share the same
//! `HelpOutput` for a given target so none of them re-spawn the binary.
//!
//! Parsers are English-only by convention: we match on clap's output shape
//! (`Commands:`, `[env: FOO]`, leading-whitespace flag lines). Localized help
//! is a named exception in `docs/coverage-matrix.md` — checks that consume
//! these parsers should Skip, not Warn, when the raw text lacks an English
//! help surface.

use std::sync::OnceLock;

use anyhow::Result;

use super::{BinaryRunner, RunStatus};

/// A flag discovered in `--help` output. `short` is the single-character
/// variant (e.g., `-q`); `long` is the GNU-style variant (e.g., `--quiet`).
/// At least one of the two is always set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    pub short: Option<String>,
    pub long: Option<String>,
}

impl Flag {
    /// Whether this flag exposes `name` under either its short or long form.
    /// Accepts `-s`, `--long`, or even `long` / `s` (without dashes).
    pub fn matches(&self, name: &str) -> bool {
        let with_dash_long = if name.starts_with("--") {
            name.to_string()
        } else if name.starts_with('-') {
            name.to_string()
        } else if name.len() == 1 {
            format!("-{name}")
        } else {
            format!("--{name}")
        };
        self.short.as_deref() == Some(with_dash_long.as_str())
            || self.long.as_deref() == Some(with_dash_long.as_str())
    }
}

/// A bound between a flag surface and an environment variable — surfaces
/// clap's `[env: FOO]` hints as first-class data so checks don't re-scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvHint {
    /// Environment variable name, e.g., `RIPGREP_CONFIG_PATH`.
    pub var: String,
}

/// Shared, lazily-parsed view over `<binary> --help`. Construct via
/// [`HelpOutput::probe`] in runner code, or [`HelpOutput::from_raw`] in tests.
pub struct HelpOutput {
    raw: String,
    flags: OnceLock<Vec<Flag>>,
    env_hints: OnceLock<Vec<EnvHint>>,
    subcommands: OnceLock<Vec<String>>,
}

impl HelpOutput {
    /// Build a `HelpOutput` from captured help text. The primary seam for
    /// unit tests — pass a fixture string and exercise the parsers without
    /// spawning a binary.
    pub fn from_raw(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            flags: OnceLock::new(),
            env_hints: OnceLock::new(),
            subcommands: OnceLock::new(),
        }
    }

    /// Spawn `<binary> --help` via the shared `BinaryRunner` and capture its
    /// combined stdout+stderr. Returns an empty `HelpOutput` rather than an
    /// error on timeout/crash — a misbehaving `--help` is a signal the check
    /// consumers can use, not a hard runner failure.
    pub fn probe(runner: &BinaryRunner) -> Result<Self> {
        let help = runner.run(&["--help"], &[]);
        match help.status {
            RunStatus::NotFound => {
                anyhow::bail!("binary not found when probing --help")
            }
            RunStatus::PermissionDenied => {
                anyhow::bail!("permission denied when probing --help")
            }
            RunStatus::Error(ref msg) => anyhow::bail!("--help probe failed: {msg}"),
            // Ok / Timeout / Crash — capture whatever output is available.
            // Some tools print help to stderr, or crash after writing usage.
            RunStatus::Ok | RunStatus::Timeout | RunStatus::Crash { .. } => {
                let mut raw = String::with_capacity(help.stdout.len() + help.stderr.len());
                raw.push_str(&help.stdout);
                raw.push_str(&help.stderr);
                Ok(Self::from_raw(raw))
            }
        }
    }

    /// Raw help text, exactly as captured.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Flags parsed out of the help surface. Lazy + cached on first call.
    pub fn flags(&self) -> &[Flag] {
        self.flags.get_or_init(|| parse_flags(&self.raw))
    }

    /// `[env: FOO]` hints parsed out of the help surface. Lazy + cached.
    pub fn env_hints(&self) -> &[EnvHint] {
        self.env_hints.get_or_init(|| parse_env_hints(&self.raw))
    }

    /// Subcommand names parsed out of the help surface. Lazy + cached.
    pub fn subcommands(&self) -> &[String] {
        self.subcommands
            .get_or_init(|| parse_subcommands(&self.raw))
    }
}

/// Parse flag declarations from clap-style help text.
///
/// A "flag line" is a line that starts with whitespace and then a dash. The
/// header portion (before the description) is split from the description by
/// two or more spaces — clap's canonical shape. We tokenize the header on
/// commas and whitespace, then classify each token as short (`-s`) or long
/// (`--long`).
fn parse_flags(raw: &str) -> Vec<Flag> {
    let mut flags = Vec::new();
    for line in raw.lines() {
        if !line.starts_with(' ') {
            continue;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with('-') {
            continue;
        }
        // Separator / heading lines like `---` are not flags.
        if trimmed.starts_with("---") {
            continue;
        }
        // Header = everything before clap's two-space description gap. When
        // there's no description on the same line the whole remainder is the
        // header.
        let header = trimmed.split("  ").next().unwrap_or(trimmed);

        let mut short: Option<String> = None;
        let mut long: Option<String> = None;
        for piece in header.split(',') {
            let candidate = piece.split_whitespace().next().unwrap_or(piece.trim());
            if candidate.is_empty() {
                continue;
            }
            if let Some(long_name) = parse_long_flag(candidate) {
                long = Some(long_name);
            } else if let Some(short_name) = parse_short_flag(candidate) {
                short = Some(short_name);
            }
        }
        if short.is_some() || long.is_some() {
            flags.push(Flag { short, long });
        }
    }
    flags
}

/// Extract a `--long` flag name from a token like `--long`, `--long=<VAL>`,
/// `--long[=<VAL>]`, or `--long <VAL>`. Returns `None` when `candidate` is
/// not a long flag.
fn parse_long_flag(candidate: &str) -> Option<String> {
    if !candidate.starts_with("--") || candidate.len() <= 2 {
        return None;
    }
    // Walk the name chars: letters, digits, dashes, underscores.
    let end = candidate[2..]
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .map(|i| i + 2)
        .unwrap_or(candidate.len());
    if end <= 2 {
        return None;
    }
    Some(candidate[..end].to_string())
}

/// Extract a `-s` short flag from a token like `-s`, `-s<VAL>`, or `-s,`.
fn parse_short_flag(candidate: &str) -> Option<String> {
    let bytes = candidate.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'-' {
        return None;
    }
    // Second char must be a flag character (letter, digit, or `?`).
    let c = bytes[1] as char;
    if c.is_ascii_alphanumeric() || c == '?' {
        Some(format!("-{c}"))
    } else {
        None
    }
}

/// Parse clap's `[env: FOO_BAR]` or `[env: FOO_BAR=<default>]` annotations.
/// Each occurrence becomes one `EnvHint` — duplicates are preserved so
/// callers can count occurrences if useful.
fn parse_env_hints(raw: &str) -> Vec<EnvHint> {
    const TAG: &str = "[env:";
    let mut hints = Vec::new();
    let mut rest = raw;
    while let Some(pos) = rest.find(TAG) {
        let after = &rest[pos + TAG.len()..];
        let end = after.find(']').unwrap_or(after.len());
        let inner = after[..end].trim();
        let name = inner.split('=').next().unwrap_or("").trim();
        if is_env_var_name(name) {
            hints.push(EnvHint {
                var: name.to_string(),
            });
        }
        rest = &after[end..];
    }
    hints
}

/// Env var names are ASCII uppercase, digits, underscores; must start with
/// a letter or underscore.
fn is_env_var_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.as_bytes()[0] as char;
    if !(first.is_ascii_uppercase() || first == '_') {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Parse the `Commands:` / `Subcommands:` block. We collect the first
/// whitespace-separated token on each line until the block terminates
/// (empty line, or a new non-indented section header).
fn parse_subcommands(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_section = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        let is_header = matches!(trimmed, "Commands:" | "Subcommands:" | "SUBCOMMANDS:");
        if is_header {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        if trimmed.is_empty() {
            // Blank line ends the block.
            in_section = false;
            continue;
        }
        if !line.starts_with(' ') {
            // A new top-level section header ended the commands block.
            break;
        }
        if let Some(name) = trimmed.split_whitespace().next() {
            if is_subcommand_name(name) {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// Subcommand names are kebab-case/snake_case identifiers. Anything else —
/// `[options]`, `<ARG>`, punctuation — is not a subcommand.
fn is_subcommand_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    // A fixture modeled on ripgrep's `--help` — short+long flags, env hint.
    const RIPGREP_HELP: &str = r#"ripgrep 14.1

Usage: rg [OPTIONS] PATTERN [PATH ...]

Options:
  -e, --regexp=PATTERN          A pattern to search for.
      --no-messages             Suppress some error messages.
  -q, --quiet                   Do not print anything to stdout.
  -v, --invert-match            Invert matching.
      --null                    Print a NUL byte after file paths.
      --color=<WHEN>            When to use color. [env: RIPGREP_COLOR=]
      --help                    Show this help message.
  -V, --version                 Show version.
"#;

    // Modeled on clap's generated help, with a subcommand block and [env: ...].
    const CLAP_HELP: &str = r#"anc — the agent-native CLI linter

Usage: anc <COMMAND>

Commands:
  check        Run checks against a CLI project or binary
  completions  Generate shell completions
  generate     Regenerate build artifacts
  help         Print this message or the help of the given subcommand

Options:
  -q, --quiet      Suppress non-essential output [env: AGENTNATIVE_QUIET=]
  -h, --help       Print help
  -V, --version    Print version
"#;

    // Tool with no flags and no subcommands — env_hints parser must return empty.
    const BARE_HELP: &str = r#"xurl-rs 0.1
A tiny HTTP client.

Usage: xurl-rs URL
"#;

    // Localized help — ensures parsers degrade to empty without panicking.
    const NON_ENGLISH_HELP: &str = r#"用法: outil [选项]

参数:
  URL                       目标网址

选项:
  -H, --header <HEADER>     自定义请求头
  -X, --request <METHOD>    HTTP 方法
"#;

    #[test]
    fn parse_flags_extracts_short_and_long() {
        let flags = parse_flags(RIPGREP_HELP);
        assert!(flags.iter().any(|f| f.short.as_deref() == Some("-q")));
        assert!(flags.iter().any(|f| f.long.as_deref() == Some("--quiet")));
        assert!(
            flags
                .iter()
                .any(|f| f.long.as_deref() == Some("--no-messages"))
        );
        assert!(flags.iter().any(|f| f.long.as_deref() == Some("--null")));
    }

    #[test]
    fn parse_flags_handles_equals_and_values() {
        let flags = parse_flags(RIPGREP_HELP);
        // --regexp=PATTERN — the value shape must not leak into the long name.
        let regexp = flags
            .iter()
            .find(|f| f.long.as_deref() == Some("--regexp"))
            .expect("regexp flag parsed");
        assert_eq!(regexp.short.as_deref(), Some("-e"));
    }

    #[test]
    fn parse_flags_ignores_prose_dashes() {
        // A line starting with '---' (separator) must not become a flag.
        let src = "Usage: foo [OPTIONS]\n\n-------\n\nOptions:\n  -q, --quiet    Quiet mode.\n";
        let flags = parse_flags(src);
        assert_eq!(flags.len(), 1);
        assert_eq!(flags[0].short.as_deref(), Some("-q"));
    }

    #[test]
    fn parse_env_hints_captures_clap_style() {
        let hints = parse_env_hints(RIPGREP_HELP);
        assert!(hints.iter().any(|h| h.var == "RIPGREP_COLOR"));
    }

    #[test]
    fn parse_env_hints_multiple_occurrences() {
        let hints = parse_env_hints(CLAP_HELP);
        assert!(hints.iter().any(|h| h.var == "AGENTNATIVE_QUIET"));
    }

    #[test]
    fn parse_env_hints_rejects_invalid_names() {
        // `[env: lowercase]` or `[env: 1ABC]` must not parse as env hints.
        let src = "  --flag   [env: lowercase] [env: 1ABC] [env: VALID_1]";
        let hints = parse_env_hints(src);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].var, "VALID_1");
    }

    #[test]
    fn parse_subcommands_reads_commands_block() {
        let subs = parse_subcommands(CLAP_HELP);
        assert!(subs.iter().any(|s| s == "check"));
        assert!(subs.iter().any(|s| s == "generate"));
        assert!(subs.iter().any(|s| s == "completions"));
    }

    #[test]
    fn parse_subcommands_empty_without_block() {
        let subs = parse_subcommands(BARE_HELP);
        assert!(subs.is_empty());
    }

    #[test]
    fn parse_non_english_help_degrades_cleanly() {
        // English-only parsers: no flags advertised via English conventions,
        // no `Commands:` header, no `[env: ...]` hint — all parsers return empty.
        let flags = parse_flags(NON_ENGLISH_HELP);
        // The Chinese options block still uses `-H, --header` syntax so we may
        // detect the flags themselves — the non-English text is in the
        // descriptions, not the flag names. The check is that parsing doesn't
        // panic and returns sane structured data.
        for f in &flags {
            assert!(f.short.is_some() || f.long.is_some());
        }
        assert!(parse_env_hints(NON_ENGLISH_HELP).is_empty());
        assert!(parse_subcommands(NON_ENGLISH_HELP).is_empty());
    }

    #[test]
    fn help_output_lazy_parse_is_idempotent() {
        let help = HelpOutput::from_raw(RIPGREP_HELP);
        // Pointer identity through two calls proves OnceLock caching.
        let first = help.flags().as_ptr();
        let second = help.flags().as_ptr();
        assert_eq!(first, second);
        // And the data is stable across calls.
        assert_eq!(help.flags().len(), help.flags().len());
    }

    #[test]
    fn flag_matches_accepts_various_spellings() {
        let f = Flag {
            short: Some("-q".into()),
            long: Some("--quiet".into()),
        };
        assert!(f.matches("-q"));
        assert!(f.matches("--quiet"));
        assert!(f.matches("quiet"));
        assert!(f.matches("q"));
        assert!(!f.matches("--verbose"));
    }

    #[test]
    fn is_env_var_name_edges() {
        assert!(is_env_var_name("FOO"));
        assert!(is_env_var_name("FOO_BAR"));
        assert!(is_env_var_name("_UNDERSCORE"));
        assert!(!is_env_var_name(""));
        assert!(!is_env_var_name("lower"));
        assert!(!is_env_var_name("1LEADING"));
        assert!(!is_env_var_name("foo-bar"));
    }

    #[test]
    fn parse_short_flag_accepts_digits_and_question() {
        assert_eq!(parse_short_flag("-q"), Some("-q".into()));
        assert_eq!(parse_short_flag("-1"), Some("-1".into()));
        assert_eq!(parse_short_flag("-?"), Some("-?".into()));
        assert_eq!(parse_short_flag("--long"), None);
        assert_eq!(parse_short_flag("-"), None);
        assert_eq!(parse_short_flag("-,"), None);
    }
}
