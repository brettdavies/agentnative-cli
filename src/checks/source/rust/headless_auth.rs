//! Check: Detect auth code that lacks a headless/no-browser flag.
//!
//! Principle: P1 (Non-Interactive) — Auth flows should support headless mode
//! so agents can authenticate without a browser.
//!
//! This is a conditional check:
//!   Trigger: the source contains auth-related code (OAuth, token, login, etc.)
//!   Requirement: a `--no-browser` or `--headless` clap flag must exist

use crate::check::Check;
use crate::project::{Language, Project};
use crate::source::has_pattern;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

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

/// Check trait implementation for headless auth detection.
pub struct HeadlessAuthCheck;

impl Check for HeadlessAuthCheck {
    fn id(&self) -> &str {
        "p1-headless-auth"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P1
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-no-browser"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut has_auth_code = false;
        let mut has_headless_flag = false;

        for (_path, parsed_file) in parsed.iter() {
            match &check_headless_auth(&parsed_file.source) {
                CheckStatus::Skip(_) => {
                    // No auth code in this file
                }
                CheckStatus::Pass => {
                    has_auth_code = true;
                    has_headless_flag = true;
                }
                CheckStatus::Warn(_) => {
                    has_auth_code = true;
                }
                _ => {}
            }
        }

        let status = if !has_auth_code {
            CheckStatus::Skip("no auth code found".to_string())
        } else if has_headless_flag {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(
                "Auth code detected but no --no-browser or --headless flag found. \
                 Agents need a way to authenticate without a browser."
                    .to_string(),
            )
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Headless auth supported".to_string(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// Check a single source string for auth code and headless flags.
///
/// Searches function definitions via ast-grep to find auth-related identifiers.
/// This avoids false positives from comments, string literals, and constant arrays.
pub(crate) fn check_headless_auth(source: &str) -> CheckStatus {
    let has_auth = has_auth_functions(source);

    if !has_auth {
        return CheckStatus::Skip("no auth code found".to_string());
    }

    // Check for --no-browser or --headless flag in clap arg definitions
    let has_flag = has_pattern(source, r#"#[arg($$$ARGS)]"#)
        && (source.contains("no-browser")
            || source.contains("no_browser")
            || source.contains("headless"));

    if has_flag {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(
            "Auth code detected but no --no-browser or --headless flag found. \
             Agents need a way to authenticate without a browser."
                .to_string(),
        )
    }
}

/// Search for function definitions whose names contain auth-related keywords.
///
/// Uses ast-grep to find `fn $NAME(...)` patterns, then checks if the function
/// name contains any auth keyword. This avoids matching keywords in comments,
/// strings, or constant arrays.
fn has_auth_functions(source: &str) -> bool {
    use ast_grep_core::Pattern;
    use ast_grep_core::tree_sitter::LanguageExt;
    use ast_grep_language::Rust;

    let pattern = match Pattern::try_new("fn $NAME($$$ARGS) $$$BODY", Rust) {
        Ok(p) => p,
        Err(_) => return false,
    };

    let root = Rust.ast_grep(source);
    for m in root.root().find_all(&pattern) {
        let text = m.text();
        if let Some(name_end) = text.find('(') {
            let fn_name = text[3..name_end].trim(); // skip "fn "
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
        let status = check_headless_auth(source);
        assert!(matches!(status, CheckStatus::Skip(_)));
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
        let status = check_headless_auth(source);
        assert_eq!(status, CheckStatus::Pass);
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
        let status = check_headless_auth(source);
        assert_eq!(status, CheckStatus::Pass);
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
        let status = check_headless_auth(source);
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
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
        let status = check_headless_auth(source);
        assert!(matches!(status, CheckStatus::Skip(_)));
    }

    #[test]
    fn applicable_for_rust() {
        let check = HeadlessAuthCheck;
        let dir = std::env::temp_dir().join(format!("anc-headless-rust-{}", std::process::id()));
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
        let check = HeadlessAuthCheck;
        let dir = std::env::temp_dir().join(format!("anc-headless-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
