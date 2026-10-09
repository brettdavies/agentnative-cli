//! Pattern 2 env-hint detection — bash-style `$FOO` / `TOOL_FOO` tokens
//! near flag definitions, or inside dedicated `ENVIRONMENT` sections.
//!
//! Extracted from `help_probe::mod` as a sibling submodule so the Pattern
//! 2 machinery (its constants, helpers, and ~9 tests) doesn't crowd the
//! orchestrator. Parent module's [`parse_env_hints`] calls
//! [`parse_env_hints_bash_style`] as the second of two merge inputs;
//! deduplication and source-tagging logic live on the parent side.
//!
//! The compounded learning behind these mitigations is
//! `docs/solutions/best-practices/cli-env-var-shape-heuristic-2026-04-21.md`
//! — edit both when widening detection.
//!
//! See [`super::EnvHint`] for the emitted shape and
//! [`super::EnvHintSource`] for the provenance tag each pattern attaches.

use std::borrow::Cow;
use std::collections::HashSet;

use super::flags::{definition_lines, is_section_heading};
use super::{EnvHint, EnvHintSource};

/// Shell/system env vars we never want to flag as flag-bound. Tools
/// routinely mention these in help prose (`respects $PAGER`, `uses $HOME`)
/// without those being user-configurable flag bindings. Listing them as
/// env hints would reward tools for documenting the ambient shell.
const SHELL_ENV_BLACKLIST: &[&str] = &[
    "PATH", "HOME", "USER", "SHELL", "PWD", "LANG", "TERM", "TMPDIR", "PAGER",
];

/// Window size (in lines, each direction) for Pattern 2's proximity scan
/// around a flag definition. Four lines is enough to catch wrapped
/// descriptions and the usual `[env: ...]`-adjacent prose without
/// reaching into unrelated flag entries.
const PATTERN2_WINDOW: usize = 4;

/// Pattern 2 — bash-style `$FOO` or uppercase `FOO_BAR` tokens that
/// co-occur with a flag definition within a ±4-line window, OR appear in
/// a dedicated `ENVIRONMENT` / `ENV VARS` section. Filters the
/// shell-environment blacklist and requires tool-scoped shape (uppercase,
/// digits, underscores; length ≥ 3).
///
/// A definition's own names and placeholders are not scanned:
/// `--iconv=CONVERT_SPEC` names a value, not a variable the tool reads. A
/// `$NAME` among them is.
///
/// Strips `[env: ...]` annotations before scanning — those belong to
/// Pattern 1. Salvaging tokens from rejected annotations (e.g.,
/// `[env: 1ABC]` contributing `ABC`) would undermine Pattern 1's
/// decision to reject them.
///
/// Each emitted hint carries a [`EnvHintSource`] tag distinguishing the
/// proximity-window path from the ENVIRONMENT-section path, so agents
/// debugging a false positive can see which branch matched.
pub(super) fn parse_env_hints_bash_style(raw: &str) -> Vec<EnvHint> {
    let stripped = strip_clap_env_annotations(raw);
    let lines: Vec<&str> = stripped.lines().collect();
    let definitions = definition_lines(&stripped);
    let env_section_range = find_env_section(&lines);

    // Track each line's matching source. ENV-section wins over
    // Proximity when both fire — a dedicated `ENVIRONMENT` header is a
    // structured, explicit signal, whereas flag-adjacent prose is
    // weaker (just "the token showed up near a flag line"). Apply
    // Proximity first, then overwrite with EnvSection where it applies.
    let mut line_source: Vec<Option<EnvHintSource>> = vec![None; lines.len()];
    for definition in &definitions {
        let lo = definition.index.saturating_sub(PATTERN2_WINDOW);
        let hi = (definition.index + PATTERN2_WINDOW + 1).min(lines.len());
        for slot in line_source[lo..hi].iter_mut() {
            *slot = Some(EnvHintSource::Proximity);
        }
    }
    if let Some((lo, hi)) = env_section_range {
        for slot in line_source[lo..hi].iter_mut() {
            *slot = Some(EnvHintSource::EnvSection);
        }
    }

    let mut hints = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(source) = line_source[i] else {
            continue;
        };
        let header_len = definitions
            .iter()
            .find(|definition| definition.index == i)
            .and_then(|definition| definition.header_len);
        for token in extract_env_tokens(&without_header(line, header_len)) {
            if SHELL_ENV_BLACKLIST.contains(&token.as_str()) {
                continue;
            }
            if seen.insert(token.clone()) {
                hints.push(EnvHint { var: token, source });
            }
        }
    }
    hints
}

