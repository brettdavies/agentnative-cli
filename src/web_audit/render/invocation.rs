//! The command line a run started from, as a next step repeats it.

use std::ffi::OsString;

use super::failure::TOKEN_STDIN_ARGUMENT;
use crate::argv::{format_invocation, rerun_argv};

/// How a run was started. A next step keeps the caller's flags and form:
/// `--no-external-dns` stays set, and a bare target stays bare, which keeps
/// withholding the token that naming `web` would send.
#[derive(Clone, Copy, Debug)]
pub struct Invocation<'a> {
    /// The command line as typed.
    pub argv: &'a [OsString],
    /// The target as typed.
    pub target: &'a str,
    /// Whether `web` was typed, rather than a bare target routed here.
    pub web_typed: bool,
    /// Whether `AGENTNATIVE_QUIET` is set. Clap counts the variable as `--quiet`
    /// whatever its value, so `--verbose` beside it exits 2 as a conflict.
    pub quiet_env: bool,
}

impl Invocation<'_> {
    /// The typed line on `shown` in place of its target.
    fn typed(&self, shown: &str) -> Vec<OsString> {
        rerun_argv(self.argv)
            .into_iter()
            .map(|arg| {
                if arg == self.target {
                    OsString::from(shown)
                } else {
                    arg
                }
            })
            .collect()
    }

    /// [`Self::typed`] without `--token-stdin`, because a printed command cannot carry
    /// a stdin.
    fn words(&self, shown: &str) -> Vec<OsString> {
        let mut words = self.typed(shown);
        words.retain(|arg| arg != TOKEN_STDIN_ARGUMENT);
        words
    }

    /// Whether a next step leaves out a `--token-stdin` the run was given.
    pub(super) fn gave_token_stdin(&self) -> bool {
        self.argv.iter().any(|arg| arg == TOKEN_STDIN_ARGUMENT)
    }

    /// The line on `shown`, shell-quoted so it runs as printed.
    pub(super) fn on(&self, shown: &str) -> String {
        format_invocation(&self.words(shown))
    }

    /// The line on `shown` with `--verbose`, which lists every row once the site
    /// answers. `--quiet` comes out because the two conflict.
    pub(super) fn retry(&self, shown: &str) -> String {
        let mut words = self.words(shown);
        words.retain(|arg| arg != "-q" && arg != "--quiet");
        if !self.quiet_env && !words.iter().any(|arg| arg == "-v" || arg == "--verbose") {
            let at = end_of_flags(&words);
            words.insert(at, OsString::from("--verbose"));
        }
        format_invocation(&words)
    }

    /// The line on `shown` with a `--ca-cert` to fill in. `<FILE>` is a placeholder,
    /// so it stays unquoted.
    pub(super) fn ca_cert_template(&self, shown: &str) -> String {
        let words = self.words(shown);
        let (flags, operands) = words.split_at(end_of_flags(&words));
        let flags = format_invocation(flags);
        if operands.is_empty() {
            format!("{flags} --ca-cert <FILE>")
        } else {
            format!("{flags} --ca-cert <FILE> {}", format_invocation(operands))
        }
    }

    /// The line as typed, on `shown`.
    pub(super) fn as_typed(&self, shown: &str) -> String {
        format_invocation(&self.typed(shown))
    }

    /// The line as typed, on `shown`, with `web` named.
    pub(super) fn naming_web(&self, shown: &str) -> String {
        let mut words = self.typed(shown);
        words.insert(1, OsString::from("web"));
        format_invocation(&words)
    }
}

/// Where a flag can still go: clap reads every word after `--` as a positional.
fn end_of_flags(words: &[OsString]) -> usize {
    words
        .iter()
        .position(|word| word == "--")
        .unwrap_or(words.len())
}
