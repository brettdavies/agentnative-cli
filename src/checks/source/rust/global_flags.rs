//! Check: Detect clap flags missing `global = true`.
//!
//! Maps to: check-p6-global-flags from the existing 24 bash checks.
//! Principle: P6 (Composable Structure) — Agentic flags (--output, --quiet,
//! --verbose, --no-color) should be `global = true` so they work on all subcommands.
//!
//! This is a conditional check:
//!   Trigger: the CLI uses clap derive with subcommands
//!   Requirement: agentic flags must have `global = true`

use ast_grep_core::Pattern;
use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::Rust;

use crate::check::Check;
use crate::project::{Language, Project};
use crate::source::has_pattern;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, SourceLocation};

/// Agentic flags that should be global when subcommands exist.
const AGENTIC_FLAGS: &[&str] = &["output", "quiet", "verbose", "no_color", "no-color"];

/// Check trait implementation for global flags detection.
pub struct GlobalFlagsCheck;

impl Check for GlobalFlagsCheck {
    fn id(&self) -> &str {
        "p6-global-flags"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-must-global-flags"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut all_warn_evidence = Vec::new();
        let mut has_subcommands = false;

        for (path, parsed_file) in parsed.iter() {
            let file_str = path.display().to_string();
            match &check_global_flags(&parsed_file.source, &file_str) {
                CheckStatus::Warn(evidence) => {
                    has_subcommands = true;
                    all_warn_evidence.push(evidence.clone());
                }
                CheckStatus::Pass => {
                    // Pass means subcommands were found but all flags are global
                    has_subcommands = true;
                }
                CheckStatus::Skip(_) => {
                    // No subcommands in this file
                }
                _ => {}
            }
        }

        let status = if !has_subcommands {
            CheckStatus::Skip("No subcommands detected".to_string())
        } else if all_warn_evidence.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(all_warn_evidence.join("\n"))
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Agentic flags are global".to_string(),
            group: self.group(),
            layer: self.layer(),
            status,
        })
    }
}

/// Check a single source string for non-global agentic flags.
///
/// Kept public(crate) for unit testing with inline source strings.
pub(crate) fn check_global_flags(source: &str, file: &str) -> CheckStatus {
    // Step 1: Detect if the project uses clap subcommands.
    let has_subcommands =
        has_pattern(source, "Subcommand") || has_pattern(source, "#[command(subcommand)]");

    if !has_subcommands {
        return CheckStatus::Skip("No subcommands detected".to_string());
    }

    // Step 2: Find all clap field attributes and check agentic flags.
    let missing = find_non_global_agentic_flags(source, file);

    if missing.is_empty() {
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
    }
}

/// Find agentic flag fields that lack `global = true`.
fn find_non_global_agentic_flags(source: &str, file: &str) -> Vec<SourceLocation> {
    let root = Rust.ast_grep(source);
    let root_node = root.root();

    let arg_attr_pattern = Pattern::new("#[arg($$$ARGS)]", Rust);

    let mut missing = Vec::new();

    for attr_match in root_node.find_all(&arg_attr_pattern) {
        let attr_text = attr_match.text().to_string();

        let is_agentic = AGENTIC_FLAGS.iter().any(|flag| {
            attr_text.contains(&format!("long = \"{flag}\"")) || attr_text.contains(flag)
        });

        if !is_agentic {
            continue;
        }

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
        let status = check_global_flags(source, "src/cli.rs");
        assert!(matches!(status, CheckStatus::Skip(_)));
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
        let status = check_global_flags(source, "src/cli.rs");
        assert_eq!(status, CheckStatus::Pass);
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
        let status = check_global_flags(source, "src/cli.rs");
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
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
        let status = check_global_flags(source, "src/cli.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let check = GlobalFlagsCheck;
        let dir = std::env::temp_dir().join(format!("anc-gflags-rust-{}", std::process::id()));
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
        let check = GlobalFlagsCheck;
        let dir = std::env::temp_dir().join(format!("anc-gflags-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
