//! Audit: Detect auth code that lacks a headless/no-browser flag.
//!
//! Principle: P1 (Non-Interactive) — Auth flows should support headless mode
//! so agents can authenticate without a browser.
//!
//! This is a conditional audit:
//!   Trigger: the source contains auth-related code (OAuth, token, login, etc.)
//!   Requirement: a `--no-browser` or `--headless` clap flag must exist

use crate::audit::Audit;
use crate::project::{Language, Project};
use crate::source::has_pattern;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Auth-related substrings to search for in Rust identifiers (not string
/// literals or comments). We search function definitions via ast-grep to
/// avoid false positives from prose.
const AUTH_IDENT_KEYWORDS: &[&str] = &[
    "oauth",
    "auth_token",
    "access_token",
    "refresh_token",
    "auth_flow",
    "auth_url",
    "authenticate",
    "authorization",
];

/// Audit trait implementation for headless auth detection.
pub struct HeadlessAuthAudit;

impl Audit for HeadlessAuthAudit {
    fn id(&self) -> &str {
        "p1-headless-auth"
    }

    fn label(&self) -> &'static str {
        "Headless auth supported"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P1
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-no-browser"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let parsed = project.parsed_files();
        let mut has_auth_code = false;
        let mut has_headless_flag = false;

        // Cross-file scan: auth code and the flag often live in different
        // files in well-structured Rust projects (auth logic in src/auth/,
        // CLI flags in src/cli/). Check both signals independently across
        // the whole parsed set, then combine at the project level.
        for (_path, parsed_file) in parsed.iter() {
            if !has_auth_code && has_auth_functions(&parsed_file.source) {
                has_auth_code = true;
            }
            if !has_headless_flag && has_headless_flag_definition(&parsed_file.source) {
                has_headless_flag = true;
            }
            if has_auth_code && has_headless_flag {
                break;
            }
        }

        let status = if !has_auth_code {
            AuditStatus::Skip("no auth code found".to_string())
        } else if has_headless_flag {
            AuditStatus::Pass
        } else {
            AuditStatus::Warn(
                "Auth code detected but no --no-browser or --headless flag found. \
                 Agents need a way to authenticate without a browser."
                    .to_string(),
            )
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: None,
        })
    }
}

/// Audit a single source string for auth code and headless flags. Kept as a
/// single-file convenience for callers (and existing unit tests) that want
/// to assert on one source string in isolation. The project-level audit at
/// [`HeadlessAuthAudit::run`] uses the two split helpers directly so it can
/// detect the flag in a different file from the auth code — a common shape
/// in well-structured Rust projects.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn audit_headless_auth(source: &str) -> AuditStatus {
    let has_auth = has_auth_functions(source);

    if !has_auth {
        return AuditStatus::Skip("no auth code found".to_string());
    }

    if has_headless_flag_definition(source) {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(
            "Auth code detected but no --no-browser or --headless flag found. \
             Agents need a way to authenticate without a browser."
                .to_string(),
        )
    }
}

