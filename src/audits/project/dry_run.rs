//! Audit: Detect whether CLIs with write/mutate commands offer a --dry-run flag.
//!
//! Principle: P5 (Safe Retries) — CLIs that perform write operations should support
//! --dry-run so agents can preview changes before committing them.
//!
//! Heuristic approach:
//!   1. Scan source files for clap arg definitions containing write/mutate keywords
//!   2. If none found → Skip (no write operations detected)
//!   3. If found, search for --dry-run flag definition
//!   4. Pass if --dry-run exists, Warn if missing

use std::fs;

use crate::audit::Audit;
use crate::project::Project;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Keywords in clap arg definitions that indicate write/mutate operations.
const WRITE_KEYWORDS: &[&str] = &[
    "--write",
    "--delete",
    "--create",
    "--update",
    "--remove",
    "--deploy",
    "--install",
    "--push",
    "\"write\"",
    "\"delete\"",
    "\"create\"",
    "\"update\"",
    "\"remove\"",
    "\"deploy\"",
    "\"install\"",
    "\"push\"",
];

/// Patterns that indicate actual remote mutating HTTP calls.
/// These are method calls on HTTP clients, not just URLs or API key declarations.
/// Must be specific enough to avoid false positives from doc comments or manifests.
const REMOTE_WRITE_PATTERNS: &[&str] = &[
    // HTTP client method calls with mutating verbs
    "client.post(",
    "client.put(",
    "client.patch(",
    "client.delete(",
    ".post(", // reqwest, axios, fetch
    ".put(",
    ".patch(",
    ".delete(",
    // Node/JS patterns
    "fetch(url, { method: 'POST'",
    "fetch(url, { method: 'PUT'",
    "fetch(url, { method: 'PATCH'",
    "fetch(url, { method: 'DELETE'",
    "fetch(url, {method:'POST'",
    "fetch(url, {method:'PUT'",
    "fetch(url, {method:'PATCH'",
    "fetch(url, {method:'DELETE'",
    "method: \"POST\"",
    "method: \"PUT\"",
    "method: \"PATCH\"",
    "method: \"DELETE\"",
];

/// Patterns indicating a --dry-run flag exists.
const DRY_RUN_PATTERNS: &[&str] = &["dry-run", "dry_run", "dryrun"];

pub struct DryRunAudit;

impl Audit for DryRunAudit {
    fn id(&self) -> &str {
        "p5-dry-run"
    }

    fn label(&self) -> &'static str {
        "Dry-run flag for write operations"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P5
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Project
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p5-must-dry-run"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.path.is_dir()
            && (project.language.is_some() || project.manifest_skip(None).is_some())
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        if let Some(reason) = project.manifest_skip(None) {
            return Ok(self.skip(reason));
        }
        let Some(language) = project.language else {
            return Ok(self.skip("no language detected".into()));
        };
        let parsed = project.parsed_files(language);

        let mut has_write_commands = false;
        let mut has_dry_run = false;

        for (_path, parsed_file) in parsed.iter() {
            let source = &parsed_file.source;
            if !has_write_commands {
                has_write_commands = WRITE_KEYWORDS.iter().any(|kw| source.contains(kw))
                    || REMOTE_WRITE_PATTERNS.iter().any(|pat| source.contains(pat));
            }
            if !has_dry_run {
                has_dry_run = DRY_RUN_PATTERNS.iter().any(|pat| source.contains(pat));
            }
            if has_write_commands && has_dry_run {
                break;
            }
        }

        // Also audit the manifest for subcommand names that imply writes,
        // but NOT for remote patterns (manifests contain URLs in metadata).
        if !has_write_commands
            && let Some(manifest) = &project.manifest_path
            && let Ok(content) = fs::read_to_string(manifest)
        {
            has_write_commands = WRITE_KEYWORDS.iter().any(|kw| content.contains(kw));
        }

