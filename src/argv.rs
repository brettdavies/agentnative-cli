//! Pre-parse argv transformation that inserts `check` as the implicit default
//! subcommand. Lives separately so `main.rs` stays focused on orchestration
//! and so the injection logic is unit-testable in isolation.

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};

use crate::cli::Cli;

/// Inject `check` as the default subcommand when the first non-flag argument
/// is not a recognized subcommand.
///
/// Bare invocation (no args beyond the program name) is left untouched so
/// clap's `arg_required_else_help` still prints help and exits 2. This is a
/// non-negotiable fork-bomb guard: when agentnative dogfoods itself, a bare
/// spawn must not recurse into `check .`.
///
/// Flag-value pairing is essential: `anc --command check` must not be misread
/// as the explicit `check` subcommand just because `check` happens to follow a
/// value-taking flag. The scanner consults clap introspection to learn which
/// flags consume the next token.
pub fn inject_default_subcommand<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    let args: Vec<OsString> = args.into_iter().collect();
    if args.len() <= 1 {
        return args;
    }

    let cmd = <Cli as clap::CommandFactory>::command();

    // Known subcommand names (including aliases). Clap auto-generates a `help`
    // subcommand that is NOT returned by `get_subcommands()`, so add it
    // explicitly — otherwise `anc help` is treated as a path.
    let mut known: Vec<String> = cmd
        .get_subcommands()
        .flat_map(|sc| {
            std::iter::once(sc.get_name().to_string()).chain(sc.get_all_aliases().map(String::from))
        })
        .collect();
    known.push(String::from("help"));

    // Build two flag catalogues from clap introspection:
    //   - top_level_flags: long/short names defined on `Cli` itself
    //     (e.g. `--quiet`, `-q`, plus clap's auto `--help`/`--version`).
    //   - all_value_flags: every value-taking flag across `Cli` and every
    //     subcommand (e.g. `--command`, `--output`, `--principle`).
    // Any flag whose base name is missing from `top_level_flags` is
    // subcommand-scoped — its presence is a strong signal the user wants
    // the implicit `check` subcommand even if no positional arg follows.
    let top_level_flags: HashSet<String> = cmd
        .get_arguments()
        .filter(|a| !a.is_positional())
        .flat_map(|a| {
            let mut names = Vec::new();
            if let Some(l) = a.get_long() {
                names.push(format!("--{l}"));
            }
            if let Some(s) = a.get_short() {
                names.push(format!("-{s}"));
            }
            names
        })
        // Clap auto-generates these regardless of whether they appear in
        // get_arguments() at every version, so add them defensively.
        .chain(
            ["--help", "-h", "--version", "-V"]
                .into_iter()
                .map(String::from),
        )
        .collect();
    let mut all_value_flags: Vec<(Option<String>, Option<char>)> = Vec::new();
    let mut collect_value = |c: &clap::Command| {
        for arg in c.get_arguments().filter(|a| !a.is_positional()) {
            if matches!(
                arg.get_action(),
                clap::ArgAction::Set | clap::ArgAction::Append
            ) {
                all_value_flags.push((arg.get_long().map(String::from), arg.get_short()));
            }
        }
    };
    collect_value(&cmd);
    for sc in cmd.get_subcommands() {
        collect_value(sc);
    }

    // Reduce a flag token to its canonical base form for set membership.
    // `--flag` / `--flag=value` -> `--flag`. `-X` / `-Xvalue` -> `-X`.
    let base_form = |token: &str| -> Option<String> {
        if let Some(rest) = token.strip_prefix("--") {
            let name = rest.split('=').next().unwrap_or(rest);
            return Some(format!("--{name}"));
        }
        if let Some(rest) = token.strip_prefix('-') {
            return rest.chars().next().map(|c| format!("-{c}"));
        }
        None
    };

    let consumes_next = |token: &str| -> bool {
        // `--flag=value` carries the value with it; the next token is independent.
        if token.starts_with("--") && token.contains('=') {
            return false;
        }
        // `-Xvalue` (concatenated short flag) — same.
        if token.starts_with('-') && !token.starts_with("--") && token.len() > 2 {
            return false;
        }
        if let Some(rest) = token.strip_prefix("--") {
            return all_value_flags
                .iter()
                .any(|(l, _)| l.as_deref() == Some(rest));
        }
        if let Some(rest) = token.strip_prefix('-') {
            if let Some(c) = rest.chars().next().filter(|_| rest.len() == 1) {
                return all_value_flags.iter().any(|(_, s)| *s == Some(c));
            }
        }
        false
    };

    let inject_check = |args: Vec<OsString>| -> Vec<OsString> {
        let mut injected = Vec::with_capacity(args.len() + 1);
        injected.push(args[0].clone());
        injected.push(OsString::from("check"));
        injected.extend(args.into_iter().skip(1));
        injected
    };

    let mut i = 1;
    let mut saw_subcommand_flag = false;
    while i < args.len() {
        let token = args[i].to_string_lossy();

        // POSIX `--` separator: anything after is positional. Inject `check`
        // before it so clap routes the remaining tokens to the Check subcommand.
        if token == "--" {
            return if i + 1 >= args.len() {
                args
            } else {
                inject_check(args)
            };
        }

        if token.starts_with('-') {
            // Track whether this flag belongs to a subcommand rather than the
            // top-level Cli. If so, the user clearly intends `check` even when
            // no positional argument follows (e.g. `anc --command rg`).
            if let Some(base) = base_form(&token) {
                if !top_level_flags.contains(&base) {
                    saw_subcommand_flag = true;
                }
            }
            i += if consumes_next(&token) { 2 } else { 1 };
            continue;
        }

        return if known.iter().any(|k| k == &*token) {
            args
        } else {
            inject_check(args)
        };
    }

    // No non-flag token. Inject `check` if any subcommand-scoped flag appeared
    // (e.g. `anc --command rg`, `anc --output json`). Otherwise leave the args
    // alone so clap can handle bare `--help` / `--version` / `-q` natively.
    if saw_subcommand_flag {
        return inject_check(args);
    }

    args
}

