//! Audit: Detect `println!()` and `print!()` macro calls in non-output files.
//!
//! Principle: P7 (Bounded Responses) — CLIs should channel output through a
//! dedicated output module, not scatter `println!` calls across the codebase.
//! `eprintln!` is exempt (diagnostics go to stderr).
//! Files with "output" or "display" in their path are exempt (output modules).
//! `build.rs` is also exempt — Cargo build scripts emit metadata via
//! `println!("cargo:...")` directives by required-by-protocol convention; no
//! alternative API exists, so flagging them produces noise without recourse.

use std::sync::LazyLock;

use ast_grep_core::Pattern;
use ast_grep_language::Rust;

use crate::audit::Audit;
use crate::audits::source::pass::{self, Scanner, SourceFile, SourceScan};
use crate::project::{Language, Project};
use crate::source::locations_each;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

static PRINT_MACROS: LazyLock<[Pattern; 2]> = LazyLock::new(|| {
    [
        Pattern::new("println!($$$ARGS)", Rust),
        Pattern::new("print!($$$ARGS)", Rust),
    ]
});

/// Audit trait implementation for naked println detection.
#[derive(Debug)]
pub struct NakedPrintlnAudit;

impl Audit for NakedPrintlnAudit {
    fn id(&self) -> &str {
        "p7-naked-println"
    }

    fn label(&self) -> &'static str {
        "No naked println!/print! outside output modules"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P7
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn applicable(&self, project: &Project) -> bool {
        project.has_language(Language::Rust)
    }

    fn scanner(&self) -> Option<Scanner<'_>> {
        Some(Scanner::Rust(self))
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let mut all_evidence = Vec::new();

        for (_, status) in pass::findings(project, Language::Rust, Rust, self) {
            if let AuditStatus::Warn(evidence) = status {
                all_evidence.push(evidence);
            }
        }

        let status = if all_evidence.is_empty() {
            AuditStatus::Pass
        } else {
            AuditStatus::Warn(all_evidence.join("\n"))
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

impl SourceScan<Rust> for NakedPrintlnAudit {
    type Finding = AuditStatus;

    fn scan(&self, file: &SourceFile<'_, Rust>) -> AuditStatus {
        let file_str = file.label();

        // Exempt files with "output" or "display" in their path, and
        // Cargo build scripts (build.rs at any crate root).
        let lower = file_str.to_lowercase();
        if lower.contains("output") || lower.contains("display") || is_cargo_build_script(&file_str)
        {
            return AuditStatus::Pass;
        }

        audit_naked_println(file)
    }
}

/// True when `path` names a Cargo build script (`build.rs` at any crate
/// root). The convention is fixed by Cargo — build scripts are always at
/// `<crate-root>/build.rs`, never nested under `src/`, `tests/`, `examples/`,
/// or `benches/`. Paths under those directories that happen to be named
/// `build.rs` are misnamed source files, not build scripts, and stay flagged.
fn is_cargo_build_script(path: &str) -> bool {
    let lower = path.to_lowercase();
    // Normalize Windows separators so segment audits are uniform.
    let normalized = lower.replace('\\', "/");
    let segments: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    let Some(last) = segments.last() else {
        return false;
    };
    if *last != "build.rs" {
        return false;
    }
    // Cargo build scripts live at crate root. Reject paths where any
    // ancestor segment is a known non-root subdirectory.
    let parents = &segments[..segments.len() - 1];
    !parents
        .iter()
        .any(|s| matches!(*s, "src" | "tests" | "examples" | "benches"))
}

/// Audit one file for `println!` and `print!` calls.
pub(crate) fn audit_naked_println(file: &SourceFile<'_, Rust>) -> AuditStatus {
    let all_matches: Vec<_> = locations_each(&file.root(), &*PRINT_MACROS, &file.label())
        .into_iter()
        .flatten()
        .collect();

    if all_matches.is_empty() {
        AuditStatus::Pass
    } else {
        let evidence = all_matches
            .iter()
            .map(|m| format!("{}:{}:{} — {}", m.file, m.line, m.column, m.text))
            .collect::<Vec<_>>()
            .join("\n");
        AuditStatus::Warn(evidence)
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
        let status = audit_naked_println(&pass::rust(source, "src/main.rs"));
        assert_eq!(status, AuditStatus::Pass);
    }

    #[test]
    fn warn_when_println_present() {
        let source = r#"
fn main() {
    println!("Hello, world!");
}
"#;
        let status = audit_naked_println(&pass::rust(source, "src/main.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
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
        let status = audit_naked_println(&pass::rust(source, "src/render.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
        if let AuditStatus::Warn(evidence) = &status {
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
        let status = audit_naked_println(&pass::rust(source, "src/main.rs"));
        assert_eq!(status, AuditStatus::Pass);
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
        let status = audit_naked_println(&pass::rust(source, "src/lib.rs"));
        if let AuditStatus::Warn(evidence) = &status {
            assert_eq!(evidence.lines().count(), 3);
        } else {
            panic!("Expected Warn");
        }
    }

    #[test]
    fn build_script_path_recognized() {
        assert!(is_cargo_build_script("build.rs"));
        assert!(is_cargo_build_script("./build.rs"));
        assert!(is_cargo_build_script("/abs/path/build.rs"));
        assert!(is_cargo_build_script("BUILD.RS")); // case-insensitive
        assert!(is_cargo_build_script("subcrate\\build.rs")); // Windows path

        assert!(!is_cargo_build_script("src/build.rs"));
        // build.rs nested under src/ is not the cargo build script — it's a
        // misnamed source file. Cargo build scripts only live at crate root.
        // Exception: workspace member build scripts at <member>/build.rs
        // are correctly matched by the `/build.rs` suffix logic.
        assert!(!is_cargo_build_script("src/skill_install.rs"));
        assert!(!is_cargo_build_script("build.rs.bak"));
    }

    #[test]
    fn audit_skips_build_script_println() {
        // Direct audit of the helper — the macro pattern matches, but the
        // path-level filter in the scan skips build.rs callers.
        let source = r#"
fn main() {
    println!("cargo:rerun-if-changed=src/principles/spec/");
}
"#;
        // The unit-level helper still warns (it doesn't know about file
        // path); the scan is what skips. Confirm both behaviors.
        let status = audit_naked_println(&pass::rust(source, "build.rs"));
        assert!(matches!(status, AuditStatus::Warn(_)));
        assert!(is_cargo_build_script("build.rs"));
    }

    #[test]
    fn applicable_for_rust() {
        let audit = NakedPrintlnAudit;
        let dir =
            std::env::temp_dir().join(format!("anc-nakedprintln-rust-{}", std::process::id()));
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
        let audit = NakedPrintlnAudit;
        let dir =
            std::env::temp_dir().join(format!("anc-nakedprintln-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!audit.applicable(&project));
    }
}
