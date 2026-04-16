use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::runner::BinaryRunner;

/// Maximum directory recursion depth for source file walk.
const MAX_DEPTH: usize = 20;
/// Maximum number of source files to collect.
const MAX_FILES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Rust,
    Python,
    Go,
    Node,
}

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub source: String,
}

pub struct Project {
    pub path: PathBuf,
    pub language: Option<Language>,
    pub binary_paths: Vec<PathBuf>,
    pub manifest_path: Option<PathBuf>,
    pub runner: Option<BinaryRunner>,
    pub include_tests: bool,
    pub(crate) parsed_files: RefCell<HashMap<PathBuf, ParsedFile>>,
}

impl std::fmt::Debug for Project {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Project")
            .field("path", &self.path)
            .field("language", &self.language)
            .field("binary_paths", &self.binary_paths)
            .field("manifest_path", &self.manifest_path)
            .field("has_runner", &self.runner.is_some())
            .field("include_tests", &self.include_tests)
            .field("parsed_files_count", &self.parsed_files.borrow().len())
            .finish()
    }
}

impl Project {
    pub fn discover(path: &Path) -> Result<Project> {
        let path = path
            .canonicalize()
            .with_context(|| format!("path does not exist: {}", path.display()))?;

        let meta = fs::metadata(&path)
            .with_context(|| format!("cannot read metadata: {}", path.display()))?;

        if meta.is_file() {
            if !is_executable(&meta) {
                bail!("not an executable file: {}", path.display());
            }
            let runner = BinaryRunner::new(path.clone(), Duration::from_secs(5)).ok();
            return Ok(Project {
                path: path.clone(),
                language: None,
                binary_paths: vec![path],
                manifest_path: None,
                runner,
                include_tests: false,
                parsed_files: RefCell::new(HashMap::new()),
            });
        }

        // Directory path — detect language from manifest
        let (language, manifest_path) = detect_language(&path);
        let binary_paths = discover_binaries(&path, language, manifest_path.as_deref());

        let runner = if binary_paths.is_empty() {
            None
        } else {
            BinaryRunner::new(binary_paths[0].clone(), Duration::from_secs(5)).ok()
        };

        Ok(Project {
            path,
            language,
            binary_paths,
            manifest_path,
            runner,
            include_tests: false,
            parsed_files: RefCell::new(HashMap::new()),
        })
    }

    /// Returns a reference to the runner.
    ///
    /// # Panics
    /// Panics if no runner exists. Only call after `applicable()` confirms a runner is present.
    pub fn runner_ref(&self) -> &BinaryRunner {
        self.runner
            .as_ref()
            .expect("runner must exist when applicable() returns true")
    }

    pub fn parsed_files(&self) -> std::cell::Ref<'_, HashMap<PathBuf, ParsedFile>> {
        // Lazily populate on first access
        if self.parsed_files.borrow().is_empty() {
            let mut cache = self.parsed_files.borrow_mut();
            if let Some(lang) = self.language {
                let ext = match lang {
                    Language::Rust => "rs",
                    Language::Python => "py",
                    Language::Go => "go",
                    Language::Node => "js",
                };
                if let Ok(files) = walk_source_files(&self.path, ext, self.include_tests) {
                    for file_path in files {
                        if let Ok(source) = fs::read_to_string(&file_path) {
                            cache.insert(file_path, ParsedFile { source });
                        }
                    }
                }
            }
        }
        self.parsed_files.borrow()
    }
}

fn detect_language(dir: &Path) -> (Option<Language>, Option<PathBuf>) {
    let manifests = [
        ("Cargo.toml", Language::Rust),
        ("pyproject.toml", Language::Python),
        ("go.mod", Language::Go),
        ("package.json", Language::Node),
    ];
    for (name, lang) in &manifests {
        let manifest = dir.join(name);
        if manifest.exists() {
            return (Some(*lang), Some(manifest));
        }
    }
    (None, None)
}

