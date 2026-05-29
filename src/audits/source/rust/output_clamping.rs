//! Audit: Detect list/iteration patterns missing output clamping.
//!
//! Principle: P7 (Bounded Responses) — CLIs that list items should clamp output
//! with `.take()`, `.clamp()`, `--limit`, or `--max` to avoid overwhelming agents
//! with unbounded output.
//!
//! This is inherently heuristic — prefers Skip/Warn over Fail.

use crate::audit::Audit;
use crate::project::{Language, Project};
use crate::source::{find_pattern_matches, has_pattern};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Patterns that suggest list/collection output.
const LIST_PATTERNS: &[&str] = &[
    "$RECV.collect::<Vec<$$$TYPES>>()",
    "for $ITEM in $ITER { $$$BODY }",
    "for $ITEM in $RECV.iter() { $$$BODY }",
];

/// Patterns that indicate output clamping is present.
const CLAMP_PATTERNS: &[&str] = &["$RECV.take($LIMIT)", "$RECV.clamp($$$ARGS)"];

/// String indicators of clamping in arg definitions or source.
const CLAMP_STRINGS: &[&str] = &["--limit", "--max", "limit", "max_results", "page_size"];

/// Audit trait implementation for output clamping detection.
pub struct OutputClampingAudit;

impl Audit for OutputClampingAudit {
    fn id(&self) -> &str {
        "p7-output-clamping"
    }

    fn label(&self) -> &'static str {
        "List output is clamped"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P7
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-must-list-clamping"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let parsed = project.parsed_files();
        let mut has_list_patterns = false;
        let mut has_clamping = false;
        let mut list_evidence = Vec::new();

        for (path, parsed_file) in parsed.iter() {
            let file_str = path.display().to_string();
            match &audit_output_clamping(&parsed_file.source, &file_str) {
                AuditStatus::Warn(evidence) => {
                    has_list_patterns = true;
                    list_evidence.push(evidence.clone());
                }
                AuditStatus::Pass => {
                    has_list_patterns = true;
                    has_clamping = true;
                }
                AuditStatus::Skip(_) => {
                    // No list patterns in this file
                }
                _ => {}
            }
        }

        let status = if !has_list_patterns {
            AuditStatus::Skip("No list output patterns detected".to_string())
        } else if has_clamping {
            AuditStatus::Pass
        } else {
            AuditStatus::Warn(list_evidence.join("\n"))
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// Audit a single source string for list patterns and clamping.
///
/// Kept public(crate) for unit testing with inline source strings.
pub(crate) fn audit_output_clamping(source: &str, file: &str) -> AuditStatus {
    // Step 1: Look for list/iteration patterns
    let mut list_locations = Vec::new();

    for pattern_str in LIST_PATTERNS {
        let mut matches = find_pattern_matches(source, pattern_str);
        for m in &mut matches {
            m.file = file.to_string();
        }
        list_locations.extend(matches);
    }

    if list_locations.is_empty() {
        return AuditStatus::Skip("No list output patterns detected".to_string());
    }

    // Step 2: Audit if clamping exists anywhere in the same source
    let has_clamping = CLAMP_PATTERNS.iter().any(|pat| has_pattern(source, pat))
        || CLAMP_STRINGS.iter().any(|s| source.contains(s));

    if has_clamping {
        AuditStatus::Pass
    } else {
        let evidence = list_locations
            .iter()
            .take(5) // Limit evidence to avoid noise
            .map(|m| {
                let text_preview = truncate_text(&m.text, 80);
                format!(
                    "{}:{}:{} — list pattern without clamping: {}",
                    m.file, m.line, m.column, text_preview
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        AuditStatus::Warn(evidence)
    }
}

/// Truncate text to a maximum length, appending "..." if truncated.
fn truncate_text(text: &str, max_len: usize) -> String {
    // Collapse to single line first
    let single_line: String = text
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    if single_line.len() <= max_len {
        single_line
    } else {
        format!("{}...", &single_line[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_list_patterns() {
        let source = r#"
fn main() {
    let x = compute_value();
    eprintln!("result: {x}");
}
"#;
        let status = audit_output_clamping(source, "src/main.rs");
        assert!(matches!(status, AuditStatus::Skip(_)));
    }

    #[test]
    fn warn_when_collect_without_clamping() {
        let source = r#"
fn list_items(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| i.name.clone()).collect::<Vec<String>>()
}
"#;
        let status = audit_output_clamping(source, "src/list.rs");
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
            assert!(evidence.contains("list pattern without clamping"));
        }
    }

    #[test]
    fn pass_when_collect_with_take() {
        let source = r#"
fn list_items(items: &[Item]) -> Vec<String> {
    items.iter().take(100).map(|i| i.name.clone()).collect::<Vec<String>>()
}
"#;
        let status = audit_output_clamping(source, "src/list.rs");
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn pass_when_limit_flag_present() {
        let source = r#"
fn list_items(items: &[Item], limit: usize) -> Vec<String> {
    items.iter().map(|i| i.name.clone()).collect::<Vec<String>>()
}

fn parse_args() {
    let limit = matches.get_one::<usize>("--limit");
}
"#;
        let status = audit_output_clamping(source, "src/list.rs");
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_for_loop_without_clamping() {
        let source = r#"
fn print_all(items: Vec<String>) {
    for item in items.iter() {
        eprintln!("{item}");
    }
}
"#;
        let status = audit_output_clamping(source, "src/printer.rs");
        assert!(matches!(status, AuditStatus::Warn(_)));
    }

    #[test]
    fn pass_when_max_results_present() {
        let source = r#"
fn list_results(items: &[Item]) -> Vec<String> {
    let max_results = 50;
    items.iter().map(|i| i.name.clone()).collect::<Vec<String>>()
}
"#;
        let status = audit_output_clamping(source, "src/list.rs");
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn applicable_for_rust() {
        let audit = OutputClampingAudit;
        let dir = std::env::temp_dir().join(format!("anc-outclamping-rust-{}", std::process::id()));
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
        let audit = OutputClampingAudit;
        let dir = std::env::temp_dir().join(format!("anc-outclamping-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }
}
