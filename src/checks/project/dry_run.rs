//! Check: Detect whether CLIs with write/mutate commands offer a --dry-run flag.
//!
//! Principle: P5 (Safe Retries) — CLIs that perform write operations should support
//! --dry-run so agents can preview changes before committing them.
//!
//! Heuristic approach:
//!   1. Scan source files for clap arg definitions containing write/mutate keywords
//!   2. If none found → Skip (no write operations detected)
//!   3. If found, search for --dry-run flag definition
//!   4. Pass if --dry-run exists, Warn if missing

use std::fs;

use crate::check::Check;
use crate::project::Project;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Keywords in clap arg definitions that indicate write/mutate operations.
const WRITE_KEYWORDS: &[&str] = &[
    "--write",
    "--delete",
    "--create",
    "--update",
    "--remove",
    "--deploy",
    "--install",
    "--push",
    "\"write\"",
    "\"delete\"",
    "\"create\"",
    "\"update\"",
    "\"remove\"",
    "\"deploy\"",
    "\"install\"",
    "\"push\"",
];

/// Patterns indicating a --dry-run flag exists.
const DRY_RUN_PATTERNS: &[&str] = &["dry-run", "dry_run", "dryrun"];

pub struct DryRunCheck;

impl Check for DryRunCheck {
    fn id(&self) -> &str {
        "p5-dry-run"
    }

    fn label(&self) -> &'static str {
        "Dry-run flag for write operations"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P5
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Project
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p5-must-dry-run"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.path.is_dir() && project.language.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();

        let mut has_write_commands = false;
        let mut has_dry_run = false;

        for (_path, parsed_file) in parsed.iter() {
            let source = &parsed_file.source;
            if !has_write_commands {
                has_write_commands = WRITE_KEYWORDS.iter().any(|kw| source.contains(kw));
            }
            if !has_dry_run {
                has_dry_run = DRY_RUN_PATTERNS.iter().any(|pat| source.contains(pat));
            }
            if has_write_commands && has_dry_run {
                break;
            }
        }

        // Also check the manifest for subcommand names that imply writes
        if !has_write_commands
            && let Some(manifest) = &project.manifest_path
            && let Ok(content) = fs::read_to_string(manifest)
        {
            has_write_commands = WRITE_KEYWORDS.iter().any(|kw| content.contains(kw));
        }

        let status = if !has_write_commands {
            CheckStatus::Skip("No write/mutate commands detected".into())
        } else if has_dry_run {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn("Write/mutate commands detected but no --dry-run flag found".into())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use crate::project::{Language, ParsedFile};

    fn temp_dir(suffix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anc-dryrun-{suffix}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after UNIX epoch")
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    fn make_project(dir: &std::path::Path, files: &[(&str, &str)]) -> Project {
        let src = dir.join("src");
        std::fs::create_dir_all(&src).expect("create src dir");

        let mut parsed = HashMap::new();
        for (name, content) in files {
            let path = src.join(name);
            std::fs::write(&path, content).expect("write test source file");
            parsed.insert(
                path,
                ParsedFile {
                    source: content.to_string(),
                },
            );
        }

        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");

        Project {
            path: dir.to_path_buf(),
            language: Some(Language::Rust),
            binary_paths: vec![],
            manifest_path: Some(dir.join("Cargo.toml")),
            runner: None,
            include_tests: false,
            parsed_files: OnceLock::from(parsed),
            help_output: OnceLock::new(),
        }
    }

    #[test]
    fn applicable_when_language_detected() {
        let dir = temp_dir("applicable");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(DryRunCheck.applicable(&project));
    }

    #[test]
    fn not_applicable_without_language() {
        let dir = temp_dir("no-lang");
        let project = Project::discover(&dir).expect("discover test project");
        assert!(!DryRunCheck.applicable(&project));
    }

    #[test]
    fn skip_when_no_write_commands() {
        let dir = temp_dir("no-write");
        let project = make_project(
            &dir,
            &[(
                "main.rs",
                r#"
fn main() {
    let args = Cli::parse();
    println!("reading data");
}
"#,
            )],
        );
        let result = DryRunCheck.run(&project).expect("run check");
        assert!(matches!(result.status, CheckStatus::Skip(_)));
    }

    #[test]
    fn pass_when_dry_run_present() {
        let dir = temp_dir("pass");
        let project = make_project(
            &dir,
            &[(
                "cli.rs",
                r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "delete")]
    delete: bool,

    #[arg(long = "dry-run")]
    dry_run: bool,
}
"#,
            )],
        );
        let result = DryRunCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn warn_when_write_without_dry_run() {
        let dir = temp_dir("warn");
        let project = make_project(
            &dir,
            &[(
                "cli.rs",
                r#"
#[derive(Parser)]
struct Cli {
    #[arg(long = "delete")]
    delete: bool,

    #[arg(long)]
    force: bool,
}
"#,
            )],
        );
        let result = DryRunCheck.run(&project).expect("run check");
        assert!(matches!(result.status, CheckStatus::Warn(_)));
        if let CheckStatus::Warn(evidence) = &result.status {
            assert!(evidence.contains("dry-run"));
        }
    }

    #[test]
    fn detects_write_from_multiple_keywords() {
        for keyword in &[
            "--deploy",
            "--install",
            "--push",
            "\"create\"",
            "\"update\"",
        ] {
            let dir = temp_dir(&format!("kw-{}", keyword.replace('"', "")));
            let source = format!("let flag = \"{keyword}\";");
            let project = make_project(&dir, &[("main.rs", &source)]);
            let result = DryRunCheck.run(&project).expect("run check");
            assert!(
                !matches!(result.status, CheckStatus::Skip(_)),
                "keyword {keyword} should be detected as a write command"
            );
        }
    }

    #[test]
    fn pass_with_dry_run_underscore_variant() {
        let dir = temp_dir("underscore");
        let project = make_project(
            &dir,
            &[(
                "cli.rs",
                r#"
struct Cli {
    #[arg(long = "delete")]
    delete: bool,

    #[arg(long)]
    dry_run: bool,
}
"#,
            )],
        );
        let result = DryRunCheck.run(&project).expect("run check");
        assert_eq!(result.status, CheckStatus::Pass);
    }

    #[test]
    fn metadata_is_correct() {
        let check = DryRunCheck;
        assert_eq!(check.id(), "p5-dry-run");
        assert_eq!(check.group(), CheckGroup::P5);
        assert_eq!(check.layer(), CheckLayer::Project);
    }
}
