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
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

/// Auth-related keywords to search for (case-insensitive on source text).
const AUTH_KEYWORDS: &[&str] = &["oauth", "token", "login", "authenticate", "authorization"];

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

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut has_auth_code = false;
        let mut has_headless_flag = false;

        for (_path, parsed_file) in parsed.iter() {
            let result = check_headless_auth(&parsed_file.source);
            match &result.status {
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
            id: "p1-headless-auth".to_string(),
            label: "Headless auth supported".to_string(),
            group: CheckGroup::P1,
            layer: CheckLayer::Source,
            status,
        })
    }
}

/// Check a single source string for auth code and headless flags.
pub(crate) fn check_headless_auth(source: &str) -> CheckResult {
    let lower = source.to_lowercase();
    let has_auth = AUTH_KEYWORDS.iter().any(|kw| lower.contains(kw));

    if !has_auth {
        return CheckResult {
            id: "p1-headless-auth".to_string(),
            label: "Headless auth supported".to_string(),
            group: CheckGroup::P1,
            layer: CheckLayer::Source,
            status: CheckStatus::Skip("no auth code found".to_string()),
        };
    }

    // Check for --no-browser or --headless flag in clap arg definitions
    let has_flag = has_pattern(source, r#"#[arg($$$ARGS)]"#)
        && (source.contains("no-browser")
            || source.contains("no_browser")
            || source.contains("headless"));

    let status = if has_flag {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(
            "Auth code detected but no --no-browser or --headless flag found. \
             Agents need a way to authenticate without a browser."
                .to_string(),
        )
    };

    CheckResult {
        id: "p1-headless-auth".to_string(),
        label: "Headless auth supported".to_string(),
        group: CheckGroup::P1,
        layer: CheckLayer::Source,
        status,
    }
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
        let result = check_headless_auth(source);
        assert!(matches!(result.status, CheckStatus::Skip(_)));
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
        let result = check_headless_auth(source);
        assert_eq!(result.status, CheckStatus::Pass);
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
        let result = check_headless_auth(source);
        assert_eq!(result.status, CheckStatus::Pass);
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
        let result = check_headless_auth(source);
        assert!(matches!(result.status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("no --no-browser"));
        }
    }

    #[test]
    fn skip_when_token_only_in_unrelated_context() {
        // "token" appears but it's auth-related per our keyword list,
        // so this should warn (no headless flag)
        let source = r#"
fn parse_token(s: &str) -> Token {
    Token::new(s)
}
"#;
        let result = check_headless_auth(source);
        // "token" is detected as auth code
        assert!(matches!(result.status, CheckStatus::Warn(_)));
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
