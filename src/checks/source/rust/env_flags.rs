//! Check: Detect agentic clap flags missing `env = "..."` attribute.
//!
//! Principle: P1 (Non-Interactive by Default) MUST — "Every flag settable
//! via environment variable." Agentic flags (output, quiet, verbose, timeout,
//! no-color, format) should all have env-var bindings so agents can set
//! defaults without passing flags every invocation. Renamed from
//! `p6-env-flags` in v0.1.1 — the spec requirement lives in P1, not P6.

use ast_grep_core::Pattern;
use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_language::Rust;

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence, SourceLocation};

/// Agentic flags that should have `env = "..."` backing.
const AGENTIC_FLAGS: &[&str] = &[
    "output", "quiet", "verbose", "timeout", "no_color", "no-color", "format",
];

/// Check trait implementation for env-backed flag detection.
pub struct EnvFlagsCheck;

impl Check for EnvFlagsCheck {
    fn id(&self) -> &str {
        "p1-env-flags-source"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P1
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-env-var"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut all_warn_evidence = Vec::new();
        let mut has_clap_attrs = false;

        for (path, parsed_file) in parsed.iter() {
            let file_str = path.display().to_string();
            match &check_env_flags(&parsed_file.source, &file_str) {
                CheckStatus::Warn(evidence) => {
                    has_clap_attrs = true;
                    all_warn_evidence.push(evidence.clone());
                }
                CheckStatus::Pass => {
                    has_clap_attrs = true;
                }
                CheckStatus::Skip(_) => {
                    // No clap attributes in this file
                }
                _ => {}
            }
        }

        let status = if !has_clap_attrs {
            CheckStatus::Skip("No clap #[arg(...)] attributes found".to_string())
        } else if all_warn_evidence.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(all_warn_evidence.join("\n"))
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Agentic flags have env backing".to_string(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// Check a single source string for agentic flags missing `env = "..."`.
///
/// Kept public(crate) for unit testing with inline source strings.
pub(crate) fn check_env_flags(source: &str, file: &str) -> CheckStatus {
    let missing = find_agentic_flags_missing_env(source, file);

    // If we found no agentic arg attributes at all, check if there are *any* arg attributes.
    if missing.found_agentic == 0 {
        let has_any_arg = has_arg_attributes(source);
        if !has_any_arg {
            return CheckStatus::Skip("No clap #[arg(...)] attributes found".to_string());
        }
        // Has arg attributes but none are agentic — that's still a skip for this file
        return CheckStatus::Skip("No agentic flags found".to_string());
    }

    if missing.locations.is_empty() {
        CheckStatus::Pass
    } else {
        let evidence = missing
            .locations
            .iter()
            .map(|m| {
                format!(
                    "{}:{}:{} — {} missing env attribute",
                    m.file, m.line, m.column, m.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        CheckStatus::Warn(evidence)
    }
}

struct MissingEnvResult {
    found_agentic: usize,
    locations: Vec<SourceLocation>,
}

/// Check if the source has any `#[arg(...)]` attributes.
fn has_arg_attributes(source: &str) -> bool {
    let pattern = match Pattern::try_new("#[arg($$$ARGS)]", Rust) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = Rust.ast_grep(source);
    root.root().find(&pattern).is_some()
}

/// Find agentic flag attributes that lack `env = "..."`.
fn find_agentic_flags_missing_env(source: &str, file: &str) -> MissingEnvResult {
    let root = Rust.ast_grep(source);
    let root_node = root.root();

    let arg_attr_pattern = match Pattern::try_new("#[arg($$$ARGS)]", Rust) {
        Ok(p) => p,
        Err(_) => {
            return MissingEnvResult {
                found_agentic: 0,
                locations: Vec::new(),
            };
        }
    };

    let mut found_agentic = 0;
    let mut missing = Vec::new();

    for attr_match in root_node.find_all(&arg_attr_pattern) {
        let attr_text = attr_match.text().to_string();

        let is_agentic = AGENTIC_FLAGS.iter().any(|flag| {
            attr_text.contains(&format!("long = \"{flag}\"")) || attr_text.contains(flag)
        });

        if !is_agentic {
            continue;
        }

        found_agentic += 1;

        // Check if the attribute has `env = "..."` or just `env`
        if !attr_text.contains("env") {
            let pos = attr_match.start_pos();
            missing.push(SourceLocation {
                file: file.to_string(),
                line: pos.line() + 1,
                column: pos.column(&attr_match) + 1,
                text: attr_text,
            });
        }
    }

    MissingEnvResult {
        found_agentic,
        locations: missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_agentic_flags_have_env() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "output", env = "MYAPP_OUTPUT")]
    output: Option<String>,

    #[arg(long = "quiet", env = "MYAPP_QUIET")]
    quiet: bool,

    #[arg(long = "verbose", env = "MYAPP_VERBOSE")]
    verbose: bool,
}
"#;
        let status = check_env_flags(source, "src/cli.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_agentic_flags_missing_env() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "output")]
    output: Option<String>,

    #[arg(long = "quiet")]
    quiet: bool,
}
"#;
        let status = check_env_flags(source, "src/cli.rs");
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
            assert!(evidence.contains("output"));
            assert!(evidence.contains("quiet"));
            assert!(evidence.contains("missing env"));
        }
    }

    #[test]
    fn skip_when_no_clap_attributes() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let status = check_env_flags(source, "src/main.rs");
        assert!(matches!(status, CheckStatus::Skip(_)));
    }

    #[test]
    fn skip_when_no_agentic_flags() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "path")]
    path: String,

    #[arg(long = "config")]
    config: Option<String>,
}
"#;
        let status = check_env_flags(source, "src/cli.rs");
        assert!(matches!(status, CheckStatus::Skip(_)));
    }

    #[test]
    fn mixed_flags_warns_only_for_missing() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "output", env = "MYAPP_OUTPUT")]
    output: Option<String>,

    #[arg(long = "verbose")]
    verbose: bool,
}
"#;
        let status = check_env_flags(source, "src/cli.rs");
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
            assert!(evidence.contains("verbose"));
            assert!(!evidence.contains("output"));
        }
    }

    #[test]
    fn pass_with_timeout_env() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "timeout", env = "MYAPP_TIMEOUT")]
    timeout: Option<u64>,
}
"#;
        let status = check_env_flags(source, "src/cli.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let check = EnvFlagsCheck;
        let dir = std::env::temp_dir().join(format!("anc-envflags-rust-{}", std::process::id()));
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
        let check = EnvFlagsCheck;
        let dir = std::env::temp_dir().join(format!("anc-envflags-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