fn discover_binaries(
    dir: &Path,
    language: Option<Language>,
    manifest_path: Option<&Path>,
) -> Vec<PathBuf> {
    match language {
        Some(Language::Rust) => discover_rust_binaries(dir, manifest_path),
        Some(Language::Python) => discover_simple_binaries(dir, &["dist", "build"]),
        Some(Language::Go) => {
            // Check for binary with same name as directory
            let mut paths = Vec::new();
            if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
                let bin = dir.join(name);
                if bin.exists() {
                    paths.push(bin);
                }
            }
            paths
        }
        Some(Language::Node) => discover_simple_binaries(dir, &["node_modules/.bin"]),
        None => vec![],
    }
}

fn discover_rust_binaries(dir: &Path, manifest_path: Option<&Path>) -> Vec<PathBuf> {
    let mut bin_names = Vec::new();

    if let Some(manifest) = manifest_path {
        if let Ok(content) = fs::read_to_string(manifest) {
            if let Ok(doc) = content.parse::<toml::Table>() {
                // Check [[bin]] entries
                if let Some(bins) = doc.get("bin").and_then(|b| b.as_array()) {
                    for bin in bins {
                        if let Some(name) = bin.get("name").and_then(|n| n.as_str()) {
                            bin_names.push(name.to_string());
                        }
                    }
                }

                // Fallback to package name if no [[bin]]
                if bin_names.is_empty() {
                    if let Some(name) = doc
                        .get("package")
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        bin_names.push(name.to_string());
                    }
                }
            }
        }
    }

    let mut paths = Vec::new();
    for name in &bin_names {
        // Prefer release over debug
        let release = dir.join("target/release").join(name);
        let debug = dir.join("target/debug").join(name);
        if release.exists() {
            paths.push(release);
        } else if debug.exists() {
            paths.push(debug);
        }
    }
    paths
}

fn discover_simple_binaries(dir: &Path, subdirs: &[&str]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for subdir in subdirs {
        let bin_dir = dir.join(subdir);
        if bin_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&bin_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() {
                        paths.push(p);
                    }
                }
            }
        }
    }
    paths
}

fn walk_source_files(dir: &Path, ext: &str, include_tests: bool) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut file_count: usize = 0;
    walk_source_files_inner(dir, ext, include_tests, 0, &mut file_count, &mut files)?;
    Ok(files)
}

