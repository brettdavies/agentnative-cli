//! Compiling the engine's own pattern literals.

use regex::Regex;

/// Compile a pattern the engine's source fixes at build time. Each one is a
/// literal its module's tests exercise, so a pattern that fails to compile
/// is a programming error caught before release, not a runtime condition.
pub(crate) fn fixed(pattern: &str) -> Regex {
    Regex::new(pattern).expect("an engine pattern literal compiles")
}
