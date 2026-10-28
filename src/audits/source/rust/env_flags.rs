//! Audit: Detect agentic clap flags missing `env = "..."` attribute.
//!
//! Principle: P1 (Non-Interactive by Default) MUST — "Every flag settable
//! via environment variable." Agentic flags (output, quiet, verbose, timeout,
//! no-color, format) should all have env-var bindings so agents can set
//! defaults without passing flags every invocation. Renamed from
//! `p6-env-flags` in v0.1.1 — the spec requirement lives in P1, not P6.

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, SourceLocation};

/// Agentic flags that should have `env = "..."` backing.
const AGENTIC_FLAGS: &[&str] = &[
    "output", "quiet", "verbose", "timeout", "no_color", "no-color", "format",
];

static ARG_ATTR: LazyLock<Pattern> = LazyLock::new(|| Pattern::new("#[arg($$$ARGS)]", Rust));

/// Audit trait implementation for env-backed flag detection.
#[derive(Debug)]
pub struct EnvFlagsAudit;

impl Audit for EnvFlagsAudit {
    fn id(&self) -> &str {
        "p1-env-flags-source"
    }

    fn label(&self) -> &'static str {
        "Agentic flags have env backing"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P1
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-env-var"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let mut all_warn_evidence = Vec::new();
        let mut has_clap_attrs = false;

        for (_, status) in pass::findings(project, Language::Rust, Rust, self) {
            match status {
                AuditStatus::Warn(evidence) => {
                    has_clap_attrs = true;
                    all_warn_evidence.push(evidence);
                }
                AuditStatus::Pass => {
                    has_clap_attrs = true;
                }
                AuditStatus::Skip(_) => {
                    // No clap attributes in this file
                }
                _ => {}
            }
        }

        let status = if !has_clap_attrs {
            AuditStatus::Skip("No clap #[arg(...)] attributes found".to_string())
        } else if all_warn_evidence.is_empty() {
            AuditStatus::Pass
        } else {
            AuditStatus::Warn(all_warn_evidence.join("\n"))
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

impl SourceScan<Rust> for EnvFlagsAudit {
    type Finding = AuditStatus;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> AuditStatus {
        audit_env_flags(file)
    }
}

/// Audit one file for agentic flags missing `env = "..."`.
pub(crate) fn audit_env_flags(file: &SourceFile<'_, Rust>) -> AuditStatus {
    let missing = find_agentic_flags_missing_env(file);

    if missing.found_agentic == 0 {
        if !missing.found_any {
            return AuditStatus::Skip("No clap #[arg(...)] attributes found".to_string());
        }
        // Has arg attributes but none are agentic — that's still a skip for this file
        return AuditStatus::Skip("No agentic flags found".to_string());
    }

    if missing.locations.is_empty() {
        AuditStatus::Pass
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
        AuditStatus::Warn(evidence)
    }
}

struct MissingEnvResult {
    found_any: bool,
    found_agentic: usize,
    locations: Vec<SourceLocation>,
}

/// Find agentic flag attributes that lack `env = "..."`.
fn find_agentic_flags_missing_env(file: &SourceFile<'_, Rust>) -> MissingEnvResult {
    let label = file.label();
    let mut found_any = false;
    let mut found_agentic = 0;
    let mut missing = Vec::new();

    for attr_match in file.root().find_all(&*ARG_ATTR) {
        found_any = true;
        let attr_text = attr_match.text().to_string();

        let is_agentic = AGENTIC_FLAGS.iter().any(|flag| {
            attr_text.contains(&format!("long = \"{flag}\"")) || attr_text.contains(flag)
        });

        if !is_agentic {
            continue;
        }

        found_agentic += 1;

        // Audit if the attribute has `env = "..."` or just `env`
        if !attr_text.contains("env") {
            let pos = attr_match.start_pos();
            missing.push(SourceLocation {
                file: label.clone(),
                line: pos.line() + 1,
                column: pos.column(&attr_match) + 1,
                text: attr_text,
            });
        }
    }

    MissingEnvResult {
        found_any,
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
        let status = audit_env_flags(&pass::rust(source, "src/cli.rs"));
        assert_eq!(status, AuditStatus::Pass);
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
        let status = audit_env_flags(&pass::rust(source, "src/cli.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
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
        let status = audit_env_flags(&pass::rust(source, "src/main.rs"));
        assert!(matches!(status, AuditStatus::Skip(_)));
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
        let status = audit_env_flags(&pass::rust(source, "src/cli.rs"));
        assert!(matches!(status, AuditStatus::Skip(_)));
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
        let status = audit_env_flags(&pass::rust(source, "src/cli.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
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
        let status = audit_env_flags(&pass::rust(source, "src/cli.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn run_reports_only_the_agentic_file_when_another_holds_plain_args() {
        let dir = std::env::temp_dir().join(format!("anc-envflags-split-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        std::fs::write(
            dir.join("src/cli.rs"),
            "#[derive(Parser)]\nstruct Cli {\n    #[arg(long = \"output\", env = \"APP_OUTPUT\")]\n    output: Option<String>,\n    #[arg(long = \"quiet\")]\n    quiet: bool,\n}\n",
        )
        .expect("write test cli.rs");
        std::fs::write(
            dir.join("src/paths.rs"),
            "#[derive(Args)]\nstruct PathArgs {\n    #[arg(long = \"path\")]\n    path: String,\n}\n",
        )
        .expect("write test paths.rs");
        let project = Project::discover(&dir).expect("discover test project");

        let result = EnvFlagsAudit.run(&project).expect("run audit");

        let cli = dir.join("src/cli.rs");
        assert_eq!(
            result.status,
            AuditStatus::Warn(format!(
                "{}:5:5 — #[arg(long = \"quiet\")] missing env attribute",
                cli.display()
            ))
        );
    }

    #[test]
    fn run_skips_when_every_arg_attribute_is_non_agentic() {
        let dir = std::env::temp_dir().join(format!("anc-envflags-plain-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");
        std::fs::write(
            dir.join("src/paths.rs"),
            "#[derive(Args)]\nstruct PathArgs {\n    #[arg(long = \"path\")]\n    path: String,\n}\n",
        )
        .expect("write test paths.rs");
        let project = Project::discover(&dir).expect("discover test project");

        let result = EnvFlagsAudit.run(&project).expect("run audit");

        assert_eq!(
            result.status,
            AuditStatus::Skip("No clap #[arg(...)] attributes found".to_string())
        );
    }

    #[test]
    fn applicable_for_rust() {
        let audit = EnvFlagsAudit;
        let dir = std::env::temp_dir().join(format!("anc-envflags-rust-{}", std::process::id()));
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
        let audit = EnvFlagsAudit;
        let dir = std::env::temp_dir().join(format!("anc-envflags-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }
}