/// Format a captured argv vector as a shell-quoted command string, suitable
/// for the scorecard's `run.invocation` field. Uses single-quote quoting:
/// args containing whitespace, single quotes, double quotes, or shell
/// metacharacters are wrapped in `'…'`, with embedded `'` escaped as
/// `'\''`. Lossy UTF-8 conversion is intentional — the field records what
/// the user typed for human review, not for byte-perfect replay.
///
/// Captured *before* `inject_default_subcommand` rewrites the args, so the
/// recorded command reflects user intent (`anc .` stays as `anc .`, not
/// `anc check .`).
pub fn format_invocation(args: &[OsString]) -> String {
    args.iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_arg(arg: &OsStr) -> String {
    let s = arg.to_string_lossy();
    if s.is_empty() {
        return "''".to_string();
    }
    if needs_quoting(&s) {
        // Single-quote everything; escape embedded single quotes by closing
        // the quoted run, emitting `\'`, and reopening — POSIX-shell idiom.
        let mut out = String::with_capacity(s.len() + 2);
        out.push('\'');
        for c in s.chars() {
            if c == '\'' {
                out.push_str("'\\''");
            } else {
                out.push(c);
            }
        }
        out.push('\'');
        out
    } else {
        s.into_owned()
    }
}

fn needs_quoting(s: &str) -> bool {
    s.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '\'' | '"'
                    | '\\'
                    | '$'
                    | '`'
                    | '|'
                    | '&'
                    | ';'
                    | '<'
                    | '>'
                    | '('
                    | ')'
                    | '{'
                    | '}'
                    | '*'
                    | '?'
                    | '['
                    | ']'
                    | '#'
                    | '~'
                    | '!'
            )
    })
}