/// `line` with the names and placeholders of a definition blanked, so a
/// value placeholder (`--iconv=CONVERT_SPEC`) is not read as a variable the
/// tool reads. A `$NAME` among them stays (`--token=$GITHUB_TOKEN`), and so
/// does a variable named in the description after them.
fn without_header(line: &str, header_len: Option<usize>) -> Cow<'_, str> {
    let Some(len) = header_len else {
        return Cow::Borrowed(line);
    };
    let mut out = String::with_capacity(line.len());
    let mut in_variable = false;
    for c in line[..len].chars() {
        in_variable = c == '$' || (in_variable && is_variable_char(c));
        out.push(if in_variable { c } else { ' ' });
    }
    out.push_str(&line[len..]);
    Cow::Owned(out)
}

fn is_variable_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '{' | '}')
}

/// Replace `[env: ...]` annotations with spaces so Pattern 2 doesn't
/// re-scan Pattern 1's territory. Space-padding (rather than deletion)
/// keeps line indices and column offsets stable — useful for future
/// callers that want to map tokens back to raw positions.
fn strip_clap_env_annotations(raw: &str) -> String {
    const TAG: &str = "[env:";
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(pos) = rest.find(TAG) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let close = after.find(']').map(|i| i + 1).unwrap_or(after.len());
        // Pad with spaces of the same width, preserving any newlines so
        // line-by-line iteration downstream sees the same line count.
        for ch in after[..close].chars() {
            out.push(if ch == '\n' { '\n' } else { ' ' });
        }
        rest = &after[close..];
    }
    out.push_str(rest);
    out
}

/// Locate an `ENVIRONMENT` / `ENV VARS` / `ENVIRONMENT VARIABLES` section
/// in the help output, returning the line-index range (exclusive upper).
/// Tools like `gh` use this convention; `ripgrep` uses free prose instead
/// and is caught by the flag-proximity window above.
fn find_env_section(lines: &[&str]) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| {
        let t = l.trim();
        matches!(
            t,
            "ENVIRONMENT"
                | "ENVIRONMENT:"
                | "ENVIRONMENT VARIABLES"
                | "ENVIRONMENT VARIABLES:"
                | "ENV VARS"
                | "ENV VARS:"
                | "Environment:"
        )
    })?;
    // Section ends at the next top-level header or end-of-text.
    let end = lines[start + 1..]
        .iter()
        .position(|l| is_section_heading(l))
        .map(|offset| start + 1 + offset)
        .unwrap_or(lines.len());
    Some((start, end))
}

