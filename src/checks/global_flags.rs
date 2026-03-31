//! PoC Check #2 (MEDIUM): Detect clap flags missing `global = true`.
//!
//! Maps to: check-p6-global-flags from the existing 24 bash checks.
//! Principle: P6 (Composable Structure) — Agentic flags (--output, --quiet,
//! --verbose, --no-color) should be `global = true` so they work on all subcommands.
//!
//! This is a conditional check:
//!   Trigger: the CLI uses clap derive with subcommands
//!   Requirement: agentic flags must have `global = true`

use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, SourceLocation};

/// Agentic flags that should be global when subcommands exist.
const AGENTIC_FLAGS: &[&str] = &["output", "quiet", "verbose", "no_color", "no-color"];

/// Check whether agentic clap flags have `global = true`.
pub fn check_global_flags(source: &str, file: &str) -> CheckResult {
    // Step 1: Detect if the project uses clap subcommands.
    // Look for #[command(subcommand)] or Subcommand derive.
    let has_subcommands =
        has_pattern(source, "Subcommand") || has_pattern(source, "#[command(subcommand)]");

    if !has_subcommands {
        return CheckResult {
            id: "p6-global-flags".to_string(),
            label: "Agentic flags are global".to_string(),
            group: CheckGroup::P6,
            layer: CheckLayer::Source,
            status: CheckStatus::Skip("No subcommands detected".to_string()),
        };
    }

    // Step 2: Find all clap field attributes and check agentic flags.
    let missing = find_non_global_agentic_flags(source, file);

    let status = if missing.is_empty() {
        CheckStatus::Pass
    } else {
        let evidence = missing
            .iter()
            .map(|m| {
                format!(
                    "{}:{}:{} — {} missing global = true",
                    m.file, m.line, m.column, m.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        CheckStatus::Warn(evidence)
    };

    CheckResult {
        id: "p6-global-flags".to_string(),
        label: "Agentic flags are global".to_string(),
        group: CheckGroup::P6,
        layer: CheckLayer::Source,
        status,
    }
}

fn has_pattern(source: &str, pattern_str: &str) -> bool {
    let pattern = match Pattern::try_new(pattern_str, Rust) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = Rust.ast_grep(source);
    root.root().find(&pattern).is_some()
}

/// Find agentic flag fields that lack `global = true`.
///
/// Strategy: find all struct fields with `#[arg(...)]` attributes,
/// check if the field name is agentic, then check if `global = true` is present.
fn find_non_global_agentic_flags(source: &str, file: &str) -> Vec<SourceLocation> {
    let root = Rust.ast_grep(source);
    let root_node = root.root();

    // Find all #[arg(...)] attributes — these mark clap CLI fields.
    let arg_attr_pattern = Pattern::new("#[arg($$$ARGS)]", Rust);

    let mut missing = Vec::new();

    for attr_match in root_node.find_all(&arg_attr_pattern) {
        let attr_text = attr_match.text().to_string();

        // Check if this attribute is on an agentic flag by looking at the field name.
        // Walk up to find the enclosing field/struct context.
        // For now, check if any agentic flag name appears in the long = "..." or the attr text.
        let is_agentic = AGENTIC_FLAGS.iter().any(|flag| {
            attr_text.contains(&format!("long = \"{flag}\"")) || attr_text.contains(flag)
        });

        if !is_agentic {
            continue;
        }

        // Check if `global = true` is present in this #[arg(...)] block.
        if !attr_text.contains("global = true") {
            let pos = attr_match.start_pos();
            missing.push(SourceLocation {
                file: file.to_string(),
                line: pos.line() + 1,
                column: pos.column(&attr_match) + 1,
                text: attr_text,
            });
        }
    }

    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_subcommands() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long)]
    output: Option<String>,
}
"#;
        let result = check_global_flags(source, "src/cli.rs");
        assert!(matches!(result.status, CheckStatus::Skip(_)));
    }

    #[test]
    fn pass_when_agentic_flags_are_global() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "output", global = true)]
    output: Option<String>,

    #[arg(long = "quiet", global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Check,
}
"#;
        let result = check_global_flags(source, "src/cli.rs");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_agentic_flags_missing_global() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "output")]
    output: Option<String>,

    #[arg(long = "quiet")]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Check,
}
"#;
        let result = check_global_flags(source, "src/cli.rs");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("output"));
            assert!(evidence.contains("quiet"));
        }
    }

    #[test]
    fn ignores_non_agentic_flags() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "path")]
    path: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Check,
}
"#;
        let result = check_global_flags(source, "src/cli.rs");
        assert_eq!(result.status, CheckStatus::Pass);
    }
}
