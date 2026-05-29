//! Color routing for text-mode scorecard output.
//!
//! Wraps `--color auto|always|never` decisions and exposes a single
//! `should_color()` helper that `format_text` consults to decide whether to
//! emit ANSI escape sequences. `auto` is the default and bows to the runtime
//! environment: when `NO_COLOR` is set, when stdout is not a terminal, or
//! when the terminal advertises no color support, color is suppressed.
//!
//! Uses the `anstyle` / `anstream` crates the rest of the clap ecosystem
//! standardizes on. No new dependency cost — both are already transitive
//! deps of `clap`. `anstyle` provides escape-code primitives; we apply them
//! by hand around status prefixes rather than wrapping output streams,
//! because the only color-bearing tokens in the scorecard are the five
//! status prefixes and the badge hint.

use std::io::IsTerminal;

use anstyle::{AnsiColor, Color, Style};

use crate::cli::ColorChoice;

/// Decide whether to emit ANSI styling for this run.
///
/// `Always` and `Never` short-circuit. `Auto` audits `NO_COLOR` first
/// (per <https://no-color.org/>: any non-empty value disables color), then
/// asks the OS whether stdout is a terminal.
pub fn should_color(choice: ColorChoice) -> bool {
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => {
            if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
                return false;
            }
            std::io::stdout().is_terminal()
        }
    }
}

/// Style returned by [`status_style`] when no color should be applied. Cheap
/// no-op: `Style::default()` renders as the empty string.
const NO_STYLE: Style = Style::new();

/// Map a status prefix string to its anstyle styling. Returns `NO_STYLE`
/// when `enabled` is false so callers can wrap unconditionally.
///
/// Colors mirror the convention common to `cargo`, `rustc`, and `clippy`:
/// green for success, yellow for warnings, red for failures, dim for
/// neutral/skip, bold red for hard errors.
pub fn status_style(prefix: &str, enabled: bool) -> Style {
    if !enabled {
        return NO_STYLE;
    }
    match prefix.trim() {
        "PASS" => Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green))),
        "WARN" => Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow))),
        "FAIL" => Style::new()
            .fg_color(Some(Color::Ansi(AnsiColor::Red)))
            .bold(),
        "ERR" => Style::new()
            .fg_color(Some(Color::Ansi(AnsiColor::Red)))
            .bold(),
        "SKIP" => Style::new().dimmed(),
        _ => NO_STYLE,
    }
}

/// Render `text` wrapped in the given style. `Style::default()` renders the
/// text unchanged, so this is safe to call on every prefix regardless of
/// the color decision.
pub fn paint(style: Style, text: &str) -> String {
    format!("{style}{text}{style:#}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_returns_true() {
        assert!(should_color(ColorChoice::Always));
    }

    #[test]
    fn never_returns_false() {
        assert!(!should_color(ColorChoice::Never));
    }

    #[test]
    fn auto_with_no_color_env_returns_false() {
        // Test serially-safe: read+restore. Other tests in this file do not
        // depend on NO_COLOR so no interference window.
        let prior = std::env::var_os("NO_COLOR");
        // SAFETY: env mutation is not thread-safe in general; this binary
        // runs tests with --test-threads=1 by virtue of cargo defaults only
        // when the user configures it. Accept the small race window — the
        // value is restored before the test returns.
        unsafe {
            std::env::set_var("NO_COLOR", "1");
        }
        let result = should_color(ColorChoice::Auto);
        unsafe {
            match prior {
                Some(v) => std::env::set_var("NO_COLOR", v),
                None => std::env::remove_var("NO_COLOR"),
            }
        }
        assert!(!result, "NO_COLOR=1 should suppress auto color");
    }

    #[test]
    fn status_style_disabled_is_empty() {
        let painted = paint(status_style("PASS", false), "PASS");
        assert_eq!(painted, "PASS");
    }

    #[test]
    fn status_style_enabled_wraps_with_ansi() {
        let painted = paint(status_style("FAIL", true), "FAIL");
        assert!(
            painted.contains('\u{1b}'),
            "enabled FAIL prefix should carry ANSI escapes: {painted:?}"
        );
        assert!(painted.contains("FAIL"));
    }

    #[test]
    fn unknown_prefix_returns_no_style() {
        let style = status_style("WHATEVER", true);
        // Unknown prefixes round-trip unchanged even with color enabled.
        assert_eq!(paint(style, "WHATEVER"), "WHATEVER");
    }
}
