//! What kind of line a help line is. Every reader of flag definitions asks
//! here, so the flag parser and the env-hint proximity window agree on which
//! lines define a flag.

/// A line that defines a flag: indented with a space, then a dash. A `---`
/// rule is not a definition.
pub(in crate::runner::help_probe) fn is_definition_line(line: &str) -> bool {
    if !line.starts_with(' ') {
        return false;
    }
    let trimmed = line.trim_start();
    trimmed.starts_with('-') && !trimmed.starts_with("---")
}

/// A top-level section heading: not indented, ends in `:`, and carries an
/// uppercase letter (`OPTIONS:`, `Global Flags:`, `ENVIRONMENT:`).
pub(in crate::runner::help_probe) fn is_section_heading(line: &str) -> bool {
    !line.is_empty()
        && !line.starts_with(' ')
        && line.trim().ends_with(':')
        && line.chars().any(|c| c.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_definition_line_is_space_led_and_dash_led() {
        assert!(is_definition_line("  -q, --quiet    Say less."));
        assert!(is_definition_line("      --null"));
        assert!(!is_definition_line("-q, --quiet    Say less."));
        assert!(!is_definition_line("\t-q, --quiet    Say less."));
        assert!(!is_definition_line("  quiet mode"));
        assert!(!is_definition_line("  ---"));
        assert!(!is_definition_line(""));
    }

    #[test]
    fn a_section_heading_is_unindented_and_ends_in_a_colon() {
        assert!(is_section_heading("Options:"));
        assert!(is_section_heading("ENVIRONMENT:"));
        assert!(!is_section_heading("  Options:"));
        assert!(!is_section_heading("options:"));
        assert!(!is_section_heading("Usage: tool [OPTIONS]"));
        assert!(!is_section_heading(""));
    }
}
