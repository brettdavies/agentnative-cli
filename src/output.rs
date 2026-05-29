//! Single stdout hand-off point for the binary.
//!
//! Centralizing this lets the `p7-naked-println` source audit exempt one
//! module by path convention (filename contains "output") instead of treating
//! every `println!`/`print!` site across the orchestration layer as a
//! naked-println violation. Callers route final user-facing output through
//! `emit` (no trailing newline) or `emit_line` (with trailing newline) per
//! whatever the caller's renderer already produces.

/// Write `s` to stdout with no trailing newline. Use when the caller's
/// renderer has already appended whatever line terminator it wants.
pub fn emit(s: &str) {
    print!("{s}");
}

/// Write `s` to stdout with a trailing newline. Use when the caller renders
/// content that does not end in a newline (e.g. JSON one-liners) and needs
/// the terminal cursor to advance.
pub fn emit_line(s: &str) {
    println!("{s}");
}
