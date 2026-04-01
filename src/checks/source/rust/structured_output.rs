//! Check: Detect structured output type (e.g., `OutputFormat` enum).
//!
//! Principle: P2 (Structured Output) — CLIs should support structured output
//! formats like JSON so agents can parse results programmatically.
//!
//! Looks for `enum OutputFormat { ... }` or `enum Format { ... }` via ast-grep.
//! Skips if no clap dependency detected.

use crate::check::Check;
use crate::project::{Language, Project};
use crate::source::has_pattern;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

/// Check trait implementation for structured output detection.
pub struct StructuredOutputCheck;

impl Check for StructuredOutputCheck {
    fn id(&self) -> &str {
        "p2-structured-output"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P2
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut has_clap = false;
        let mut has_output_format = false;

        for (_path, parsed_file) in parsed.iter() {
            let result = check_structured_output(&parsed_file.source);
            match &result.status {
                CheckStatus::Skip(_) => {
                    // No clap in this file
                }
                CheckStatus::Pass => {
                    has_clap = true;
                    has_output_format = true;
                }
                CheckStatus::Warn(_) => {
                    has_clap = true;
                }
                _ => {}
            }
        }

        let status = if !has_clap {
            CheckStatus::Skip("no clap dependency detected".to_string())
        } else if has_output_format {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(
                "No OutputFormat or Format enum found. CLIs should support \
                 structured output (e.g., --output json) for agent consumption."
                    .to_string(),
            )
        };

        Ok(CheckResult {
            id: "p2-structured-output".to_string(),
            label: "Structured output type exists".to_string(),
            group: CheckGroup::P2,
            layer: CheckLayer::Source,
            status,
        })
    }
}

/// Check a single source string for OutputFormat enum.
pub(crate) fn check_structured_output(source: &str) -> CheckResult {
    let has_clap = source.contains("clap") || source.contains("#[derive(Parser)]");

    if !has_clap {
        return CheckResult {
            id: "p2-structured-output".to_string(),
            label: "Structured output type exists".to_string(),
            group: CheckGroup::P2,
            layer: CheckLayer::Source,
            status: CheckStatus::Skip("no clap dependency detected".to_string()),
        };
    }

    let has_output_format = has_pattern(source, "enum OutputFormat { $$$BODY }")
        || has_pattern(source, "enum Format { $$$BODY }");

    let status = if has_output_format {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(
            "No OutputFormat or Format enum found. CLIs should support \
             structured output (e.g., --output json) for agent consumption."
                .to_string(),
        )
    };

    CheckResult {
        id: "p2-structured-output".to_string(),
        label: "Structured output type exists".to_string(),
        group: CheckGroup::P2,
        layer: CheckLayer::Source,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_clap() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let result = check_structured_output(source);
        assert!(matches!(result.status, CheckStatus::Skip(_)));
    }

    #[test]
    fn pass_with_output_format_enum() {
        let source = r#"
use clap::Parser;

#[derive(Clone)]
enum OutputFormat {
    Json,
    Text,
    Table,
}

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    output: OutputFormat,
}
"#;
        let result = check_structured_output(source);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn pass_with_format_enum() {
        let source = r#"
use clap::Parser;

enum Format {
    Json,
    Yaml,
}

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    format: Format,
}
"#;
        let result = check_structured_output(source);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_clap_but_no_output_format() {
        let source = r#"
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    verbose: bool,
}
"#;
        let result = check_structured_output(source);
        assert!(matches!(result.status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("OutputFormat"));
        }
    }

    #[test]
    fn skip_detects_clap_via_derive_parser() {
        // Even without `use clap`, #[derive(Parser)] should trigger detection
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long)]
    name: String,
}
"#;
        let result = check_structured_output(source);
        // Has clap (via derive(Parser)) but no output format
        assert!(matches!(result.status, CheckStatus::Warn(_)));
    }

    #[test]
    fn applicable_for_rust() {
        let check = StructuredOutputCheck;
        let dir = std::env::temp_dir().join(format!("anc-structout-rust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(check.applicable(&project));
    }

    #[test]
    fn not_applicable_for_none() {
        let check = StructuredOutputCheck;
        let dir = std::env::temp_dir().join(format!("anc-structout-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