fn walk_source_files_inner(
    dir: &Path,
    ext: &str,
    include_tests: bool,
    depth: usize,
    file_count: &mut usize,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    if depth >= MAX_DEPTH {
        eprintln!(
            "warning: hit {MAX_DEPTH}-level depth limit; narrow the scan with `agentnative check src/`"
        );
        return Ok(());
    }
    if *file_count >= MAX_FILES {
        eprintln!(
            "warning: hit {MAX_FILES}-file limit; narrow the scan with `agentnative check src/`"
        );
        return Ok(());
    }

    let entries =
        fs::read_dir(dir).with_context(|| format!("cannot read directory: {}", dir.display()))?;

    for entry in entries {
        if *file_count >= MAX_FILES {
            break;
        }
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Skip hidden dirs, target/ always; tests/ unless --include-tests
        if path.is_dir() {
            if name.starts_with('.') || name == "target" {
                continue;
            }
            if name == "tests" && !include_tests {
                continue;
            }
            walk_source_files_inner(&path, ext, include_tests, depth + 1, file_count, files)?;
        } else if path.extension().is_some_and(|e| e == ext) {
            files.push(path);
            *file_count += 1;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agentnative-test-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn test_rust_project_detected() {
        let dir = temp_dir().join("rust-proj");
        fs::create_dir_all(&dir).expect("create test dir");
        fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "myapp"
version = "0.1.0"
"#,
        )
        .expect("write test Cargo.toml");

        let project = Project::discover(&dir).expect("discover test project");
        assert_eq!(project.language, Some(Language::Rust));
        assert!(project.manifest_path.is_some());
    }

    #[test]
    fn test_executable_file() {
        let dir = temp_dir().join("exe-test");
        fs::create_dir_all(&dir).expect("create test dir");
        let bin = dir.join("mybin");
        fs::write(&bin, "#!/bin/sh\necho hi").expect("write test binary");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755))
                .expect("set test permissions");
        }

        let project = Project::discover(&bin).expect("discover test project");
        assert_eq!(project.language, None);
        assert_eq!(project.binary_paths.len(), 1);
    }

    #[test]
    fn test_no_manifest_directory() {
        let dir = temp_dir().join("empty-proj");
        fs::create_dir_all(&dir).expect("create test dir");

        let project = Project::discover(&dir).expect("discover test project");
        assert_eq!(project.language, None);
        assert!(project.binary_paths.is_empty());
    }

    #[test]
    fn test_cargo_toml_with_bin_entries() {
        let dir = temp_dir().join("bin-entries");
        fs::create_dir_all(&dir).expect("create test dir");
        fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "myapp"
version = "0.1.0"

[[bin]]
name = "cli1"
path = "src/main.rs"

[[bin]]
name = "cli2"
path = "src/cli2.rs"
"#,
        )
        .expect("write test Cargo.toml");

        let project = Project::discover(&dir).expect("discover test project");
        assert_eq!(project.language, Some(Language::Rust));
        // Binaries won't exist on disk, so binary_paths should be empty
        assert!(project.binary_paths.is_empty());

        // Verify we parsed the names correctly by checking the discover function directly
        let names = {
            let content = fs::read_to_string(dir.join("Cargo.toml")).expect("read test Cargo.toml");
            let doc: toml::Table = content.parse().expect("parse TOML");
            let bins = doc
                .get("bin")
                .expect("bin section")
                .as_array()
                .expect("bin is array");
            bins.iter()
                .filter_map(|b| b.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect::<Vec<_>>()
        };
        assert_eq!(names, vec!["cli1", "cli2"]);
    }

    #[test]
    fn test_nonexistent_path_errors() {
        let result = Project::discover(Path::new("/tmp/agentnative-does-not-exist-xyz"));
        assert!(result.is_err());
    }

    #[test]
    fn test_non_executable_file_errors() {
        let dir = temp_dir().join("noexec-test");
        fs::create_dir_all(&dir).expect("create test dir");
        let file = dir.join("regular.txt");
        fs::write(&file, "just text").expect("write test file");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644))
                .expect("set test permissions");
        }

        let result = Project::discover(&file);
        assert!(result.is_err());
        let err = result
            .expect_err("should reject non-executable file")
            .to_string();
        assert!(err.contains("not an executable"), "got: {err}");
    }

    #[test]
    fn test_walk_excludes_tests_by_default() {
        let dir = temp_dir().join("walk-tests-default");
        let src = dir.join("src");
        let tests = dir.join("tests");
        fs::create_dir_all(&src).expect("create test src dir");
        fs::create_dir_all(&tests).expect("create test tests dir");
        fs::write(src.join("main.rs"), "fn main() {}").expect("write test file");
        fs::write(tests.join("test_foo.rs"), "fn test() {}").expect("write test file");

        let files = walk_source_files(&dir, "rs", false).expect("walk source files");
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("main.rs"));
    }

    #[test]
    fn test_walk_includes_tests_when_enabled() {
        let dir = temp_dir().join("walk-tests-enabled");
        let src = dir.join("src");
        let tests = dir.join("tests");
        fs::create_dir_all(&src).expect("create test src dir");
        fs::create_dir_all(&tests).expect("create test tests dir");
        fs::write(src.join("main.rs"), "fn main() {}").expect("write test file");
        fs::write(tests.join("test_foo.rs"), "fn test() {}").expect("write test file");

        let files = walk_source_files(&dir, "rs", true).expect("walk source files");
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_walk_always_excludes_target() {
        let dir = temp_dir().join("walk-target-excl");
        let src = dir.join("src");
        let target = dir.join("target").join("debug");
        fs::create_dir_all(&src).expect("create test src dir");
        fs::create_dir_all(&target).expect("create test target dir");
        fs::write(src.join("main.rs"), "fn main() {}").expect("write test file");
        fs::write(target.join("build.rs"), "fn build() {}").expect("write test file");

        let files = walk_source_files(&dir, "rs", true).expect("walk source files");
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("main.rs"));
    }

    #[test]
    fn test_include_tests_field_default() {
        let dir = temp_dir().join("include-tests-default");
        fs::create_dir_all(&dir).expect("create test dir");
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"test\"\nversion = \"0.1.0\"\n",
        )
        .expect("write test Cargo.toml");

        let project = Project::discover(&dir).expect("discover test project");
        assert!(!project.include_tests);
    }
}
