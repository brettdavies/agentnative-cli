//! Check: Detect `println!()` and `print!()` macro calls in non-output files.
//!
//! Principle: P7 (Bounded Responses) — CLIs should channel output through a
//! dedicated output module, not scatter `println!` calls across the codebase.
//! `eprintln!` is exempt (diagnostics go to stderr).
//! Files with "output" or "display" in their path are exempt (output modules).

use crate::check::Check;
use crate::project::{Language, Project};
use crate::source::find_pattern_matches;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

const PRINTLN_PATTERN: &str = "println!($$$ARGS)";
const PRINT_PATTERN: &str = "print!($$$ARGS)";

/// Check trait implementation for naked println detection.
pub struct NakedPrintlnCheck;

impl Check for NakedPrintlnCheck {
    fn id(&self) -> &str {
        "p7-naked-println"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P7
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

            // Exempt files with "output" or "display" in their path
            let lower = file_str.to_lowercase();
            if lower.contains("output") || lower.contains("display") {
                continue;
            }

            if let CheckStatus::Warn(evidence) = check_naked_println(&parsed_file.source, &file_str)
            {
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
            label: "No naked println!/print! outside output modules".to_string(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// Check a single source string for `println!` and `print!` calls.
///
/// Kept public(crate) for unit testing with inline source strings.
pub(crate) fn check_naked_println(source: &str, file: &str) -> CheckStatus {
    let mut println_matches = find_pattern_matches(source, PRINTLN_PATTERN);
    let mut print_matches = find_pattern_matches(source, PRINT_PATTERN);

    for m in &mut println_matches {
        m.file = file.to_string();
    }
    for m in &mut print_matches {
        m.file = file.to_string();
    }

    let mut all_matches = println_matches;
    all_matches.append(&mut print_matches);

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
    fn pass_when_no_println() {
        let source = r#"
fn main() -> anyhow::Result<()> {
    eprintln!("debug info");
    Ok(())
}
"#;
        let status = check_naked_println(source, "src/main.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_println_present() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let status = check_naked_println(source, "src/main.rs");
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
            assert!(evidence.contains("println!"));
            assert!(evidence.contains("src/main.rs"));
        }
    }

    #[test]
    fn warn_when_print_present() {
        let source = r#"
fn render() {
    print!("loading...");
}
"#;
        let status = check_naked_println(source, "src/render.rs");
        assert!(matches!(status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &status {
            assert!(evidence.contains("print!"));
            assert!(evidence.contains("src/render.rs"));
        }
    }

    #[test]
    fn eprintln_is_exempt() {
        let source = r#"
fn main() {
    eprintln!("warning: something happened");
    eprintln!("error: {}", msg);
}
"#;
        let status = check_naked_println(source, "src/main.rs");
        assert_eq!(status, CheckStatus::Pass);
    }

    #[test]
    fn counts_multiple_violations() {
        let source = r#"
fn main() {
    println!("one");
    println!("two");
    print!("three");
}
"#;
        let status = check_naked_println(source, "src/lib.rs");
        if let CheckStatus::Warn(evidence) = &status {
            assert_eq!(evidence.lines().count(), 3);
        } else {
            panic!("Expected Warn");
        }
    }

    #[test]
    fn applicable_for_rust() {
        let check = NakedPrintlnCheck;
        let dir =
            std::env::temp_dir().join(format!("anc-nakedprintln-rust-{}", std::process::id()));
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
        let check = NakedPrintlnCheck;
        let dir =
            std::env::temp_dir().join(format!("anc-nakedprintln-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!check.applicable(&project));
    }
}
