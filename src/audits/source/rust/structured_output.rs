//! Audit: Detect structured output type (e.g., `OutputFormat` enum).
//!
//! Principle: P2 (Structured Output) — CLIs should support structured output
//! formats like JSON so agents can parse results programmatically.
//!
//! Looks for `enum OutputFormat { ... }` or `enum Format { ... }` via ast-grep.
//! Skips if no clap dependency detected.

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::has_any;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

static OUTPUT_FORMAT_ENUMS: LazyLock<[Pattern; 4]> = LazyLock::new(|| {
    [
        Pattern::new("enum OutputFormat { $$$BODY }", Rust),
        Pattern::new("pub enum OutputFormat { $$$BODY }", Rust),
        Pattern::new("enum Format { $$$BODY }", Rust),
        Pattern::new("pub enum Format { $$$BODY }", Rust),
    ]
});

const MISSING_FORMAT: &str = "No OutputFormat or Format enum found. CLIs should support \
                              structured output (e.g., --output json) for agent consumption.";

/// Audit trait implementation for structured output detection.
#[derive(Debug)]
pub struct StructuredOutputAudit;

impl Audit for StructuredOutputAudit {
    fn id(&self) -> &str {
        "p2-structured-output"
    }

    fn label(&self) -> &'static str {
        "Structured output type exists"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P2
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-must-output-flag"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let mut has_clap = false;
        let mut has_output_format = false;

        for (_, status) in pass::findings(project, Language::Rust, Rust, self) {
            match status {
                AuditStatus::Skip(_) => {
                    // No clap in this file
                }
                AuditStatus::Pass => {
                    has_clap = true;
                    has_output_format = true;
                }
                AuditStatus::Warn(_) => {
                    has_clap = true;
                }
                _ => {}
            }
        }

        let status = if !has_clap {
            AuditStatus::Skip("no clap dependency detected".to_string())
        } else if has_output_format {
            AuditStatus::Pass
        } else {
            AuditStatus::Warn(MISSING_FORMAT.to_string())
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

impl SourceScan<Rust> for StructuredOutputAudit {
    type Finding = AuditStatus;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> AuditStatus {
        audit_structured_output(file)
    }
}

/// Audit one file for an OutputFormat enum.
pub(crate) fn audit_structured_output(file: &SourceFile<'_, Rust>) -> AuditStatus {
    let source = file.source();
    let has_clap = source.contains("clap") || source.contains("#[derive(Parser)]");

    if !has_clap {
        return AuditStatus::Skip("no clap dependency detected".to_string());
    }

    if has_any(&file.root(), &*OUTPUT_FORMAT_ENUMS) {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(MISSING_FORMAT.to_string())
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
        let status = audit_structured_output(&pass::rust(source, "src/cli.rs"));
        assert!(matches!(status, AuditStatus::Skip(_)));
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
        let status = audit_structured_output(&pass::rust(source, "src/cli.rs"));
        assert_eq!(status, AuditStatus::Pass);
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
        let status = audit_structured_output(&pass::rust(source, "src/cli.rs"));
        assert_eq!(status, AuditStatus::Pass);
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
        let status = audit_structured_output(&pass::rust(source, "src/cli.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
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
        let status = audit_structured_output(&pass::rust(source, "src/cli.rs"));
        // Has clap (via derive(Parser)) but no output format
        assert!(matches!(status, AuditStatus::Warn(_)));
    }

    #[test]
    fn applicable_for_rust() {
        let audit = StructuredOutputAudit;
        let dir = std::env::temp_dir().join(format!("anc-structout-rust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(audit.applicable(&project));
    }

    #[test]
    fn not_applicable_for_none() {
        let audit = StructuredOutputAudit;
        let dir = std::env::temp_dir().join(format!("anc-structout-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }
}