/// Pull uppercase-identifier tokens from a single line. Accepts bare
/// `FOO_BAR`, `$FOO`, or `${FOO}` shapes. Tokens must be at least 3 chars
/// long AND either be `$`-prefixed OR contain an underscore — this is
/// how the parser distinguishes tool-scoped env vars (`RIPGREP_CONFIG`,
/// `$PATH`) from placeholders that litter help text (`OPTIONS`,
/// `[COMMAND]`, `HTTP`).
///
/// Also rejects tokens immediately wrapped in `[FOO]` or `<FOO>` when
/// they lack a `$` prefix — clap's usage syntax uses these for argument
/// placeholders, not env-var references.
///
/// **Recall gap on bracketed ENV-section prose.** Some tools document
/// env bindings inside an ENVIRONMENT section using `[VAR]` syntax
/// (e.g., `"Uses [GH_TOKEN] when set."`). The bracket-rejection above
/// loses these. Adding a `$`-prefix escape would over-match placeholder
/// usage elsewhere; the conservative choice is to accept the recall
/// gap and wait for a concrete tool whose ENV section needs this.
///
/// Section-header-shaped lines (e.g., `DOCKER_CONFIG:`) are excluded
/// entirely: a header that happens to contain an underscored uppercase
/// token is not a prose reference to an env var.
fn extract_env_tokens(line: &str) -> Vec<String> {
    if is_section_heading(line) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip leading `$` and optional `{` for `$FOO` / `${FOO}` forms.
        let mut start = i;
        let mut had_dollar = false;
        if bytes[i] == b'$' {
            had_dollar = true;
            start = i + 1;
            if start < bytes.len() && bytes[start] == b'{' {
                start += 1;
            }
        }
        // Collect an identifier: [A-Z_][A-Z0-9_]*
        if start < bytes.len() && (bytes[start].is_ascii_uppercase() || bytes[start] == b'_') {
            let mut end = start + 1;
            while end < bytes.len()
                && (bytes[end].is_ascii_uppercase()
                    || bytes[end].is_ascii_digit()
                    || bytes[end] == b'_')
            {
                end += 1;
            }
            let candidate = &line[start..end];

            // Reject lowercase-adjacent matches (CamelCase / MACROname).
            let left_ok = start == 0 || !bytes[start - 1].is_ascii_lowercase();
            let right_ok = end >= bytes.len() || !bytes[end].is_ascii_lowercase();

            // Reject `[FOO]` or `<FOO>` placeholders unless `$`-prefixed —
            // those are clap usage placeholders, not env-var references.
            let in_placeholder_bracket = !had_dollar
                && matches!(
                    start.checked_sub(1).map(|p| bytes[p]),
                    Some(b'[') | Some(b'<')
                )
                && matches!(bytes.get(end).copied(), Some(b']') | Some(b'>'));

            // Require tool-scoped shape: `$`-prefixed or contains `_`.
            // This is the defining filter that separates real env vars
            // (`RIPGREP_CONFIG_PATH`, `$PATH`) from placeholders like
            // `OPTIONS`, `COMMAND`, `HTTP`, `FILES`.
            let is_tool_scoped = had_dollar || candidate.contains('_');

            // `is_env_var_name(candidate)` would be redundant here —
            // the greedy scan above only admits `[A-Z_][A-Z0-9_]*` bytes,
            // so the candidate is already a valid env-var name by
            // construction.
            if left_ok
                && right_ok
                && !in_placeholder_bracket
                && is_tool_scoped
                && candidate.len() >= 3
            {
                out.push(candidate.to_string());
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::parse_env_hints;
    use super::*;

    const GH_HELP: &str = "\
Work seamlessly with GitHub from the command line.

USAGE
  gh <command> <subcommand> [flags]

OPTIONS
  --help   Show help for command.

ENVIRONMENT VARIABLES
  GH_TOKEN, GITHUB_TOKEN: authentication credentials.
  GH_REPO: specifies default repo for commands.
";

    const RIPGREP_PROSE: &str = "\
USAGE: rg [OPTIONS] PATTERN

OPTIONS:
  --config <FILE>
          Use config file. If set to an empty string, RIPGREP_CONFIG_PATH
          is read from $RIPGREP_CONFIG_PATH. Respects RIPGREP_COLOR env
          variable when rendering output.
";

    fn names(raw: &str) -> Vec<String> {
        parse_env_hints_bash_style(raw)
            .into_iter()
            .map(|hint| hint.var)
            .collect()
    }

    #[test]
    fn a_flag_s_own_placeholder_is_not_an_environment_variable() {
        // rsync 3.5.1, and a bare placeholder after a short name.
        for help in [
            "Options\n--iconv=CONVERT_SPEC     request charset conversion of filenames\n--checksum-seed=NUM      set block/file checksum seed (advanced)\n",
            "Options:\n  -o OUTPUT_FILE    Write the result here\n  -h, --help        Show help\n",
        ] {
            assert_eq!(names(help), Vec::<String>::new(), "{help}");
        }
        assert_eq!(
            names(&crate::runner::help_probe::fixture_snapshots::fixture(
                "rsync__--help.txt"
            )),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_variable_beside_the_names_still_counts() {
        assert_eq!(
            names("Options:\n      --token=$GITHUB_TOKEN   API token\n"),
            ["GITHUB_TOKEN"]
        );
        assert_eq!(
            names("Options:\n      --token ${GITHUB_TOKEN}   API token\n"),
            ["GITHUB_TOKEN"]
        );
    }

    #[test]
    fn a_placeholder_in_a_column_of_its_own_is_not_a_variable() {
        assert!(
            names("Options:\n  --config-file  -c  CONFIG_FILE  Path to the config\n").is_empty()
        );
    }

    #[test]
    fn a_tab_indented_line_keeps_the_variable_its_description_names() {
        assert_eq!(
            names("Options:\n\t-t TOKEN  reads API_TOKEN\n"),
            ["API_TOKEN"]
        );
    }

    #[test]
    fn a_variable_named_in_a_description_still_counts() {
        assert_eq!(
            names(
                "Options:\n      --config FILE    Config file (default: TOOL_CONFIG, then $XDG_CONFIG_HOME)\n"
            ),
            ["TOOL_CONFIG", "XDG_CONFIG_HOME"]
        );
        assert_eq!(
            names("Options:\n      --token GITHUB_TOKEN    Token; also read from GITHUB_TOKEN\n"),
            ["GITHUB_TOKEN"]
        );
    }

    #[test]
    fn pattern2_captures_gh_environment_section() {
        let hints = parse_env_hints(GH_HELP);
        let names: Vec<&str> = hints.iter().map(|h| h.var.as_str()).collect();
        assert!(names.contains(&"GH_TOKEN"), "got {names:?}");
        assert!(names.contains(&"GITHUB_TOKEN"), "got {names:?}");
        assert!(names.contains(&"GH_REPO"), "got {names:?}");
    }

    #[test]
    fn pattern2_captures_ripgrep_prose_near_flag() {
        // RIPGREP_CONFIG_PATH appears in the description of --config, within
        // the 4-line window.
        let hints = parse_env_hints(RIPGREP_PROSE);
        let names: Vec<&str> = hints.iter().map(|h| h.var.as_str()).collect();
        assert!(names.contains(&"RIPGREP_CONFIG_PATH"), "got {names:?}");
        assert!(names.contains(&"RIPGREP_COLOR"), "got {names:?}");
    }

    #[test]
    fn pattern2_blacklist_rejects_shell_env() {
        // $PATH, $HOME, $PAGER in flag prose must not become an EnvHint.
        // PAGER is the archetypal example named in the SHELL_ENV_BLACKLIST
        // doc comment — tools like git and gh document "respects $PAGER"
        // as ambient shell behavior, not as a flag-bound env var.
        let src = "\
USAGE: foo
OPTIONS:
      --bin    Runs the binary from $PATH. Use $HOME to override.
      --less   Pipes output through $PAGER when stdout is a TTY.
";
        let hints = parse_env_hints(src);
        let names: Vec<&str> = hints.iter().map(|h| h.var.as_str()).collect();
        assert!(!names.contains(&"PATH"));
        assert!(!names.contains(&"HOME"));
        assert!(
            !names.contains(&"PAGER"),
            "$PAGER must stay in SHELL_ENV_BLACKLIST — it's ambient shell, \
             not a flag binding (overlaps with p6-no-pager signalling)",
        );
    }

    #[test]
    fn pattern2_ignores_tokens_outside_flag_window() {
        // MYVAR appears far from any flag and there's no ENVIRONMENT header
        // — must not be captured.
        let src = "\
MyTool - does stuff.

See also: MYVAR is described in the CONFIG manual, page 42.

Completely unrelated paragraph with no flags in sight. MYVAR again.

Another paragraph.

Another paragraph.

Another paragraph.

Another paragraph.
";
        let hints = parse_env_hints(src);
        let names: Vec<&str> = hints.iter().map(|h| h.var.as_str()).collect();
        assert!(
            !names.contains(&"MYVAR"),
            "MYVAR outside flag window should be ignored, got {names:?}",
        );
    }

    #[test]
    fn pattern2_dedupes_against_pattern1() {
        // A flag documented with BOTH `[env: FOO]` AND a bash-style mention
        // of $FOO in prose must produce exactly one EnvHint for FOO.
        let src = "\
USAGE: tool [OPTIONS]

OPTIONS:
      --foo <VAL>    Configures foo. See $MY_FOO for details. [env: MY_FOO]
";
        let hints = parse_env_hints(src);
        let my_foo_count = hints.iter().filter(|h| h.var == "MY_FOO").count();
        assert_eq!(
            my_foo_count, 1,
            "expected MY_FOO deduped across patterns, got {hints:?}",
        );
    }

    #[test]
    fn pattern2_ignores_section_header_lines() {
        // A section header that happens to be shaped like a tool-scoped
        // env var (underscored uppercase, ends with `:`) must not be
        // captured as a hint — it's prose structure, not a reference.
        let src = "\
USAGE: tool

OPTIONS:
      --flag    Does something.

DOCKER_CONFIG:
      /etc/tool/docker.conf
";
        let hints = parse_env_hints(src);
        let names: Vec<&str> = hints.iter().map(|h| h.var.as_str()).collect();
        assert!(
            !names.contains(&"DOCKER_CONFIG"),
            "section-header line must not contribute env hints, got {names:?}",
        );
    }

    #[test]
    fn pattern2_rejects_mixed_case_and_placeholders_near_flag() {
        // Negative shapes that should never produce hints:
        //   - `$Path`          — mixed case; not tool-scoped
        //   - `[FILES]`        — clap placeholder bracket form, no $
        //   - `<TEMPLATE>`     — clap angle-bracket placeholder, no $
        //   - `CamelCase`      — not uppercase-only, not tool-scoped
        //   - `MACROname`      — leading capital but not tool-scoped
        let src = "\
USAGE: tool [OPTIONS] [FILES]

OPTIONS:
      --tpl <TEMPLATE>   Uses CamelCase naming. Read $Path then MACROname.
";
        let hints = parse_env_hints(src);
        let names: Vec<&str> = hints.iter().map(|h| h.var.as_str()).collect();
        for rejected in ["Path", "FILES", "TEMPLATE", "CamelCase", "MACROname"] {
            assert!(
                !names.contains(&rejected),
                "{rejected} must be rejected by Pattern 2, got {names:?}",
            );
        }
    }

    #[test]
    fn pattern2_rejects_bracketed_env_vars_in_prose() {
        // Some tools document env bindings as `[GH_TOKEN]` or
        // `<GITHUB_TOKEN>` inside an ENVIRONMENT section rather than as
        // bare tokens. Current Pattern 2 treats bracket-wrapped
        // uppercase tokens as clap usage placeholders and rejects them,
        // accepting a small recall gap.
        //
        // This test locks the current rejection behavior. Widening
        // Pattern 2 to accept bracketed ENV-section tokens (e.g., by
        // relaxing the `in_placeholder_bracket` check inside an ENV
        // section window) must update this test deliberately — and
        // remove the recall-gap note from `extract_env_tokens`'s
        // doc comment. The regression surface is: "Pattern 2 started
        // matching `[FOO]` everywhere" vs. "Pattern 2 matches `[FOO]`
        // only inside ENVIRONMENT sections".
        let src = "\
USAGE: tool

ENVIRONMENT:
  Uses [GH_TOKEN] when set.
  Also respects <GITHUB_TOKEN>.
";
        let hints = parse_env_hints(src);
        let names: Vec<&str> = hints.iter().map(|h| h.var.as_str()).collect();
        assert!(
            !names.contains(&"GH_TOKEN"),
            "bracketed [GH_TOKEN] must stay rejected; see \
             extract_env_tokens doc comment (recall gap noted)",
        );
        assert!(
            !names.contains(&"GITHUB_TOKEN"),
            "angle-bracket <GITHUB_TOKEN> must stay rejected",
        );
    }

    #[test]
    fn pattern2_tags_source_for_each_hint() {
        // ENV-section matches wire up as EnvSection; proximity-only
        // matches wire up as Proximity. When both apply, EnvSection wins
        // — a dedicated header is a stronger structured signal.
        let hints = parse_env_hints(GH_HELP);
        let gh_token = hints
            .iter()
            .find(|h| h.var == "GH_TOKEN")
            .expect("GH_TOKEN in hints");
        assert_eq!(gh_token.source, EnvHintSource::EnvSection);

        let hints = parse_env_hints(RIPGREP_PROSE);
        let rg_color = hints
            .iter()
            .find(|h| h.var == "RIPGREP_COLOR")
            .expect("RIPGREP_COLOR in hints");
        assert_eq!(rg_color.source, EnvHintSource::Proximity);
    }
}
