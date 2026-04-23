//! Check: Detect `.unwrap()` calls in Rust source.
//!
//! Maps to: check-code-unwrap from the existing 24 bash checks.
//! Principle: P4 (Actionable Errors) — CLIs should handle errors explicitly.

use crate::check::Check;
use crate::project::{Language, Project};
use crate::source::find_pattern_matches;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

const PATTERN: &str = "$RECV.unwrap()";

/// Check trait implementation for unwrap detection.
pub struct UnwrapCheck;

impl Check for UnwrapCheck {
    fn id(&self) -> &str {
        "code-unwrap"
    }

    fn label(&self) -> &'static str {
        "No .unwrap() in source"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::CodeQuality
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut all_evidence = Vec::new();

        for (path, parsed_file) in parsed.iter() {
            let file_str = path.display().to_string();
            if let CheckStatus::Fail(evidence) = check_unwrap(&parsed_file.source, &file_str) {
                all_evidence.push(evidence);
            }
        }

        let status = if all_evidence.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail(all_evidence.join("\n"))
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// Check a single source string for `.unwrap()` calls.
///
/// Kept public(crate) for unit testing with inline source strings.
pub(crate) fn check_unwrap(source: &str, file: &str) -> CheckStatus {
    let mut matches = find_pattern_matches(source, PATTERN);
    for m in &mut matches {
        m.file = file.to_string();
    }

    if matches.is_empty() {
        CheckStatus::Pass
    } else {
        let evidence = matches
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        CheckStatus::Fail(evidence)
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
        let status = check_unwrap(source, "src/main.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn fail_when_unwrap_present() {
        let source = r#"
fn main() {
    let config = load_config().unwrap();
}
"#;
        let status = check_unwrap(source, "src/main.rs");
        assert!(matches!(status, CheckStatus::Fail(_)));
        if let CheckStatus::Fail(evidence) = &status {
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
        let status = check_unwrap(source, "src/lib.rs");
        if let CheckStatus::Fail(evidence) = &status {
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
        let status = check_unwrap(source, "src/main.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn ignores_unwrap_in_strings() {
        let source = r#"
fn main() -> anyhow::Result<()> {
    eprintln!("Don't use .unwrap() in production");
    Ok(())
}
"#;
        let status = check_unwrap(source, "src/main.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let check = UnwrapCheck;
        let dir = std::env::temp_dir().join(format!("anc-unwrap-test-{}", std::process::id()));
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
    fn not_applicable_for_python() {
        let check = UnwrapCheck;
        let dir = std::env::temp_dir().join(format!("anc-unwrap-py-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test pyproject.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }

    #[test]
    fn not_applicable_for_none() {
        let check = UnwrapCheck;
        let dir = std::env::temp_dir().join(format!("anc-unwrap-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
