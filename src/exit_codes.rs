//! The exit table every verb shares: the four codes `anc audit` and
//! `anc web` return, and the text `--help` and the README print for them.

/// The exit table's text. A macro so `--help` can splice it into its
/// literal `after_help` strings with `concat!` and [`EXIT_TABLE`] stays the
/// same bytes.
#[doc(hidden)]
#[macro_export]
macro_rules! exit_table {
    () => {
        "Exit codes (the same for every verb):
  0  clean: every applicable check passed
  1  warnings only: a SHOULD or MAY check missed
  2  failures present: a MUST check missed, or a usage error
  3  could not check: the target was unreachable, an audit or probe errored or
     was cut short by the deadline, or every selected check was inapplicable"
    };
}

/// Every applicable check passed.
pub const EXIT_CLEAN: i32 = 0;
/// Warnings only: a SHOULD or MAY miss.
pub const EXIT_WARNINGS: i32 = 1;
/// Failures present: a MUST miss, or a usage error.
pub const EXIT_FAILURES: i32 = 2;
/// Could not check: an unreachable target, an audit or probe that errored or
/// was cut short, or every selected check inapplicable.
pub const EXIT_COULD_NOT_CHECK: i32 = 3;

/// The exit table, as `--help` and the README print it.
pub const EXIT_TABLE: &str = exit_table!();

/// `one` when `n` is 1, else `many`.
pub fn plural(n: usize, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 { one } else { many }
}
