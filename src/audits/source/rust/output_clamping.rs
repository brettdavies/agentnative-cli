//! Audit: Detect list/iteration patterns missing output clamping.
//!
//! Principle: P7 (Bounded Responses) — CLIs that list items should clamp output
//! with `.take()`, `.clamp()`, `--limit`, or `--max` to avoid overwhelming agents
//! with unbounded output.
//!
//! This is inherently heuristic — prefers Skip/Warn over Fail.

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::{evidence_preview, has_any, locations_each};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Patterns that suggest list/collection output.
static LIST_PATTERNS: LazyLock<[Pattern; 3]> = LazyLock::new(|| {
    [
        Pattern::new("$RECV.collect::<Vec<$$$TYPES>>()", Rust),
        Pattern::new("for $ITEM in $ITER { $$$BODY }", Rust),
        Pattern::new("for $ITEM in $RECV.iter() { $$$BODY }", Rust),
    ]
});

/// Patterns that indicate output clamping is present.
static CLAMP_PATTERNS: LazyLock<[Pattern; 2]> = LazyLock::new(|| {
    [
        Pattern::new("$RECV.take($LIMIT)", Rust),
        Pattern::new("$RECV.clamp($$$ARGS)", Rust),
    ]
});

/// String indicators of clamping in arg definitions or source.
const CLAMP_STRINGS: &[&str] = &["--limit", "--max", "limit", "max_results", "page_size"];

/// Audit trait implementation for output clamping detection.
#[derive(Debug)]
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
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let mut has_list_patterns = false;
        let mut has_clamping = false;
        let mut list_evidence = Vec::new();

        for (_, status) in pass::findings(project, Language::Rust, Rust, self) {
            match status {
                AuditStatus::Warn(evidence) => {
                    has_list_patterns = true;
                    list_evidence.push(evidence);
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
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

impl SourceScan<Rust> for OutputClampingAudit {
    type Finding = AuditStatus;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> AuditStatus {
        audit_output_clamping(file)
    }
}

/// Audit one file for list patterns and clamping.
pub(crate) fn audit_output_clamping(file: &SourceFile<'_, Rust>) -> AuditStatus {
    let root = file.root();
    let list_locations: Vec<_> = locations_each(&root, &*LIST_PATTERNS, &file.label())
        .into_iter()
        .flatten()
        .collect();

    if list_locations.is_empty() {
        return AuditStatus::Skip("No list output patterns detected".to_string());
    }

    let source = file.source();
    let has_clamping =
        CLAMP_STRINGS.iter().any(|s| source.contains(s)) || has_any(&root, &*CLAMP_PATTERNS);

    if has_clamping {
        AuditStatus::Pass
    } else {
        let evidence = list_locations
            .iter()
            .take(5) // Limit evidence to avoid noise
            .map(|m| {
                let text_preview = evidence_preview(&m.text, 80);
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
        let status = audit_output_clamping(&pass::rust(source, "src/main.rs"));
        assert!(matches!(status, AuditStatus::Skip(_)));
    }

    #[test]
    fn warn_when_collect_without_clamping() {
        let source = r#"
fn list_items(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| i.name.clone()).collect::<Vec<String>>()
}
"#;
        let status = audit_output_clamping(&pass::rust(source, "src/list.rs"));
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
        let status = audit_output_clamping(&pass::rust(source, "src/list.rs"));
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
        let status = audit_output_clamping(&pass::rust(source, "src/list.rs"));
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
        let status = audit_output_clamping(&pass::rust(source, "src/printer.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
    }

    #[test]
    fn warn_preview_of_long_ascii_text_is_eighty_chars_plus_ellipsis() {
        let source = r#"
fn list_items(items: &[Item]) -> Vec<String> {
    items.iter().map(|item| format!("{} was renamed to {}", item.old_name, item.new_name)).collect::<Vec<String>>()
}
"#;
        let status = audit_output_clamping(&pass::rust(source, "src/list.rs"));
        let AuditStatus::Warn(evidence) = status else {
            panic!("expected Warn, got {status:?}");
        };
        let preview = evidence
            .split("list pattern without clamping: ")
            .nth(1)
            .expect("preview follows the label");
        assert_eq!(preview.len(), 83, "80 chars plus one ellipsis: {preview}");
        assert!(preview.starts_with("items.iter().map(|item| format!("));
        assert_eq!(preview.matches("...").count(), 1);
    }

    #[test]
    fn warn_preview_of_short_ascii_text_has_no_ellipsis() {
        let source = r#"
fn list_items(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| i.name.clone()).collect::<Vec<String>>()
}
"#;
        let status = audit_output_clamping(&pass::rust(source, "src/list.rs"));
        let AuditStatus::Warn(evidence) = status else {
            panic!("expected Warn, got {status:?}");
        };
        assert!(
            evidence.ends_with("items.iter().map(|i| i.name.clone()).collect::<Vec<String>>()"),
            "short matched text is shown whole: {evidence}"
        );
        assert!(!evidence.contains("..."));
    }

    #[test]
    fn warn_when_braille_glyphs_straddle_the_preview_budget() {
        let source = r#"
fn spin(frames_seen: &mut Vec<&str>) {
    for frame in ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"].iter() {
        frames_seen.push(frame);
    }
}
"#;
        let status = audit_output_clamping(&pass::rust(source, "src/spinner.rs"));
        let AuditStatus::Warn(evidence) = status else {
            panic!("expected Warn, got {status:?}");
        };
        assert!(
            evidence.contains("..."),
            "matched text exceeds the budget: {evidence}"
        );
    }

    #[test]
    fn warn_when_cjk_glyphs_straddle_the_preview_budget() {
        let source = r#"
fn describe(names: &[String]) -> Vec<String> {
    names.iter().map(|name| format!("名前は{name}です。よろしくお願いします")).collect::<Vec<String>>()
}
"#;
        let status = audit_output_clamping(&pass::rust(source, "src/describe.rs"));
        let AuditStatus::Warn(evidence) = status else {
            panic!("expected Warn, got {status:?}");
        };
        assert!(
            evidence.contains("..."),
            "matched text exceeds the budget: {evidence}"
        );
    }

    #[test]
    fn warn_when_emoji_straddle_the_preview_budget() {
        let source = r#"
fn celebrate(winners: &[String]) {
    for winner in winners.iter() {
        eprintln!("congratulations {winner} 🏆🏆🏆🏆🏆🏆🏆🏆🏆🏆🏆🏆");
    }
}
"#;
        let status = audit_output_clamping(&pass::rust(source, "src/celebrate.rs"));
        let AuditStatus::Warn(evidence) = status else {
            panic!("expected Warn, got {status:?}");
        };
        assert!(
            evidence.contains("..."),
            "matched text exceeds the budget: {evidence}"
        );
    }

    #[test]
    fn pass_when_max_results_present() {
        let source = r#"
fn list_results(items: &[Item]) -> Vec<String> {
    let max_results = 50;
    items.iter().map(|i| i.name.clone()).collect::<Vec<String>>()
}
"#;
        let status = audit_output_clamping(&pass::rust(source, "src/list.rs"));
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
