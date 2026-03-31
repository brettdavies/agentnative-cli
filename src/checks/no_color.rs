//! PoC Check #3 (KEYWORD): Detect NO_COLOR environment variable handling.
//!
//! Maps to: check-p6-no-color from the existing 24 bash checks.
//! Principle: P6 (Composable Structure) — CLIs must respect NO_COLOR.
//! See https://no-color.org/
//!
//! This check verifies the source references `NO_COLOR` as an env var.
//! It spans source + behavioral layers; this is the source half.

use ast_grep_core::tree_sitter::LanguageExt;
use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

pub fn check_no_color(source: &str, file: &str) -> CheckResult {
    // Strategy: look for any reference to the "NO_COLOR" string literal
    // in env-reading contexts. Multiple patterns to catch common idioms:
    //
    //   std::env::var("NO_COLOR")
    //   env::var("NO_COLOR")
    //   env = "NO_COLOR" (clap #[arg(env = "NO_COLOR")])
    //   "NO_COLOR" as a general string literal (fallback)

    let found_env_var = has_pattern(source, r#"std::env::var("NO_COLOR")"#)
        || has_pattern(source, r#"env::var("NO_COLOR")"#)
        || has_pattern(source, r#"std::env::var_os("NO_COLOR")"#)
        || has_pattern(source, r#"env::var_os("NO_COLOR")"#);

    let found_clap_env = source_contains_no_color_clap_attr(source);

    let found_any = found_env_var || found_clap_env || has_string_literal(source, "NO_COLOR");

    let status = if found_any {
        CheckStatus::Pass
    } else {
        CheckStatus::Fail(format!(
            "{file}: No reference to NO_COLOR found. CLIs must respect the NO_COLOR convention. \
             See https://no-color.org/"
        ))
    };

    CheckResult {
        id: "p6-no-color".to_string(),
        label: "Respects NO_COLOR".to_string(),
        group: CheckGroup::P6,
        layer: CheckLayer::Source,
        status,
    }
}

fn has_pattern(source: &str, pattern_str: &str) -> bool {
    let pattern = match Pattern::try_new(pattern_str, Rust) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = Rust.ast_grep(source);
    root.root().find(&pattern).is_some()
}

/// Check for clap attribute: `#[arg(env = "NO_COLOR")]`
fn source_contains_no_color_clap_attr(source: &str) -> bool {
    // Use ast-grep to find #[arg(...)] containing NO_COLOR
    let pattern = match Pattern::try_new(r#"#[arg($$$ARGS)]"#, Rust) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = Rust.ast_grep(source);
    for m in root.root().find_all(&pattern) {
        if m.text().contains("NO_COLOR") {
            return true;
        }
    }
    false
}

/// Fallback: scan for "NO_COLOR" as a string literal anywhere in the AST.
/// This catches patterns we didn't explicitly enumerate.
fn has_string_literal(source: &str, needle: &str) -> bool {
    let pattern = match Pattern::try_new(&format!(r#""{needle}""#), Rust) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let root = Rust.ast_grep(source);
    root.root().find(&pattern).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_with_std_env_var() {
        let source = r#"
fn setup_color() {
    if std::env::var("NO_COLOR").is_ok() {
        disable_color();
    }
}
"#;
        let result = check_no_color(source, "src/output.rs");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn pass_with_env_var_os() {
        let source = r#"
use std::env;

fn setup_color() {
    if env::var_os("NO_COLOR").is_some() {
        disable_color();
    }
}
"#;
        let result = check_no_color(source, "src/output.rs");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn pass_with_clap_env_attr() {
        let source = r#"
#[derive(Parser)]
struct Cli {
    #[arg(long, env = "NO_COLOR")]
    no_color: bool,
}
"#;
        let result = check_no_color(source, "src/cli.rs");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn fail_when_no_color_absent() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let result = check_no_color(source, "src/main.rs");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
        if let CheckStatus::Fail(evidence) = &result.status {
            assert!(evidence.contains("NO_COLOR"));
            assert!(evidence.contains("no-color.org"));
        }
    }

    #[test]
    fn pass_with_string_literal_fallback() {
        let source = r#"
const COLOR_ENV: &str = "NO_COLOR";

fn check_color() {
    std::env::var(COLOR_ENV).ok();
}
"#;
        let result = check_no_color(source, "src/color.rs");
        assert_eq!(result.status, CheckStatus::Pass);
    }
}
