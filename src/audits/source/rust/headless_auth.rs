//! Audit: Detect auth code that lacks a headless/no-browser flag.
//!
//! Principle: P1 (Non-Interactive) — Auth flows should support headless mode
//! so agents can authenticate without a browser.
//!
//! This is a conditional audit:
//!   Trigger: the source contains auth-related code (OAuth, token, login, etc.)
//!   Requirement: a `--no-browser` or `--headless` clap flag must exist

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::any_match;
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

/// Function definitions for every common visibility + async combination,
/// each with the byte offset where the function name starts in its matched
/// text, past the leading `pub fn `, `async fn `, etc., so the slice up to
/// the first `(` is the bare identifier.
///
/// The set is required because ast-grep's tree-sitter pattern compiler
/// treats `fn $NAME(...)` as a literal prefix that does NOT match
/// `pub fn $NAME(...)`: a Rust library exposing `pub fn` auth APIs would
/// otherwise be invisible to the audit, leaving consumers stuck at "no auth
/// code found" Skip even though headless-auth support is needed.
const AUTH_FN_SHAPES: [(&str, usize); 7] = [
    ("fn $NAME($$$ARGS) $$$BODY", 3),
    ("pub fn $NAME($$$ARGS) $$$BODY", 7),
    ("pub(crate) fn $NAME($$$ARGS) $$$BODY", 14),
    ("pub(super) fn $NAME($$$ARGS) $$$BODY", 14),
    ("async fn $NAME($$$ARGS) $$$BODY", 9),
    ("pub async fn $NAME($$$ARGS) $$$BODY", 13),
    ("pub(crate) async fn $NAME($$$ARGS) $$$BODY", 20),
];

static AUTH_FN_PATTERNS: LazyLock<[Pattern; 7]> =
    LazyLock::new(|| AUTH_FN_SHAPES.map(|(source, _)| Pattern::new(source, Rust)));

static ARG_ATTR: LazyLock<Pattern> = LazyLock::new(|| Pattern::new("#[arg($$$ARGS)]", Rust));

const MISSING_FLAG: &str = "Auth code detected but no --no-browser or --headless flag found. \
                            Agents need a way to authenticate without a browser.";

/// Audit trait implementation for headless auth detection.
#[derive(Debug)]
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
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        // Cross-file scan: auth code and the flag often live in different
        // files in well-structured Rust projects (auth logic in src/auth/,
        // CLI flags in src/cli/). Check both signals independently across
        // the whole parsed set, then combine at the project level.
        let files = pass::findings(project, Language::Rust, Rust, self);
        let has_auth_code = files.iter().any(|(_, file)| file.auth_code);
        let has_headless_flag = files.iter().any(|(_, file)| file.headless_flag);

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status: verdict(has_auth_code, has_headless_flag),
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

impl SourceScan<Rust> for HeadlessAuthAudit {
    type Finding = AuthSignals;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> AuthSignals {
        auth_signals(file)
    }
}

/// What one file shows about auth code and a headless flag.
#[derive(Debug, Clone, Copy)]
pub struct AuthSignals {
    auth_code: bool,
    headless_flag: bool,
}

fn auth_signals(file: &SourceFile<'_, Rust>) -> AuthSignals {
    AuthSignals {
        auth_code: has_auth_functions(file),
        headless_flag: has_headless_flag_definition(file),
    }
}

fn verdict(has_auth_code: bool, has_headless_flag: bool) -> AuditStatus {
    if !has_auth_code {
        AuditStatus::Skip("no auth code found".to_string())
    } else if has_headless_flag {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(MISSING_FLAG.to_string())
    }
}

/// Detect a `#[arg(...)]` clap definition for `--no-browser` / `--headless`.
fn has_headless_flag_definition(file: &SourceFile<'_, Rust>) -> bool {
    let source = file.source();
    (source.contains("no-browser") || source.contains("no_browser") || source.contains("headless"))
        && file.root().find_all(&*ARG_ATTR).next().is_some()
}

/// Search for function definitions whose names contain auth-related keywords.
///
/// Restricting to definitions (not just any identifier mention) avoids
/// false positives from comments, string literals, and constant arrays.
fn has_auth_functions(file: &SourceFile<'_, Rust>) -> bool {
    any_match(&file.root(), &*AUTH_FN_PATTERNS, |index, m| {
        names_auth_function(&m.text(), AUTH_FN_SHAPES[index].1)
    })
}

/// Whether the function definition `text`, whose name starts at byte
/// `skip`, is named for auth.
fn names_auth_function(text: &str, skip: usize) -> bool {
    if text.len() <= skip {
        return false;
    }
    #[expect(
        clippy::string_slice,
        reason = "skip is the byte length of the pattern's ASCII literal prefix"
    )]
    let Some(name_end) = text[skip..].find('(') else {
        return false;
    };
    #[expect(
        clippy::string_slice,
        reason = "skip is an ASCII prefix length and name_end comes from find('(')"
    )]
    let fn_name = text[skip..(skip + name_end)].trim();
    let lower_name = fn_name.to_lowercase();
    AUTH_IDENT_KEYWORDS.iter().any(|kw| lower_name.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The verdict a project made of this one file gets.
    pub(super) fn audit_headless_auth(file: &SourceFile<'_, Rust>) -> AuditStatus {
        let found = auth_signals(file);
        verdict(found.auth_code, found.headless_flag)
    }

    #[test]
    fn skip_when_no_auth_code() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let status = audit_headless_auth(&pass::rust(source, "src/auth.rs"));
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
        let status = audit_headless_auth(&pass::rust(source, "src/auth.rs"));
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
        let status = audit_headless_auth(&pass::rust(source, "src/auth.rs"));
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
        let status = audit_headless_auth(&pass::rust(source, "src/auth.rs"));
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
        let status = audit_headless_auth(&pass::rust(source, "src/auth.rs"));
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
    use super::tests::audit_headless_auth;
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
        assert_eq!(
            audit_headless_auth(&pass::rust(source, "src/auth.rs")),
            AuditStatus::Pass
        );
    }

    #[test]
    fn warn_when_pub_fn_oauth_no_flag() {
        let source = r#"
pub fn get_oauth2_scopes() -> Vec<&'static str> { vec![] }
pub fn refresh_oauth2_token() {}
"#;
        let status = audit_headless_auth(&pass::rust(source, "src/auth.rs"));
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
        assert_eq!(
            audit_headless_auth(&pass::rust(source, "src/auth.rs")),
            AuditStatus::Pass
        );
    }

    #[test]
    fn warn_when_pub_crate_fn_auth_url_no_flag() {
        let source = r#"
pub(crate) fn build_auth_url() {}
"#;
        let status = audit_headless_auth(&pass::rust(source, "src/auth.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)), "got {status:?}");
    }
}