#[cfg(test)]
mod tests {
    use super::{format_invocation, inject_default_subcommand};
    use std::ffi::OsString;

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    fn names(v: Vec<OsString>) -> Vec<String> {
        v.into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn bare_invocation_is_untouched() {
        // Fork-bomb guard: no injection for bare `anc`.
        let out = inject_default_subcommand(args(&["anc"]));
        assert_eq!(names(out), vec!["anc"]);
    }

    #[test]
    fn dot_path_gets_check_injected() {
        let out = inject_default_subcommand(args(&["anc", "."]));
        assert_eq!(names(out), vec!["anc", "check", "."]);
    }

    #[test]
    fn global_short_flag_before_path_gets_check_injected_in_canonical_position() {
        // `check` goes before the global flag so clap parses
        // ["anc", "check", "-q", "."] cleanly.
        let out = inject_default_subcommand(args(&["anc", "-q", "."]));
        assert_eq!(names(out), vec!["anc", "check", "-q", "."]);
    }

    #[test]
    fn global_long_flag_before_path_gets_check_injected() {
        let out = inject_default_subcommand(args(&["anc", "--quiet", "."]));
        assert_eq!(names(out), vec!["anc", "check", "--quiet", "."]);
    }

    #[test]
    fn explicit_check_subcommand_is_untouched() {
        let out = inject_default_subcommand(args(&["anc", "check", "."]));
        assert_eq!(names(out), vec!["anc", "check", "."]);
    }

    #[test]
    fn explicit_completions_subcommand_is_untouched() {
        let out = inject_default_subcommand(args(&["anc", "completions", "bash"]));
        assert_eq!(names(out), vec!["anc", "completions", "bash"]);
    }

    #[test]
    fn help_flag_alone_is_untouched() {
        // `anc --help` — no non-flag token, no injection.
        let out = inject_default_subcommand(args(&["anc", "--help"]));
        assert_eq!(names(out), vec!["anc", "--help"]);
    }

    #[test]
    fn version_flag_alone_is_untouched() {
        let out = inject_default_subcommand(args(&["anc", "--version"]));
        assert_eq!(names(out), vec!["anc", "--version"]);
    }

    #[test]
    fn quiet_flag_alone_is_untouched() {
        // `anc -q` with no path — `-q` is a top-level Cli flag, not a
        // subcommand flag, so we leave args alone and let the `None` arm
        // in `run()` print help and exit 2.
        let out = inject_default_subcommand(args(&["anc", "-q"]));
        assert_eq!(names(out), vec!["anc", "-q"]);
    }

    #[test]
    fn help_subcommand_passes_through() {
        // `anc help` — clap auto-generates the `help` subcommand. It is NOT
        // returned by `get_subcommands()` so we add it explicitly. Without
        // that, `help` would be misclassified as a path.
        let out = inject_default_subcommand(args(&["anc", "help"]));
        assert_eq!(names(out), vec!["anc", "help"]);
    }

    #[test]
    fn help_subcommand_with_target_passes_through() {
        let out = inject_default_subcommand(args(&["anc", "help", "check"]));
        assert_eq!(names(out), vec!["anc", "help", "check"]);
    }

    #[test]
    fn command_flag_value_matching_subcommand_name_is_paired() {
        // `anc --command check` — `check` is the value of `--command`, NOT the
        // explicit subcommand. The scanner pairs the value-taking flag with
        // its argument and proceeds to inject `check` (because `--command` is
        // a subcommand-scoped flag with no positional following).
        let out = inject_default_subcommand(args(&["anc", "--command", "check"]));
        assert_eq!(names(out), vec!["anc", "check", "--command", "check"]);
    }

    #[test]
    fn command_flag_with_no_positional_injects_check() {
        // `anc --command rg` — subcommand-scoped flag with no positional.
        // Without injection, clap would reject `--command` at the top level.
        let out = inject_default_subcommand(args(&["anc", "--command", "rg"]));
        assert_eq!(names(out), vec!["anc", "check", "--command", "rg"]);
    }

    #[test]
    fn output_flag_with_no_positional_injects_check() {
        // `anc --output json --source` — only flags, but `--output` and
        // `--source` are both subcommand-scoped, so inject `check`.
        let out = inject_default_subcommand(args(&["anc", "--output", "json", "--source"]));
        assert_eq!(
            names(out),
            vec!["anc", "check", "--output", "json", "--source"]
        );
    }

    #[test]
    fn equals_form_value_flag_is_recognized_as_subcommand_scoped() {
        // `anc --output=json --source` — equals form. The scanner classifies
        // `--output=json` as a single subcommand-scoped token (no separate
        // value to skip) and still injects `check`.
        let out = inject_default_subcommand(args(&["anc", "--output=json", "--source"]));
        assert_eq!(
            names(out),
            vec!["anc", "check", "--output=json", "--source"]
        );
    }

    #[test]
    fn principle_value_flag_pairs_with_numeric_value() {
        // `anc --principle 4` — `4` is the value, not a path candidate.
        let out = inject_default_subcommand(args(&["anc", "--principle", "4"]));
        assert_eq!(names(out), vec!["anc", "check", "--principle", "4"]);
    }

    #[test]
    fn double_dash_separator_injects_check_before_separator() {
        // `anc -- .` — POSIX `--` ends option parsing. Inject before it so
        // clap's `check` parser sees `-- .`.
        let out = inject_default_subcommand(args(&["anc", "--", "."]));
        assert_eq!(names(out), vec!["anc", "check", "--", "."]);
    }

    #[test]
    fn double_dash_alone_passes_through() {
        // `anc --` with nothing after — let clap handle it natively.
        let out = inject_default_subcommand(args(&["anc", "--"]));
        assert_eq!(names(out), vec!["anc", "--"]);
    }

    #[test]
    fn directory_path_gets_check_injected() {
        let out = inject_default_subcommand(args(&["anc", "/some/dir"]));
        assert_eq!(names(out), vec!["anc", "check", "/some/dir"]);
    }

    #[test]
    fn trailing_flags_pass_through() {
        let out = inject_default_subcommand(args(&["anc", ".", "--output", "json"]));
        assert_eq!(names(out), vec!["anc", "check", ".", "--output", "json"]);
    }

    // ---- format_invocation ----

    #[test]
    fn format_invocation_simple_args_unquoted() {
        let out = format_invocation(&args(&["anc", "check", "."]));
        assert_eq!(out, "anc check .");
    }

    #[test]
    fn format_invocation_pre_injection_user_intent_preserved() {
        // Plan R4 intent check: a user who typed `anc .` MUST see `anc .` in
        // the scorecard, not `anc check .` (which would be a fact about anc's
        // internals, not the user's command).
        let out = format_invocation(&args(&["anc", "."]));
        assert_eq!(out, "anc .");
    }

    #[test]
    fn format_invocation_arg_with_space_is_single_quoted() {
        let out = format_invocation(&args(&["anc", "check", "/tmp/with space/repo"]));
        assert_eq!(out, "anc check '/tmp/with space/repo'");
    }

    #[test]
    fn format_invocation_arg_with_single_quote_is_escaped() {
        // POSIX-shell escape: close, emit `\'`, reopen.
        let out = format_invocation(&args(&["anc", "check", "ab'cd"]));
        assert_eq!(out, "anc check 'ab'\\''cd'");
    }

    #[test]
    fn format_invocation_arg_with_metacharacters_is_quoted() {
        let out = format_invocation(&args(&["anc", "check", "$(rm -rf)"]));
        assert_eq!(out, "anc check '$(rm -rf)'");
    }

    #[test]
    fn format_invocation_empty_arg_renders_as_empty_quotes() {
        let out = format_invocation(&args(&["anc", ""]));
        assert_eq!(out, "anc ''");
    }

    #[test]
    fn format_invocation_double_quote_is_quoted() {
        let out = format_invocation(&args(&["anc", "say\"hi"]));
        assert_eq!(out, "anc 'say\"hi'");
    }

    #[test]
    fn format_invocation_round_trip_no_panic_on_invalid_utf8() {
        // Linux-only: build an OsString containing invalid UTF-8. Lossy
        // conversion must not panic; the field carries the lossy form.
        #[cfg(target_os = "linux")]
        {
            use std::ffi::OsString;
            use std::os::unix::ffi::OsStringExt;

            let invalid = OsString::from_vec(vec![0xff, 0xfe]);
            let out = format_invocation(&[OsString::from("anc"), invalid]);
            assert!(out.starts_with("anc "));
        }
    }
}
