//! Check: Detect `.parse().unwrap()` patterns.
//!
//! Principle: P4 (Actionable Errors) — Parsing user input should use
//! proper error handling, not panicking unwraps. Prefer `?` or `match`.
//!
//! Only flags `.parse().unwrap()` — `.parse().expect()` is not flagged because
//! the developer has already considered the error case and provided a message.

use crate::check::Check;
use crate::project::{Language, Project};
use crate::source::find_pattern_matches;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

const PATTERNS: &[&str] = &["$RECV.parse().unwrap()"];

/// Check trait implementation for parse-unwrap detection.
pub struct TryParseCheck;

impl Check for TryParseCheck {
    fn id(&self) -> &str {
        "p4-try-parse"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P4
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-must-try-parse"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut all_evidence = Vec::new();

        for (path, parsed_file) in parsed.iter() {
            let file_str = path.display().to_string();
            if let CheckStatus::Warn(evidence) = check_try_parse(&parsed_file.source, &file_str) {
                all_evidence.push(evidence);
            }
        }

        let status = if all_evidence.is_empty() {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn(all_evidence.join("\n"))
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "No .parse().unwrap()".to_string(),
            group: self.group(),
            layer: self.layer(),
            status,
        })
    }
}

/// Check a single source string for `.parse().unwrap()` and `.parse().expect()`.
///
/// Kept public(crate) for unit testing with inline source strings.
pub(crate) fn check_try_parse(source: &str, file: &str) -> CheckStatus {
    let mut all_matches = Vec::new();

    for pattern_str in PATTERNS {
        let mut matches = find_pattern_matches(source, pattern_str);
        for m in &mut matches {
            m.file = file.to_string();
        }
        all_matches.extend(matches);
    }

    if all_matches.is_empty() {
        CheckStatus::Pass
    } else {
        let evidence = all_matches
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        CheckStatus::Warn(evidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_no_parse_unwrap() {
        let source = r#"
fn parse_port(s: &str) -> Result<u16, std::num::ParseIntError> {
    s.parse()
}
"#;
        let status = check_try_parse(source, "src/config.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn pass_with_question_mark() {
        let source = r#"
fn parse_port(s: &str) -> anyhow::Result<u16> {
    let port: u16 = s.parse()?;
    Ok(port)
}
"#;
        let status = check_try_parse(source, "src/config.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn warn_with_parse_unwrap() {
        let source = r#"
fn main() {
    let port: u16 = args[1].parse().unwrap();
}
"#;
        let status = check_try_parse(source, "src/main.rs");
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
            assert!(evidence.contains("parse().unwrap()"));
            assert!(evidence.contains("src/main.rs"));
        }
    }

    #[test]
    fn pass_with_parse_expect() {
        // .parse().expect() is acceptable — the developer already considered the error
        let source = r#"
fn main() {
    let port: u16 = args[1].parse().expect("bad port");
}
"#;
        let status = check_try_parse(source, "src/main.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn warn_counts_multiple() {
        let source = r#"
fn main() {
    let port: u16 = args[1].parse().unwrap();
    let count: usize = args[2].parse().unwrap();
    let timeout: u64 = args[3].parse().expect("bad timeout");
}
"#;
        let status = check_try_parse(source, "src/main.rs");
        if let CheckStatus::Warn(evidence) = &status {
            // Only .parse().unwrap() lines are flagged, not .expect()
            assert_eq!(evidence.lines().count(), 2);
        } else {
            panic!("Expected Warn");
        }
    }

    #[test]
    fn pass_with_match_on_parse() {
        let source = r#"
fn parse_port(s: &str) -> u16 {
    match s.parse::<u16>() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Invalid port: {e}");
            std::process::exit(1);
        }
    }
}
"#;
        let status = check_try_parse(source, "src/config.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let check = TryParseCheck;
        let dir = std::env::temp_dir().join(format!("anc-tryparse-rust-{}", std::process::id()));
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
        let check = TryParseCheck;
        let dir = std::env::temp_dir().join(format!("anc-tryparse-py-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("pyproject.toml"),
            "[project]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test pyproject.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
