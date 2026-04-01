//! Check: Detect network code without a `--timeout` flag.
//!
//! Principle: P6 (Composable Structure) — Network-calling CLIs should expose
//! a `--timeout` flag so agents can bound execution time.
//!
//! This is a conditional check:
//!   Trigger: the source uses network libraries (reqwest, hyper, curl, ureq)
//!   Requirement: a `--timeout` flag must exist

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

/// Network library indicators to search for in source text.
const NETWORK_INDICATORS: &[&str] = &["reqwest", "hyper", "curl", "ureq"];

/// Check trait implementation for timeout flag detection.
pub struct TimeoutFlagCheck;

impl Check for TimeoutFlagCheck {
    fn id(&self) -> &str {
        "p6-timeout"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut has_network = false;
        let mut has_timeout_flag = false;

        for (_path, parsed_file) in parsed.iter() {
            let result = check_timeout_flag(&parsed_file.source);
            match &result.status {
                CheckStatus::Skip(_) => {
                    // No network code in this file
                }
                CheckStatus::Pass => {
                    has_network = true;
                    has_timeout_flag = true;
                }
                CheckStatus::Warn(_) => {
                    has_network = true;
                }
                _ => {}
            }
        }

        let status = if !has_network {
            CheckStatus::Skip("no network code detected".to_string())
        } else if has_timeout_flag {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(
                "Network code detected but no --timeout flag found. \
                 Agents need to bound execution time for network operations."
                    .to_string(),
            )
        };

        Ok(CheckResult {
            id: "p6-timeout".to_string(),
            label: "Timeout flag for network ops".to_string(),
            group: CheckGroup::P6,
            layer: CheckLayer::Source,
            status,
        })
    }
}

/// Check a single source string for network code and timeout flag.
pub(crate) fn check_timeout_flag(source: &str) -> CheckResult {
    let has_network = NETWORK_INDICATORS.iter().any(|ind| source.contains(ind));

    if !has_network {
        return CheckResult {
            id: "p6-timeout".to_string(),
            label: "Timeout flag for network ops".to_string(),
            group: CheckGroup::P6,
            layer: CheckLayer::Source,
            status: CheckStatus::Skip("no network code detected".to_string()),
        };
    }

    // Check for --timeout flag in source
    let has_flag = source.contains("timeout")
        && (source.contains("--timeout")
            || source.contains("\"timeout\"")
            || source.contains("long = \"timeout\"")
            || source.contains("long(\"timeout\")"));

    let status = if has_flag {
        CheckStatus::Pass
    } else {
        CheckStatus::Warn(
            "Network code detected but no --timeout flag found. \
             Agents need to bound execution time for network operations."
                .to_string(),
        )
    };

    CheckResult {
        id: "p6-timeout".to_string(),
        label: "Timeout flag for network ops".to_string(),
        group: CheckGroup::P6,
        layer: CheckLayer::Source,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_network_code() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let result = check_timeout_flag(source);
        assert!(matches!(result.status, CheckStatus::Skip(_)));
    }

    #[test]
    fn pass_when_timeout_flag_exists() {
        let source = r#"
use reqwest::Client;
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[arg(long = "timeout")]
    timeout: Option<u64>,
}

fn fetch(url: &str, timeout: u64) {
    let client = Client::new();
    // --timeout used here
}
"#;
        let result = check_timeout_flag(source);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_network_but_no_timeout() {
        let source = r#"
use reqwest::Client;

fn fetch(url: &str) {
    let client = Client::new();
    let resp = client.get(url).send();
}
"#;
        let result = check_timeout_flag(source);
        assert!(matches!(result.status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("no --timeout"));
        }
    }

    #[test]
    fn warn_with_hyper() {
        let source = r#"
use hyper::Client;

async fn fetch() {
    let client = Client::new();
}
"#;
        let result = check_timeout_flag(source);
        assert!(matches!(result.status, CheckStatus::Warn(_)));
    }

    #[test]
    fn warn_with_ureq() {
        let source = r#"
fn fetch(url: &str) -> String {
    ureq::get(url).call().into_string()
}
"#;
        let result = check_timeout_flag(source);
        assert!(matches!(result.status, CheckStatus::Warn(_)));
    }

    #[test]
    fn pass_with_timeout_in_arg_attr() {
        let source = r#"
use ureq;
use clap::Parser;

#[derive(Parser)]
struct Cli {
    /// Request timeout in seconds
    #[arg(long = "timeout", default_value = "30")]
    timeout: u64,
}
"#;
        let result = check_timeout_flag(source);
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let check = TimeoutFlagCheck;
        let dir = std::env::temp_dir().join(format!("anc-timeout-rust-{}", std::process::id()));
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
        let check = TimeoutFlagCheck;
        let dir = std::env::temp_dir().join(format!("anc-timeout-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
