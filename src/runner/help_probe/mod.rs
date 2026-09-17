//! Shared `--help` probe + lazy parsers.
//!
//! The runner spawns `<binary> --help` exactly once per target. The captured
//! text is parsed on demand into views: flags, env hints, command blocks, and
//! subcommands. Behavioral audits that need to inspect the help surface share
//! the same `HelpOutput` for a given target so none of them re-spawn the
//! binary.
//!
//! Parsers are English-only by convention: we match on clap's output shape
//! (`Commands:`, `[env: FOO]`, leading-whitespace flag lines) and on the
//! hand-written command-block shape (`Common commands:` headers whose entries
//! lead with the tool's own name). Localized help is a named exception in
//! `docs/coverage-matrix.md` — audits that consume these parsers should Skip,
//! not Warn, when the raw text lacks an English help surface.
//!
//! `parse_env_hints` uses two complementary patterns:
//! - **Pattern 1 (clap-style)**: `[env: FOO]` annotations inside the flag
//!   table. Exact match; high precision.
//! - **Pattern 2 (bash-style)**: `$FOO` or `TOOL_FOO` tokens co-occurring
//!   within a ±4-line window of a flag definition, plus a dedicated
//!   `ENVIRONMENT` section scan. Catches tools like `ripgrep`, `gh`, and
//!   `aider` that document env bindings in free prose rather than clap
//!   annotations. Three mitigations keep false positives in check:
//!   uppercase-identifier shape (length ≥ 3), same-paragraph window, and
//!   a shell-env blacklist (`PATH`, `HOME`, etc.).
//!
//! Results from both patterns are deduped by env-var name. Confidence on
//! `p1-env-hints` stays `Medium` — widening does not raise confidence.

use std::collections::HashSet;
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
        let with_dash_long = if name.starts_with('-') {
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

mod env_hints_bash;

/// Which detection pattern surfaced an [`EnvHint`]. Agents debugging a
/// false positive need to know whether a hint came from a clap
/// `[env: FOO]` annotation (high signal), flag-adjacent prose (medium),
/// or a dedicated `ENVIRONMENT` section (medium). Serialized as
/// snake_case if/when [`EnvHint`] is exposed in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvHintSource {
    /// Pattern 1 — clap's `[env: FOO]` / `[env: FOO=<default>]`
    /// annotation. The strongest signal; clap emits this when
    /// `env = "FOO"` is declared on an `Arg`.
    ClapAnnotation,
    /// Pattern 2a — a bash-style `$FOO` or `TOOL_FOO` token appears
    /// within the ±4-line proximity window around a flag definition.
    Proximity,
    /// Pattern 2b — a token appears inside a dedicated `ENVIRONMENT` /
    /// `ENV VARS` / `ENVIRONMENT VARIABLES` section.
    EnvSection,
}

/// A bound between a flag surface and an environment variable — surfaces
/// clap's `[env: FOO]` hints as first-class data so audits don't re-scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvHint {
    /// Environment variable name, e.g., `RIPGREP_CONFIG_PATH`.
    pub var: String,
    /// Which detection pattern surfaced this hint. Lets evidence strings
    /// and downstream agents see where the signal came from without
    /// re-running the parser.
    pub source: EnvHintSource,
}

/// A `... commands:` block from the help surface, kept alongside the names
/// read from it so audits can report what was read, not only what survived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandBlock {
    /// The header line as written, e.g. `Commands:` or `Common commands:`.
    pub header: String,
    /// Entry lines as written, without their indentation.
    pub entries: Vec<String>,
    /// The tool-name token every entry led with. Names are read from what
    /// follows it; `None` when the block is not prefixed.
    pub prefix: Option<String>,
}

