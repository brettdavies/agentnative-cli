//! Audit: `p4-should-enumerate-valid-set`.
//!
//! When a CLI rejects input against a closed set, the error message includes
//! the valid set. Clap satisfies this for free when the closed-set is declared
//! via `ValueEnum`, `value_parser!`, or `PossibleValuesParser` — the default
//! "invalid value" error names every accepted variant. This audit verifies the
//! closed-set is **declared** at all; the message-shape is then guaranteed by
//! clap.
//!
//! Vacuous Pass when no clap usage is detected. Warn when clap is used but
//! no closed-set declaration appears — the CLI may be hand-rolling string
//! matching, in which case it likely fails the requirement.

use crate::audit::Audit;
use crate::project::{Language, Project};
use crate::source::has_pattern_in;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct EnumerateValidSetAudit;

impl Audit for EnumerateValidSetAudit {
    fn id(&self) -> &str {
        "p4-enumerate-valid-set"
    }

    fn label(&self) -> &'static str {
        "Closed-set rejection declares valid choices"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P4
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-should-enumerate-valid-set"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let parsed = project.parsed_files();
        let mut found_closed_set = false;
        let mut found_clap = false;

        for (_path, parsed_file) in parsed.iter() {
            match audit_enumerate_valid_set(&parsed_file.source) {
                EnumerateScan::ClosedSetDeclared => {
                    found_closed_set = true;
                    break;
                }
                EnumerateScan::ClapWithoutClosedSet => found_clap = true,
                EnumerateScan::NoClap => {}
            }
        }

        let status = if found_closed_set {
            AuditStatus::Pass
        } else if found_clap {
            AuditStatus::Warn(
                "clap detected but no `ValueEnum` / `PossibleValuesParser` / \
                 `value_parser!` declaration found. Closed-set rejection \
                 messages should enumerate the valid choices."
                    .into(),
            )
        } else {
            AuditStatus::Pass
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: None,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EnumerateScan {
    ClosedSetDeclared,
    ClapWithoutClosedSet,
    NoClap,
}

/// Inspect a single Rust source file. Returns the strongest signal found.
pub(crate) fn audit_enumerate_valid_set(source: &str) -> EnumerateScan {
    // Closed-set declarations — any of these means clap will surface valid
    // choices in the rejection message for free.
    let closed_set_signals = [
        // String-literal sniffs cover the surface across clap derive macros,
        // builder API, and re-exports without needing rich AST patterns.
        "ValueEnum",
        "PossibleValuesParser",
        "value_parser!",
        "PossibleValue::new",
    ];

    if closed_set_signals.iter().any(|sig| source.contains(sig)) {
        return EnumerateScan::ClosedSetDeclared;
    }

    let clap_signals = [
        "clap::Parser",
        "clap::Args",
        "clap::Subcommand",
        "clap::Command",
        "use clap::",
        "#[command(",
        "#[arg(",
        "#[derive(Parser",
        "#[derive(Subcommand",
        "Arg::new(",
    ];

    let has_clap = clap_signals.iter().any(|sig| source.contains(sig))
        || has_pattern_in(source, "clap::Parser", Language::Rust);

    if has_clap {
        EnumerateScan::ClapWithoutClosedSet
    } else {
        EnumerateScan::NoClap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_value_enum_derive() {
        let source = r#"
use clap::ValueEnum;

#[derive(Clone, Debug, ValueEnum)]
enum Format {
    Text,
    Json,
}
"#;
        assert_eq!(
            audit_enumerate_valid_set(source),
            EnumerateScan::ClosedSetDeclared
        );
    }

    #[test]
    fn happy_path_possible_values_parser() {
        let source = r#"
use clap::Arg;
use clap::builder::PossibleValuesParser;

fn cli() {
    Arg::new("mode")
        .value_parser(PossibleValuesParser::new(["fast", "slow"]));
}
"#;
        assert_eq!(
            audit_enumerate_valid_set(source),
            EnumerateScan::ClosedSetDeclared
        );
    }

    #[test]
    fn happy_path_value_parser_macro() {
        let source = r#"
use clap::Arg;

#[derive(Clone, ValueEnum)]
enum Mode { Fast, Slow }

fn cli() {
    Arg::new("mode").value_parser(value_parser!(Mode));
}
"#;
        assert_eq!(
            audit_enumerate_valid_set(source),
            EnumerateScan::ClosedSetDeclared
        );
    }

    #[test]
    fn warn_clap_without_closed_set() {
        let source = r#"
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    mode: String,
}
"#;
        assert_eq!(
            audit_enumerate_valid_set(source),
            EnumerateScan::ClapWithoutClosedSet
        );
    }

    #[test]
    fn vacuous_pass_no_clap() {
        let source = r#"
fn main() {
    println!("hello");
}
"#;
        assert_eq!(audit_enumerate_valid_set(source), EnumerateScan::NoClap);
    }

    #[test]
    fn applicable_for_rust() {
        use crate::project::{Language, Project};
        use std::path::PathBuf;
        use std::sync::OnceLock;

        let project = Project {
            path: PathBuf::from("."),
            language: Some(Language::Rust),
            binary_paths: vec![],
            manifest_path: None,
            runner: None,
            include_tests: false,
            parsed_files: OnceLock::new(),
            help_output: OnceLock::new(),
        };
        assert!(EnumerateValidSetAudit.applicable(&project));
    }

    #[test]
    fn not_applicable_for_python() {
        use crate::project::{Language, Project};
        use std::path::PathBuf;
        use std::sync::OnceLock;

        let project = Project {
            path: PathBuf::from("."),
            language: Some(Language::Python),
            binary_paths: vec![],
            manifest_path: None,
            runner: None,
            include_tests: false,
            parsed_files: OnceLock::new(),
            help_output: OnceLock::new(),
        };
        assert!(!EnumerateValidSetAudit.applicable(&project));
    }
}