/// Detect a `#[arg(...)]` clap definition for `--no-browser` / `--headless`.
/// Used by the project-level audit to scan every file independently of
/// whether that file also carries auth functions.
fn has_headless_flag_definition(source: &str) -> bool {
    has_pattern(source, r#"#[arg($$$ARGS)]"#)
        && (source.contains("no-browser")
            || source.contains("no_browser")
            || source.contains("headless"))
}

/// Search for function definitions whose names contain auth-related keywords.
///
/// Uses ast-grep with a pattern set covering every common visibility + async
/// combination in Rust function definitions. The set is required because
/// ast-grep's tree-sitter pattern compiler treats `fn $NAME(...)` as a
/// literal prefix that does NOT match `pub fn $NAME(...)` — a Rust library
/// exposing `pub fn` auth APIs would otherwise be invisible to the audit,
/// leaving consumers stuck at "no auth code found" Skip even though
/// headless-auth support is needed.
///
/// Restricting to definitions (not just any identifier mention) avoids
/// false positives from comments, string literals, and constant arrays.
fn has_auth_functions(source: &str) -> bool {
    use ast_grep_core::Pattern;
    use ast_grep_core::tree_sitter::LanguageExt;
    use ast_grep_language::Rust;

    // Each entry is (pattern_source, prefix_byte_offset). The offset is
    // where the function name starts in the matched text — skip past the
    // leading `pub fn `, `async fn `, etc. so the slice up to the first
    // `(` is the bare identifier.
    let pattern_set: &[(&str, usize)] = &[
        ("fn $NAME($$$ARGS) $$$BODY", 3),
        ("pub fn $NAME($$$ARGS) $$$BODY", 7),
        ("pub(crate) fn $NAME($$$ARGS) $$$BODY", 14),
        ("pub(super) fn $NAME($$$ARGS) $$$BODY", 14),
        ("async fn $NAME($$$ARGS) $$$BODY", 9),
        ("pub async fn $NAME($$$ARGS) $$$BODY", 13),
        ("pub(crate) async fn $NAME($$$ARGS) $$$BODY", 20),
    ];

    let root = Rust.ast_grep(source);
    for (pattern_src, skip) in pattern_set {
        let Ok(pattern) = Pattern::try_new(pattern_src, Rust) else {
            continue;
        };
        for m in root.root().find_all(&pattern) {
            let text = m.text();
            if text.len() <= *skip {
                continue;
            }
            let Some(name_end) = text[*skip..].find('(') else {
                continue;
            };
            let fn_name = text[*skip..(*skip + name_end)].trim();
            let lower_name = fn_name.to_lowercase();
            if AUTH_IDENT_KEYWORDS.iter().any(|kw| lower_name.contains(kw)) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_auth_code() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let status = audit_headless_auth(source);
        assert!(matches!(status, AuditStatus::Skip(_)));
    }

    #[test]
    fn pass_when_headless_flag_exists() {
        let source = r#"
use clap::Parser;

fn do_oauth_flow() {
    // OAuth logic here
}

#[derive(Parser)]
struct Cli {
    #[arg(long = "no-browser")]
    no_browser: bool,
}
"#;
        let status = audit_headless_auth(source);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn pass_with_headless_flag() {
        let source = r#"
use clap::Parser;

fn authenticate() {
    // token exchange
}

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    headless: bool,
}
"#;
        let status = audit_headless_auth(source);
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_auth_code_but_no_flag() {
        let source = r#"
use clap::Parser;

fn do_oauth_flow() {
    // OAuth token exchange
}

#[derive(Parser)]
struct Cli {
    #[arg(long)]
    verbose: bool,
}
"#;
        let status = audit_headless_auth(source);
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
            assert!(evidence.contains("no --no-browser"));
        }
    }

    #[test]
    fn skip_when_token_only_in_unrelated_context() {
        // Bare "token" no longer triggers auth detection — the keyword list
        // requires compound auth terms like "auth_token" or "access_token"
        // to reduce false positives.
        let source = r#"
fn parse_token(s: &str) -> Token {
    Token::new(s)
}
"#;
        let status = audit_headless_auth(source);
        assert!(matches!(status, AuditStatus::Skip(_)));
    }

    #[test]
    fn applicable_for_rust() {
        let audit = HeadlessAuthAudit;
        let dir = std::env::temp_dir().join(format!("anc-headless-rust-{}", std::process::id()));
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
        let audit = HeadlessAuthAudit;
        let dir = std::env::temp_dir().join(format!("anc-headless-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }
}

#[cfg(test)]
mod pub_fn_tests {
    use super::*;

    #[test]
    fn pass_when_pub_fn_oauth_and_flag() {
        let source = r#"
use clap::Parser;
pub fn run_oauth2_flow() {}
#[derive(Parser)]
struct Cli {
    #[arg(long = "no-browser")]
    no_browser: bool,
}
"#;
        assert_eq!(audit_headless_auth(source), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_pub_fn_oauth_no_flag() {
        let source = r#"
pub fn get_oauth2_scopes() -> Vec<&'static str> { vec![] }
pub fn refresh_oauth2_token() {}
"#;
        let status = audit_headless_auth(source);
        assert!(matches!(status, AuditStatus::Warn(_)), "got {status:?}");
    }

    #[test]
    fn pass_when_async_fn_authenticate_and_flag() {
        let source = r#"
use clap::Parser;
pub async fn authenticate() {}
#[derive(Parser)]
struct Cli {
    #[arg(long)]
    headless: bool,
}
"#;
        assert_eq!(audit_headless_auth(source), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_pub_crate_fn_auth_url_no_flag() {
        let source = r#"
pub(crate) fn build_auth_url() {}
"#;
        let status = audit_headless_auth(source);
        assert!(matches!(status, AuditStatus::Warn(_)), "got {status:?}");
    }
}