impl CommandBlock {
    /// The invocation part of `entry`: the text before the two-space (or
    /// tab) description gap, minus the shared prefix when the block is
    /// prefixed. Empty for the bare-invocation entry, whose invocation is
    /// the tool name alone; that entry may also carry its description after
    /// a single space (`tool Launch the app`), so in a prefixed block a
    /// gapless entry whose remainder starts with an uppercase letter is read
    /// as description only: commands are lowercase by convention, sentences
    /// are capitalized.
    pub fn command_text<'a>(&self, entry: &'a str) -> &'a str {
        let invocation = before_description_gap(entry.split('\t').next().unwrap_or(entry));
        if self.prefix.is_none() {
            return invocation;
        }
        let rest = invocation
            .split_once(char::is_whitespace)
            .map_or("", |(_, rest)| rest.trim_start());
        let has_description_gap = invocation.len() < entry.len();
        if !has_description_gap && rest.starts_with(char::is_uppercase) {
            return "";
        }
        rest
    }
}

/// Shared, lazily-parsed view over `<binary> --help`. Construct via
/// [`HelpOutput::probe`] in runner code, or [`HelpOutput::from_raw`] in tests.
pub struct HelpOutput {
    raw: String,
    /// File stem of the binary the probe spawned, `None` for text built
    /// via [`HelpOutput::from_raw`]. Joins the `Usage:` line's tool name as
    /// a prefix candidate when command blocks are parsed.
    binary_stem: Option<String>,
    flags: OnceLock<Vec<Flag>>,
    env_hints: OnceLock<Vec<EnvHint>>,
    command_blocks: OnceLock<Vec<CommandBlock>>,
    subcommands: OnceLock<Vec<String>>,
}

impl HelpOutput {
    /// Build a `HelpOutput` from captured help text. The primary seam for
    /// unit tests — pass a fixture string and exercise the parsers without
    /// spawning a binary. Does no parsing; every view is built on first use.
    pub fn from_raw(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            binary_stem: None,
            flags: OnceLock::new(),
            env_hints: OnceLock::new(),
            command_blocks: OnceLock::new(),
            subcommands: OnceLock::new(),
        }
    }

    /// Spawn `<binary> --help` via the shared `BinaryRunner` and capture its
    /// combined stdout+stderr. Returns an empty `HelpOutput` rather than an
    /// error on timeout/crash — a misbehaving `--help` is a signal the audit
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
                let mut parsed = Self::from_raw(raw);
                parsed.binary_stem = runner.binary_stem().map(str::to_string);
                Ok(parsed)
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

    /// Every command block found in the help surface, in order. Lazy + cached.
    pub fn command_blocks(&self) -> &[CommandBlock] {
        self.command_blocks.get_or_init(|| {
            let candidates = prefix_candidates(&self.raw, self.binary_stem.as_deref());
            parse_command_blocks(&self.raw, &candidates)
        })
    }

    /// Single-token top-level subcommand names read from every command
    /// block, deduplicated in order of first appearance. Lazy + cached.
    pub fn subcommands(&self) -> &[String] {
        self.subcommands
            .get_or_init(|| subcommand_names(self.command_blocks()))
    }

    /// Why [`HelpOutput::subcommands`] is empty, worded as what the parser
    /// observed so an audit never asserts the tool has no subcommands when it
    /// means none were parsed.
    pub fn missing_subcommands_reason(&self) -> &'static str {
        if self.command_blocks().is_empty() {
            "no command block found in --help output, so no subcommands were parsed"
        } else {
            "a command block was found in --help output but no subcommand names could be parsed from it"
        }
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
        let header = before_description_gap(trimmed);

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

