//! PoC Check #1 (EASY): Detect `.unwrap()` calls in Rust source.
//!
//! Maps to: check-code-unwrap from the existing 24 bash checks.
//! Principle: P4 (Actionable Errors) — CLIs should handle errors explicitly.

use crate::source::find_pattern_matches;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

const PATTERN: &str = "$RECV.unwrap()";

pub fn check_unwrap(source: &str, file: &str) -> CheckResult {
    let mut matches = find_pattern_matches(source, PATTERN);
    for m in &mut matches {
        m.file = file.to_string();
    }

    let status = if matches.is_empty() {
        CheckStatus::Pass
    } else {
        let evidence = matches
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        CheckStatus::Fail(evidence)
    };

    CheckResult {
        id: "code-unwrap".to_string(),
        label: "No .unwrap() in source".to_string(),
        group: CheckGroup::CodeQuality,
        layer: CheckLayer::Source,
        status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_no_unwrap() {
        let source = r#"
fn main() -> anyhow::Result<()> {
    let config = load_config()?;
    let data = fetch_data()?;
    Ok(())
}
"#;
        let result = check_unwrap(source, "src/main.rs");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn fail_when_unwrap_present() {
        let source = r#"
fn main() {
    let config = load_config().unwrap();
}
"#;
        let result = check_unwrap(source, "src/main.rs");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
        if let CheckStatus::Fail(evidence) = &result.status {
            assert!(evidence.contains("unwrap"));
            assert!(evidence.contains("src/main.rs"));
        }
    }

    #[test]
    fn fail_counts_multiple_unwraps() {
        let source = r#"
fn main() {
    let a = foo().unwrap();
    let b = bar().unwrap();
    let c = baz().unwrap();
}
"#;
        let result = check_unwrap(source, "src/lib.rs");
        if let CheckStatus::Fail(evidence) = &result.status {
            assert_eq!(evidence.lines().count(), 3);
        } else {
            panic!("Expected Fail");
        }
    }

    #[test]
    fn ignores_unwrap_in_comments() {
        let source = r#"
fn main() -> anyhow::Result<()> {
    // Previously: config.unwrap()
    let config = load_config()?;
    Ok(())
}
"#;
        let result = check_unwrap(source, "src/main.rs");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn ignores_unwrap_in_strings() {
        let source = r#"
fn main() -> anyhow::Result<()> {
    eprintln!("Don't use .unwrap() in production");
    Ok(())
}
"#;
        let result = check_unwrap(source, "src/main.rs");
        assert_eq!(result.status, CheckStatus::Pass);
    }
}