        let status = if !has_write_commands {
            AuditStatus::Skip("No write/mutate commands detected".into())
        } else if has_dry_run {
            AuditStatus::Pass
        } else {
            AuditStatus::Warn("Write/mutate commands detected but no --dry-run flag found".into())
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use crate::project::{Language, ParsedFile};

    fn temp_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anc-dryrun-{suffix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after UNIX epoch")
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    fn make_project(dir: &std::path::Path, files: &[(&str, &str)]) -> Project {
        let src = dir.join("src");
        std::fs::create_dir_all(&src).expect("create src dir");

        let mut parsed = HashMap::new();
        for (name, content) in files {
            let path = src.join(name);
            std::fs::write(&path, content).expect("write test source file");
            parsed.insert(
                path,
                ParsedFile {
                    source: content.to_string(),
                },
            );
        }

        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");

        Project {
            path: dir.to_path_buf(),
            language: Some(Language::Rust),
            binary_paths: vec![],
            manifest_path: Some(dir.join("Cargo.toml")),
            runner: None,
            include_tests: false,
            parsed_files: OnceLock::from(HashMap::from([(Language::Rust, parsed)])),
            help_output: OnceLock::new(),
            anc_config: Default::default(),
            inventory: Default::default(),
        }
    }

    #[test]
    fn applicable_when_language_detected() {
        let dir = temp_dir("applicable");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(DryRunAudit.applicable(&project));
    }

    #[test]
    fn not_applicable_without_language() {
        let dir = temp_dir("no-lang");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!DryRunAudit.applicable(&project));
    }

    #[test]
    fn skip_when_no_write_commands() {
        let dir = temp_dir("no-write");
        let project = make_project(
            &dir,
            &[(
                "main.rs",
                r#"
fn main() {
    let args = Cli::parse();
    println!("reading data");
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert!(matches!(result.status, AuditStatus::Skip(_)));
    }

    #[test]
    fn pass_when_dry_run_present() {
        let dir = temp_dir("pass");
        let project = make_project(
            &dir,
            &[(
                "cli.rs",
                r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "delete")]
    delete: bool,

    #[arg(long = "dry-run")]
    dry_run: bool,
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_write_without_dry_run() {
        let dir = temp_dir("warn");
        let project = make_project(
            &dir,
            &[(
                "cli.rs",
                r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "delete")]
    delete: bool,

    #[arg(long)]
    force: bool,
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert!(matches!(result.status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("dry-run"));
        }
    }

    #[test]
    fn detects_write_from_multiple_keywords() {
        for keyword in &[
            "--deploy",
            "--install",
            "--push",
            "\"create\"",
            "\"update\"",
        ] {
            let dir = temp_dir(&format!("kw-{}", keyword.replace('"', "")));
            let source = format!("let flag = \"{keyword}\";");
            let project = make_project(&dir, &[("main.rs", &source)]);
            let result = DryRunAudit.run(&project).expect("run audit");
            assert!(
                !matches!(result.status, AuditStatus::Skip(_)),
                "keyword {keyword} should be detected as a write command"
            );
        }
    }

    #[test]
    fn pass_with_dry_run_underscore_variant() {
        let dir = temp_dir("underscore");
        let project = make_project(
            &dir,
            &[(
                "cli.rs",
                r#"
struct Cli {
    #[arg(long = "delete")]
    delete: bool,

    #[arg(long)]
    dry_run: bool,
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn metadata_is_correct() {
        let audit = DryRunAudit;
        assert_eq!(audit.id(), "p5-dry-run");
        assert_eq!(audit.group(), AuditGroup::P5);
        assert_eq!(audit.layer(), AuditLayer::Project);
    }

    #[test]
    fn warn_when_remote_api_write_without_dry_run() {
        // Regression test: detect remote side-effecting CLIs that write by default
        let dir = temp_dir("remote-api");
        let project = make_project(
            &dir,
            &[(
                "main.rs",
                r#"
use reqwest;

fn main() {
    let client = reqwest::blocking::Client::new();
    
    // This CLI posts to a remote API by default
    let response = client.post("https://api.example.com/reports")
        .json(&report_data())
        .send()
        .unwrap();
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert!(
            matches!(result.status, AuditStatus::Warn(_)),
            "expected Warn for remote API writes without dry-run, got {:?}",
            result.status
        );
    }

    #[test]
    fn pass_when_remote_api_has_dry_run() {
        let dir = temp_dir("remote-api-dry");
        let project = make_project(
            &dir,
            &[(
                "cli.rs",
                r#"
#[derive(Parser)]
struct Cli {
    #[arg(long)]
    dry_run: bool,
}

fn main() {
    let args = Cli::parse();
    let client = reqwest::blocking::Client::new();
    
    if !args.dry_run {
        client.post("https://api.example.com/reports")
            .send()
            .unwrap();
    }
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert_eq!(result.status, AuditStatus::Pass);
    }

    #[test]
    fn detects_various_remote_write_patterns() {
        // Test different patterns that indicate remote writes
        for pattern in &["client.post(", ".put(url)", "client.delete("] {
            let dir = temp_dir(&format!(
                "remote-{}",
                pattern
                    .chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>()
            ));
            let source = format!("fn main() {{ {} }}", pattern);
            let project = make_project(&dir, &[("main.rs", &source)]);
            let result = DryRunAudit.run(&project).expect("run audit");
            assert!(
                !matches!(result.status, AuditStatus::Skip(_)),
                "pattern '{}' should detect remote writes, got {:?}",
                pattern,
                result.status
            );
        }
    }

    #[test]
    fn skip_when_cargo_toml_has_homepage_url() {
        // Negative test: Cargo.toml with homepage URL should not be flagged
        let dir = temp_dir("homepage-url");
        std::fs::create_dir_all(dir.join("src")).expect("create src dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "test"
version = "0.1.0"
homepage = "https://example.com"
repository = "https://github.com/user/repo"
"#,
        )
        .expect("write Cargo.toml");
        std::fs::write(
            dir.join("src/main.rs"),
            r#"fn main() {
    println!("Hello, world!");
}
"#,
        )
        .expect("write main.rs");

        let project = Project::discover(&dir).expect("discover project");
        let result = DryRunAudit.run(&project).expect("run audit");
        assert!(
            matches!(result.status, AuditStatus::Skip(_)),
            "Cargo.toml with homepage URL should Skip, got {:?}",
            result.status
        );
    }

    #[test]
    fn skip_when_only_get_requests() {
        // Negative test: read-only GET client should not be flagged
        let dir = temp_dir("get-only");
        let project = make_project(
            &dir,
            &[(
                "main.rs",
                r#"
use reqwest;

fn main() {
    let client = reqwest::blocking::Client::new();
    // Read-only GET request
    let response = client.get("https://api.example.com/data")
        .send()
        .unwrap();
    println!("{:?}", response);
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert!(
            matches!(result.status, AuditStatus::Skip(_)),
            "read-only GET client should Skip, got {:?}",
            result.status
        );
    }

    #[test]
    fn skip_when_doc_comment_has_url() {
        // Negative test: doc comment with URL should not be flagged
        let dir = temp_dir("doc-url");
        let project = make_project(
            &dir,
            &[(
                "lib.rs",
                r#"
/// This library interacts with the API at https://api.example.com
/// 
/// See the documentation for more details about the endpoint.
/// API keys can be obtained from your account page.
pub fn fetch_data() -> String {
    "data".to_string()
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert!(
            matches!(result.status, AuditStatus::Skip(_)),
            "doc comment with URL should Skip, got {:?}",
            result.status
        );
    }
}