/// The text before clap's two-space description gap: the flag header of a
/// flag line, the invocation of a command entry. The whole line when there
/// is no description on it.
fn before_description_gap(line: &str) -> &str {
    line.split("  ").next().unwrap_or(line)
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

/// Parse env-var bindings from help text using two complementary patterns.
///
/// Pattern 1 (clap-style `[env: FOO]`) and Pattern 2 (bash-style `$FOO` or
/// `TOOL_FOO` near a flag) are scanned independently, then merged and
/// deduped by var name. Duplicates within a single pattern are preserved
/// in Pattern 1's output but collapsed across patterns — callers that want
/// occurrence counts should inspect Pattern 1's raw output directly.
fn parse_env_hints(raw: &str) -> Vec<EnvHint> {
    let pattern1 = parse_env_hints_clap_style(raw);
    let pattern2 = env_hints_bash::parse_env_hints_bash_style(raw);

    // Dedup by var name. Pattern 1 wins when both patterns match the same
    // name — its signal (explicit clap annotation) is strictly stronger
    // than proximity-or-section inference, and the emitted `source` tag
    // reflects that provenance. Iteration order preserves Pattern 1's
    // occurrences first, so `seen.insert` keeps the clap-annotation hint.
    let mut seen: HashSet<String> = HashSet::new();
    let mut merged = Vec::new();
    for hint in pattern1.into_iter().chain(pattern2) {
        if seen.insert(hint.var.clone()) {
            merged.push(hint);
        }
    }
    merged
}

/// Pattern 1 — clap's `[env: FOO_BAR]` or `[env: FOO_BAR=<default>]`
/// annotations. Each occurrence becomes one `EnvHint` tagged with
/// [`EnvHintSource::ClapAnnotation`].
fn parse_env_hints_clap_style(raw: &str) -> Vec<EnvHint> {
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
                source: EnvHintSource::ClapAnnotation,
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

/// The tool's own name as the `Usage:` line states it: the first token after
/// `Usage:` on the same line, or on the next non-empty line when the header
/// stands alone (cobra, clap v3). A path is reduced to its final component.
fn usage_tool_name(raw: &str) -> Option<String> {
    let mut lines = raw.lines();
    while let Some(line) = lines.next() {
        let Some((label, rest)) = line.trim().split_once(':') else {
            continue;
        };
        if !label.eq_ignore_ascii_case("usage") {
            continue;
        }
        let candidate = rest.split_whitespace().next().or_else(|| {
            lines
                .find(|l| !l.trim().is_empty())
                .and_then(|l| l.split_whitespace().next())
        })?;
        let name = candidate.rsplit(['/', '\\']).next().unwrap_or(candidate);
        return name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            .then(|| name.to_string());
    }
    None
}

/// A block header is any line whose last word is `commands:` or
/// `subcommands:`, case-insensitively: clap's `Commands:`, cobra's
/// `Available Commands:`, and hand-written `Common commands:` alike.
fn is_command_header(trimmed: &str) -> bool {
    trimmed.split_whitespace().last().is_some_and(|last| {
        last.eq_ignore_ascii_case("commands:") || last.eq_ignore_ascii_case("subcommands:")
    })
}

/// The tokens a prefixed block's entries may lead with: the `Usage:` line's
/// tool name and the probe-supplied binary stem, deduplicated. Both are
/// needed because a usage line can lead with a launcher (`npx tool`,
/// `python -m tool`) or a suffixed file name (`tool.exe`) while the entries
/// lead with the bare tool name.
fn prefix_candidates(raw: &str, binary_stem: Option<&str>) -> Vec<String> {
    let mut candidates: Vec<String> = usage_tool_name(raw).into_iter().collect();
    if let Some(stem) = binary_stem
        && !candidates.iter().any(|c| c == stem)
    {
        candidates.push(stem.to_string());
    }
    candidates
}

/// Collect every command block: a header, then the indented lines that
/// follow it until a blank line or a non-indented line closes the block.
/// The block's first entry sets its entry indentation; a later line
/// indented more deeply than that continues the previous entry (a wrapped
/// description, a nested subcommand) and is not recorded. Headers with no
/// entries are dropped.
fn parse_command_blocks(raw: &str, candidates: &[String]) -> Vec<CommandBlock> {
    let mut blocks: Vec<CommandBlock> = Vec::new();
    let mut entry_indent: Option<usize> = None;
    let mut in_block = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if is_command_header(trimmed) {
            blocks.push(CommandBlock {
                header: trimmed.to_string(),
                entries: Vec::new(),
                prefix: None,
            });
            entry_indent = None;
            in_block = true;
        } else if !in_block {
            continue;
        } else if trimmed.is_empty() || !line.starts_with(char::is_whitespace) {
            in_block = false;
        } else if let Some(block) = blocks.last_mut() {
            let indent = line.chars().take_while(|c| c.is_whitespace()).count();
            let first = *entry_indent.get_or_insert(indent);
            if indent <= first {
                block.entries.push(trimmed.to_string());
            }
        }
    }
    blocks.retain(|block| !block.entries.is_empty());
    for block in &mut blocks {
        block.prefix = shared_tool_prefix(&block.entries, candidates);
    }
    blocks
}

/// The candidate every entry in the block leads with, the hand-written shape
/// `herdr status` / `herdr server stop`. Agreement has to be unanimous so a
/// command that merely shares the tool's name is not eaten, and it is judged
/// per block so a one-entry block strips on its own. Candidates are tried in
/// order, so the `Usage:` line's name wins over the binary stem when both
/// would match.
fn shared_tool_prefix(entries: &[String], candidates: &[String]) -> Option<String> {
    candidates
        .iter()
        .find(|candidate| {
            entries
                .iter()
                .all(|entry| entry.split_whitespace().next() == Some(candidate.as_str()))
        })
        .cloned()
}

/// Single-token top-level names: the first token of each entry after any
/// shared prefix, validated by [`is_subcommand_name`] and deduplicated in
/// order of first appearance. A nested entry (`server stop`) contributes
/// `server`; an entry the prefix strip leaves empty contributes nothing.
fn subcommand_names(blocks: &[CommandBlock]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for block in blocks {
        for entry in &block.entries {
            let Some(name) = block.command_text(entry).split_whitespace().next() else {
                continue;
            };
            if is_subcommand_name(name) && !out.iter().any(|seen| seen == name) {
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
  audit        Run audits against a CLI project or binary
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

    // Pattern 2 fixtures (GH_HELP, RIPGREP_PROSE_HELP) and Pattern 2
    // tests live alongside the Pattern 2 code in
    // `runner/help_probe/env_hints_bash.rs`. Parent keeps only Pattern 1
    // + shared-fixture tests.

    // Hand-written help modeled on herdr: a `Usage:` line naming the tool,
    // two `... commands:` blocks, and every entry led by the tool name.
    const HERDR_HELP: &str = r#"herdr — terminal workspace manager for AI coding agents

Usage: herdr [options]
       herdr --session <name> [options]
       herdr server stop

Common commands:
  herdr                            Launch or attach to the persistent session
  herdr status [server|client]     Show local client and running server status
  herdr update                     Download and install the latest version
  herdr completion zsh             Generate shell completions for zsh
  herdr server stop                Stop the running server via the API socket
  herdr channel set <stable|preview> Choose the stable or preview update channel
  herdr server reload-config       Reload config.toml in the running server
  herdr config reset-keys          Back up config.toml and remove custom keybindings
  herdr channel <subcommand>       Manage the stable or preview update channel
  herdr machine <subcommand>       Manage saved SSH machines
  herdr api <subcommand>           Inspect socket API metadata and live runtime state

Advanced commands:
  herdr server                     Run as headless server

Options:
  --session <name>    Use or create a named persistent session
  --version, -V       Print version and exit
  --help, -h          Show this help
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
    fn pattern1_existing_behavior_unchanged() {
        // Regression guard: the Pattern 1 fixtures that shipped in v0.1.2
        // must still produce the same hits post-widening. Pattern 2
        // specific coverage lives in env_hints_bash::tests.
        let rg = parse_env_hints(RIPGREP_HELP);
        assert!(rg.iter().any(|h| h.var == "RIPGREP_COLOR"));
        let clap = parse_env_hints(CLAP_HELP);
        assert!(clap.iter().any(|h| h.var == "AGENTNATIVE_QUIET"));
    }

    #[test]
    fn pattern1_tags_hints_as_clap_annotation() {
        // Every Pattern 1 emission must carry EnvHintSource::ClapAnnotation.
        let rg = parse_env_hints(RIPGREP_HELP);
        let hint = rg
            .iter()
            .find(|h| h.var == "RIPGREP_COLOR")
            .expect("RIPGREP_COLOR in Pattern 1 hits");
        assert_eq!(hint.source, EnvHintSource::ClapAnnotation);
    }

    #[test]
    fn parse_subcommands_reads_commands_block() {
        let help = HelpOutput::from_raw(CLAP_HELP);
        assert_eq!(
            help.subcommands(),
            ["audit", "completions", "generate", "help"]
        );
        let [block] = help.command_blocks() else {
            panic!("expected one block, got {:?}", help.command_blocks());
        };
        assert_eq!(block.header, "Commands:");
        assert_eq!(block.prefix, None);
    }

    #[test]
    fn single_entry_clap_block_is_not_read_as_prefixed() {
        // Unanimity is trivial in a one-entry block; the tool name is what
        // distinguishes `audit  Run audits` from `herdr status`.
        let help =
            HelpOutput::from_raw("Usage: tool <COMMAND>\n\nCommands:\n  audit  Run audits\n");
        assert_eq!(help.subcommands(), ["audit"]);
        assert_eq!(help.command_blocks()[0].prefix, None);
    }

    #[test]
    fn hand_written_block_records_prefix_and_entries() {
        let help = HelpOutput::from_raw(HERDR_HELP);
        let blocks = help.command_blocks();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].header, "Common commands:");
        assert_eq!(blocks[0].prefix.as_deref(), Some("herdr"));
        assert_eq!(blocks[0].entries.len(), 11);
        assert_eq!(
            blocks[0].entries[0],
            "herdr                            Launch or attach to the persistent session"
        );
        assert_eq!(blocks[0].command_text(&blocks[0].entries[0]), "");
        assert_eq!(
            blocks[0].command_text(&blocks[0].entries[1]),
            "status [server|client]"
        );
        assert_eq!(blocks[1].header, "Advanced commands:");
        assert_eq!(blocks[1].prefix.as_deref(), Some("herdr"));
        assert_eq!(
            blocks[1].entries,
            ["herdr server                     Run as headless server"]
        );
    }

    #[test]
    fn nested_and_placeholder_entries_contribute_top_level_name_only() {
        let help = HelpOutput::from_raw(HERDR_HELP);
        let subs = help.subcommands();
        assert!(
            subs.iter().all(|s| !s.contains(['<', '[', '>', ']'])),
            "{subs:?}"
        );
        assert!(
            !subs
                .iter()
                .any(|s| s == "stop" || s == "set" || s == "herdr"),
            "{subs:?}"
        );
    }

    #[test]
    fn block_without_a_shared_prefix_is_read_as_written() {
        let help = HelpOutput::from_raw(
            "Usage: tool <COMMAND>\n\nCommon commands:\n  status   Show status\n  tool-up  Bring the tool up\n",
        );
        assert_eq!(help.subcommands(), ["status", "tool-up"]);
        assert_eq!(help.command_blocks()[0].prefix, None);
    }

    #[test]
    fn command_that_shares_the_tool_name_breaks_unanimity() {
        let help = HelpOutput::from_raw(
            "Usage: tool <COMMAND>\n\nCommands:\n  tool     Run the tool\n  status   Show status\n",
        );
        assert_eq!(help.subcommands(), ["tool", "status"]);
        assert_eq!(help.command_blocks()[0].prefix, None);
    }

    #[test]
    fn cobra_style_header_and_usage_on_next_line() {
        let help = HelpOutput::from_raw(
            "Usage:\n  kubectl [command]\n\nAvailable Commands:\n  apply   Apply a configuration\n  get     Display resources\n\nFlags:\n  -h, --help  help\n",
        );
        assert_eq!(help.subcommands(), ["apply", "get"]);
        assert_eq!(help.command_blocks()[0].header, "Available Commands:");
    }

    #[test]
    fn usage_tool_name_reads_the_usage_line() {
        assert_eq!(usage_tool_name(HERDR_HELP).as_deref(), Some("herdr"));
        assert_eq!(usage_tool_name(CLAP_HELP).as_deref(), Some("anc"));
        assert_eq!(
            usage_tool_name("USAGE:\n    anc <SUBCOMMAND>\n").as_deref(),
            Some("anc")
        );
        assert_eq!(
            usage_tool_name("Usage: ./target/debug/anc <COMMAND>\n").as_deref(),
            Some("anc")
        );
        assert_eq!(usage_tool_name("Usage: [OPTIONS]\n"), None);
        assert_eq!(usage_tool_name(NON_ENGLISH_HELP), None);
    }

    #[test]
    fn probe_falls_back_to_the_binary_name_without_a_usage_line() {
        use crate::audits::behavioral::tests::test_project_with_sh_script;
        let project = test_project_with_sh_script(
            "echo 'Common commands:'; echo '  test status   Show status'; echo '  test server   Run the server'",
        );
        let help = project.help_output().expect("probe succeeds");
        assert_eq!(help.subcommands(), ["status", "server"]);
    }

    #[test]
    fn missing_subcommands_reason_distinguishes_no_block_from_unparsed_block() {
        let none = HelpOutput::from_raw(BARE_HELP);
        assert!(none.subcommands().is_empty());
        assert!(
            none.missing_subcommands_reason()
                .contains("no command block")
        );

        let unparsed = HelpOutput::from_raw("Usage: tool\n\nCommands:\n  <name>   A placeholder\n");
        assert!(unparsed.subcommands().is_empty());
        assert!(
            unparsed
                .missing_subcommands_reason()
                .contains("but no subcommand names")
        );
    }

    #[test]
    fn hand_written_block_yields_top_level_names_without_the_tool_prefix() {
        let help = HelpOutput::from_raw(HERDR_HELP);
        assert_eq!(
            help.subcommands(),
            [
                "status",
                "update",
                "completion",
                "server",
                "channel",
                "config",
                "machine",
                "api",
            ]
        );
    }

    #[test]
    fn wrapped_description_continuation_keeps_the_prefixed_block_intact() {
        let wrapped = HERDR_HELP.replace(
            "  herdr status [server|client]     Show local client and running server status\n",
            "  herdr status [server|client]     Show local client and running server\n                                   status, including uptime\n",
        );
        assert_ne!(wrapped, HERDR_HELP, "fixture line was rewritten");
        let help = HelpOutput::from_raw(&wrapped);
        assert_eq!(
            help.subcommands(),
            HelpOutput::from_raw(HERDR_HELP).subcommands()
        );
        assert!(!help.subcommands().iter().any(|s| s == "herdr"));
        assert_eq!(help.command_blocks()[0].entries.len(), 11);
    }

    #[test]
    fn deeper_indented_nested_lines_are_continuations_of_their_parent_entry() {
        let help = HelpOutput::from_raw(
            "Usage: tool <COMMAND>\n\nCommands:\n  server   Manage\n    start   Start it\n    stop    Stop it\n",
        );
        assert_eq!(help.subcommands(), ["server"]);
        assert_eq!(help.command_blocks()[0].entries, ["server   Manage"]);
    }

    #[test]
    fn probe_strips_the_binary_stem_when_the_usage_line_leads_with_a_launcher() {
        use crate::audits::behavioral::tests::test_project_with_sh_script;
        let project = test_project_with_sh_script(
            "echo 'Usage: npx test [options]'; echo ''; echo 'Commands:'; echo '  test status   Show status'; echo '  test server   Run the server'",
        );
        let help = project.help_output().expect("probe succeeds");
        assert_eq!(help.subcommands(), ["status", "server"]);
        assert_eq!(help.command_blocks()[0].prefix.as_deref(), Some("test"));
    }

    #[test]
    fn single_space_bare_entry_in_a_prefixed_block_is_description_only() {
        let help = HelpOutput::from_raw(
            "Usage: tool [options]\n\nCommands:\n  tool Launch the app\n  tool status  Show status\n",
        );
        assert_eq!(help.subcommands(), ["status"]);
        let block = &help.command_blocks()[0];
        assert_eq!(block.prefix.as_deref(), Some("tool"));
        assert_eq!(block.command_text(&block.entries[0]), "");
        assert_eq!(block.command_text(&block.entries[1]), "status");
    }

    #[test]
    fn parse_subcommands_empty_without_block() {
        let help = HelpOutput::from_raw(BARE_HELP);
        assert!(help.subcommands().is_empty());
        assert!(help.command_blocks().is_empty());
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
        assert!(
            HelpOutput::from_raw(NON_ENGLISH_HELP)
                .subcommands()
                .is_empty()
        );
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
