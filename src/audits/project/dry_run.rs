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

/// HTTP client library import/usage patterns that indicate remote operations.
/// Used to gate the more specific method detection.
const HTTP_CLIENT_SIGNALS: &[&str] = &[
    "reqwest::",
    "use reqwest",
    "axios",
    "node-fetch",
    "got(",
    "undici",
    "fetch(",
    "hyper::",
    "ureq::",
];

/// HTTP mutating method names. When these appear as method calls on an HTTP client,
/// they indicate remote write operations.
const HTTP_MUTATING_METHODS: &[&str] = &["post", "put", "patch", "delete"];

/// Method string literals in fetch/axios options that indicate mutating operations.
const HTTP_METHOD_LITERALS: &[&str] = &[
    "\"POST\"",
    "\"PUT\"",
    "\"PATCH\"",
    "\"DELETE\"",
    "'POST'",
    "'PUT'",
    "'PATCH'",
    "'DELETE'",
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
                    || has_remote_write_calls(source);
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

/// Check if source contains remote HTTP write calls.
/// More robust than substring matching: gates method detection on HTTP client presence
/// and looks for method calls in context.
fn has_remote_write_calls(source: &str) -> bool {
    // First, check if an HTTP client is present
    let has_http_client = HTTP_CLIENT_SIGNALS.iter().any(|sig| source.contains(sig));
    if !has_http_client {
        return false;
    }

    // Check for HTTP method string literals (in fetch/axios options)
    if HTTP_METHOD_LITERALS.iter().any(|lit| source.contains(lit)) {
        return true;
    }

    // Check for HTTP mutating method calls.
    // To avoid false positives from Map.delete(), Set.delete(), cache.put(), etc.,
    // we look for patterns that suggest HTTP client method calls:
    // - Explicitly named clients: client.post(, http.delete(, api.put(
    // - Chain after await/response: .then(...).post(, response.delete(
    // - Common HTTP client variables: req.post(, request.put(
    for method in HTTP_MUTATING_METHODS {
        let patterns = [
            format!("client.{}(", method),
            format!("http.{}(", method),
            format!("api.{}(", method),
            format!("req.{}(", method),
            format!("request.{}(", method),
            format!("response.{}(", method),
            // Rust: Client::new().post(...), Client::builder().delete(...)
            format!("Client::new().{}(", method),
            format!("Client::builder().{}(", method),
            // Method chaining after await: .await?.post(, .await.delete(
            format!(".await.{}(", method),
            format!(".await?.{}(", method),
        ];

        if patterns.iter().any(|pat| source.contains(pat)) {
            return true;
        }
    }

    false
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
        // Each needs HTTP client context to avoid false positives
        let test_cases = vec![
            (
                "reqwest-post",
                "use reqwest; fn main() { let client = reqwest::Client::new(); client.post(url).send(); }",
            ),
            (
                "reqwest-delete",
                "use reqwest; fn main() { let http = reqwest::Client::new(); http.delete(url).send(); }",
            ),
            (
                "fetch-post",
                "use reqwest; async fn main() { let resp = fetch(url).await?.post(url); }",
            ),
        ];

        for (name, source) in test_cases {
            let dir = temp_dir(&format!("remote-{}", name));
            let project = make_project(&dir, &[("main.rs", source)]);
            let result = DryRunAudit.run(&project).expect("run audit");
            assert!(
                !matches!(result.status, AuditStatus::Skip(_)),
                "pattern '{}' should detect remote writes, got {:?}",
                name,
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

    #[test]
    fn skip_when_map_delete_is_collection_method() {
        // Negative test: Map.delete(), Set.delete() should not trigger remote write detection
        let dir = temp_dir("map-delete");
        let project = make_project(
            &dir,
            &[(
                "main.rs",
                r#"
use std::collections::HashMap;

fn main() {
    let mut map = HashMap::new();
    map.insert("key", "value");
    
    // This is a collection delete, not an HTTP delete
    map.delete(&"key");
    
    let mut set = std::collections::HashSet::new();
    set.insert("item");
    set.delete(&"item");
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert!(
            matches!(result.status, AuditStatus::Skip(_)),
            "Map.delete()/Set.delete() should Skip, got {:?}",
            result.status
        );
    }

    #[test]
    fn skip_when_cache_put_is_collection_method() {
        // Negative test: cache.put() should not trigger remote write detection
        let dir = temp_dir("cache-put");
        let project = make_project(
            &dir,
            &[(
                "main.rs",
                r#"
struct Cache {
    items: Vec<String>,
}

impl Cache {
    fn put(&mut self, item: String) {
        self.items.push(item);
    }
}

fn main() {
    let mut cache = Cache { items: vec![] };
    // This is a local cache operation, not an HTTP PUT
    cache.put("data".to_string());
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert!(
            matches!(result.status, AuditStatus::Skip(_)),
            "cache.put() should Skip, got {:?}",
            result.status
        );
    }

    #[test]
    fn warn_when_reqwest_client_post_without_dry_run() {
        // Positive test: actual HTTP client.post() should be detected
        let dir = temp_dir("reqwest-post");
        let project = make_project(
            &dir,
            &[(
                "main.rs",
                r#"
use reqwest;

fn main() {
    let client = reqwest::blocking::Client::new();
    let response = client.post("https://api.example.com/data")
        .json(&data())
        .send()
        .unwrap();
}
"#,
            )],
        );
        let result = DryRunAudit.run(&project).expect("run audit");
        assert!(
            matches!(result.status, AuditStatus::Warn(_)),
            "reqwest client.post() without dry-run should Warn, got {:?}",
            result.status
        );
    }
}
