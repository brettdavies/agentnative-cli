//! Check: Detect a centralized output/format module in the project source tree.
//!
//! Principle: P2 (Structured Output) — Projects should centralize output
//! formatting in a dedicated module rather than scattering print/format calls.
//!
//! Detection is content-based: any non-main source file that contains output
//! formatting functions (format_, render_, display_, emit_ prefixed fns, or
//! `impl Display`) qualifies. This avoids hardcoding acceptable file names.

use std::path::Path;

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

// Rust source check — lives in source/rust because the content patterns
// (fn format_, impl Display, std::fmt::Write) are Rust-specific.

pub struct OutputModuleCheck;

impl Check for OutputModuleCheck {
    fn id(&self) -> &str {
        "p2-output-module"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P2
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();

        for (path, parsed_file) in parsed.iter() {
            // Skip main.rs and lib.rs — those aren't "dedicated" modules
            if is_entry_point(path) {
                continue;
            }
            if has_output_formatting_code(&parsed_file.source) {
                return Ok(CheckResult {
                    id: self.id().to_string(),
                    label: "Centralized output module exists".into(),
                    group: self.group(),
                    layer: self.layer(),
                    status: CheckStatus::Pass,
                });
            }
        }

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Centralized output module exists".into(),
            group: self.group(),
            layer: self.layer(),
            status: CheckStatus::Warn(
                "No dedicated output module found. Centralize formatting in a module \
                 with format/render/display functions rather than scattering print calls."
                    .into(),
            ),
        })
    }
}

/// Returns true if the path is a typical entry point (main.rs, lib.rs).
fn is_entry_point(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name == "main.rs" || name == "lib.rs")
}

/// Returns true if the source contains output formatting code — functions
/// with format/render/display/emit prefixes, or `impl Display`.
fn has_output_formatting_code(source: &str) -> bool {
    // Look for function definitions with output-related prefixes
    let output_fn_prefixes = ["fn format_", "fn render_", "fn display_", "fn emit_"];
    for prefix in &output_fn_prefixes {
        if source.contains(prefix) {
            return true;
        }
    }

    // Look for Display trait implementations
    if source.contains("impl") && source.contains("Display") && source.contains("fn fmt(") {
        return true;
    }

    // Look for functions returning formatted output (pub fn ... -> String with write!/format!)
    if (source.contains("fn format_") || source.contains("fn render"))
        && source.contains("-> String")
    {
        return true;
    }

    // Look for Write trait usage (structured output formatting)
    if source.contains("use std::fmt::Write")
        || (source.contains("Write as _") && source.contains("writeln!"))
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_format_functions() {
        let source = r#"
pub fn format_text(results: &[CheckResult]) -> String {
    let mut out = String::new();
    out
}
"#;
        assert!(has_output_formatting_code(source));
    }

    #[test]
    fn detects_render_functions() {
        let source = r#"
pub fn render_table(data: &[Row]) -> String {
    todo!()
}
"#;
        assert!(has_output_formatting_code(source));
    }

    #[test]
    fn detects_display_impl() {
        let source = r#"
impl std::fmt::Display for Scorecard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.total)
    }
}
"#;
        assert!(has_output_formatting_code(source));
    }

    #[test]
    fn detects_write_trait_usage() {
        let source = r#"
use std::fmt::Write as _;

pub fn format_output(results: &[Result]) -> String {
    let mut out = String::new();
    writeln!(out, "done").ok();
    out
}
"#;
        assert!(has_output_formatting_code(source));
    }

    #[test]
    fn rejects_plain_main() {
        let source = r#"
fn main() {
    println!("hello");
}
"#;
        assert!(!has_output_formatting_code(source));
    }

    #[test]
    fn entry_point_detection() {
        assert!(is_entry_point(Path::new("src/main.rs")));
        assert!(is_entry_point(Path::new("src/lib.rs")));
        assert!(!is_entry_point(Path::new("src/scorecard.rs")));
        assert!(!is_entry_point(Path::new("src/output.rs")));
    }

    #[test]
    fn applicable_when_language_detected() {
        let dir = std::env::temp_dir().join(format!(
            "anc-outmod-app-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(OutputModuleCheck.applicable(&project));
    }

    #[test]
    fn not_applicable_without_language() {
        let dir = std::env::temp_dir().join(format!(
            "anc-outmod-nolang-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!OutputModuleCheck.applicable(&project));
    }

    #[test]
    fn metadata_is_correct() {
        let check = OutputModuleCheck;
        assert_eq!(check.id(), "p2-output-module");
        assert_eq!(check.group(), CheckGroup::P2);
        assert_eq!(check.layer(), CheckLayer::Source);
    }
}
