use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;

use crate::anc_toml::ResolvedConfig;
use crate::runner::{BinaryRunner, HelpOutput};

pub use bins::Candidate;

mod bins;
mod inventory;
mod scan;
pub mod select;
mod workspace;

pub use inventory::Inventory;

/// Maximum directory recursion depth for source file walk.
const MAX_DEPTH: usize = 20;
/// Maximum number of source files to collect.
const MAX_FILES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Rust,
    Python,
    Go,
    Node,
}

impl Language {
    /// Every language anc recognizes.
    pub const ALL: [Language; 4] = [
        Language::Rust,
        Language::Python,
        Language::Go,
        Language::Node,
    ];

    /// The manifest file that makes a directory a package of this language.
    pub fn manifest_name(self) -> &'static str {
        match self {
            Language::Rust => "Cargo.toml",
            Language::Python => "pyproject.toml",
            Language::Go => "go.mod",
            Language::Node => "package.json",
        }
    }

    /// The language whose manifest is named `file_name`.
    pub fn of_manifest(file_name: &std::ffi::OsStr) -> Option<Language> {
        Language::ALL
            .into_iter()
            .find(|lang| file_name == lang.manifest_name())
    }

    /// The language a source file with extension `ext` is written in.
    fn of_source_extension(ext: &OsStr) -> Option<Language> {
        [
            ("rs", Language::Rust),
            ("py", Language::Python),
            ("go", Language::Go),
            ("js", Language::Node),
        ]
        .into_iter()
        .find_map(|(known, lang)| (ext == known).then_some(lang))
    }
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
    pub(crate) parsed_files: OnceLock<HashMap<Language, HashMap<PathBuf, ParsedFile>>>,
    pub(crate) help_output: OnceLock<Option<HelpOutput>>,
    /// The merged `.anc.toml` chain for this target, resolved once per run.
    pub anc_config: ResolvedConfig,
    /// Every package under a directory target; empty for a binary target.
    pub inventory: Inventory,
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
            .field(
                "parsed_files_count",
                &self.parsed_files.get().map_or(0, |m| m.len()),
            )
            .field("help_probed", &self.help_output.get().is_some())
            .field("anc_config", &self.anc_config)
            .field("packages", &self.inventory.packages.len())
            .finish()
    }
}

impl Project {
    #[cfg(test)]
    pub fn discover(path: &Path) -> Result<Project> {
        Self::discover_with_tests(path, false)
    }

    /// Discover `path`; `include_tests` lets the package scan and the source
    /// walk enter `tests` directories.
    pub fn discover_with_tests(path: &Path, include_tests: bool) -> Result<Project> {
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
                include_tests,
                parsed_files: OnceLock::new(),
                help_output: OnceLock::new(),
                anc_config: ResolvedConfig::default(),
                inventory: Inventory::default(),
            });
        }

        // Directory path — detect language from manifest
        let inventory = inventory::inventory(&path, include_tests);
        let (language, manifest_path) = detect_language(&path);
        let binary_paths: Vec<PathBuf> = inventory
            .candidates
            .iter()
            .map(|candidate| candidate.path.clone())
            .collect();

        Ok(Project {
            path,
            language,
            binary_paths,
            manifest_path,
            runner: None,
            include_tests,
            parsed_files: OnceLock::new(),
            help_output: OnceLock::new(),
            anc_config: ResolvedConfig::default(),
            inventory,
        })
    }

    /// Grade `candidate`: every behavioral audit probes its binary, and its
    /// package supplies the language and manifest the audits read.
    pub fn grade(&mut self, candidate: &Candidate) {
        self.runner = BinaryRunner::new(candidate.path.clone(), Duration::from_secs(5)).ok();
        self.binary_paths = vec![candidate.path.clone()];
        self.help_output = OnceLock::new();
        self.language = Some(candidate.language);
        self.manifest_path = Some(candidate.manifest.clone());
    }

    /// With nothing graded, anchor manifest reads on the one package that
    /// declares a binary. With several, no manifest is read and
    /// [`Project::manifest_skip`] says why; with none, the audit root's own
    /// package stays.
    pub fn anchor_ungraded(&mut self) {
        let declaring: Vec<(Language, PathBuf)> = self
            .declaring_packages()
            .map(|pkg| (pkg.language, pkg.manifest.clone()))
            .collect();
        match declaring.as_slice() {
            [] => {}
            [(language, manifest)] => {
                self.language = Some(*language);
                self.manifest_path = Some(manifest.clone());
            }
            _ => self.manifest_path = None,
        }
    }

    fn declaring_packages(&self) -> impl Iterator<Item = &inventory::Package> {
        self.inventory
            .packages
            .iter()
            .filter(|pkg| !pkg.bins.is_empty())
    }

    /// Why a manifest-reading audit skips: nothing is graded and several
    /// packages declare binaries, at least one of them in `language` when
    /// the audit reads only that language's manifests.
    pub fn manifest_skip(&self, language: Option<Language>) -> Option<String> {
        if self.manifest_path.is_some() || !self.binary_paths.is_empty() {
            return None;
        }
        let declaring: Vec<&inventory::Package> = self.declaring_packages().collect();
        let relevant = language.is_none_or(|lang| declaring.iter().any(|pkg| pkg.language == lang));
        if declaring.len() < 2 || !relevant {
            return None;
        }
        let names: Vec<&str> = declaring.iter().map(|pkg| pkg.name.as_str()).collect();
        Some(format!(
            "several packages declare binaries ({}) and none is graded, so anc reads no manifest; build one and grade it with --bin to audit its manifest",
            names.join(", ")
        ))
    }

    /// Every language present: the graded package's, then each inventoried
    /// package's in inventory order.
    pub fn languages(&self) -> Vec<Language> {
        let mut found: Vec<Language> = self.language.into_iter().collect();
        for pkg in &self.inventory.packages {
            if !found.contains(&pkg.language) {
                found.push(pkg.language);
            }
        }
        found
    }

    pub fn has_language(&self, language: Language) -> bool {
        self.languages().contains(&language)
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

    /// Lazily probe `<binary> --help` exactly once, returning a shared
    /// reference that behavioral audits consume. Returns `None` when the
    /// project has no runner or the help probe fails outright (e.g., binary
    /// missing). `HelpOutput` itself handles partial captures from timeouts
    /// and crashes — those still yield `Some(_)`.
    pub fn help_output(&self) -> Option<&HelpOutput> {
        self.help_output
            .get_or_init(|| {
                let runner = self.runner.as_ref()?;
                HelpOutput::probe(runner).ok()
            })
            .as_ref()
    }

    /// Every `language` source file under the audit root that the shared
    /// walker reaches, read once for all languages.
    pub fn parsed_files(&self, language: Language) -> &HashMap<PathBuf, ParsedFile> {
        static NONE: LazyLock<HashMap<PathBuf, ParsedFile>> = LazyLock::new(HashMap::new);
        self.parsed_files
            .get_or_init(|| read_sources(&self.path, self.include_tests))
            .get(&language)
            .unwrap_or(&NONE)
    }
}

/// Source files under `root` by language, walked with the package scan's
/// rules: the repository's ignore files, no hidden, build, or dependency
/// directories, and `tests` only with `include_tests`. Example programs
/// beside a manifest are skipped too ([`scan::source_walker`]).
fn read_sources(
    root: &Path,
    include_tests: bool,
) -> HashMap<Language, HashMap<PathBuf, ParsedFile>> {
    let mut found: HashMap<Language, HashMap<PathBuf, ParsedFile>> = HashMap::new();
    if !root.is_dir() {
        return found;
    }
    let mut count = 0usize;
    for entry in scan::source_walker(root, include_tests).build().flatten() {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let Some(language) = entry
            .path()
            .extension()
            .and_then(Language::of_source_extension)
        else {
            continue;
        };
        if count >= MAX_FILES {
            eprintln!("warning: hit {MAX_FILES}-file limit; narrow the scan with `anc audit src/`");
            break;
        }
        if let Ok(source) = fs::read_to_string(entry.path()) {
            count += 1;
            found
                .entry(language)
                .or_default()
                .insert(entry.into_path(), ParsedFile { source });
        }
    }
    found
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

    /// Regression for the v0.4.0-spec-sync stale-release-binary trap:
    /// when both `target/release/<bin>` and `target/debug/<bin>` exist,
    /// pick the newer one by mtime (so dev workflows where `cargo run`
    /// only refreshes debug don't probe a stale release binary). See
    /// docs/solutions/test-failures/stale-release-binary-dogfood-fail-2026-05-07.md.
    #[cfg(unix)]
    #[test]
    fn test_discover_picks_newer_artifact_by_mtime() {
        use std::fs::File;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, SystemTime};

        let dir = temp_dir().join("mtime-pick");
        fs::create_dir_all(dir.join("target/release")).expect("mkdir release");
        fs::create_dir_all(dir.join("target/debug")).expect("mkdir debug");
        fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "myapp"
version = "0.1.0"
"#,
        )
        .expect("write Cargo.toml");

        fs::create_dir_all(dir.join("src")).expect("mkdir src");
        fs::write(dir.join("src/main.rs"), "fn main() {}\n").expect("write src/main.rs");
        let release_bin = dir.join("target/release/myapp");
        let debug_bin = dir.join("target/debug/myapp");
        fs::write(&release_bin, "#!/bin/sh\necho stale").expect("write release binary");
        fs::write(&debug_bin, "#!/bin/sh\necho fresh").expect("write debug binary");
        fs::set_permissions(&release_bin, fs::Permissions::from_mode(0o755))
            .expect("chmod release");
        fs::set_permissions(&debug_bin, fs::Permissions::from_mode(0o755)).expect("chmod debug");

        // Stamp release with an old mtime; debug stays at "now".
        let one_hour_ago = SystemTime::now() - Duration::from_secs(3600);
        File::options()
            .write(true)
            .open(&release_bin)
            .expect("open release for mtime")
            .set_modified(one_hour_ago)
            .expect("set release mtime");

        let project = Project::discover(&dir).expect("discover test project");
        assert_eq!(project.binary_paths.len(), 1);
        assert_eq!(
            project.binary_paths[0], debug_bin,
            "discover should pick the newer (debug) binary when release is stale; \
             got {:?}",
            project.binary_paths[0],
        );
    }

    /// Symmetric case: when release is newer than debug (e.g., `cargo build
    /// --release` was just run), `pick_newer_artifact` returns release.
    #[cfg(unix)]
    #[test]
    fn test_discover_picks_release_when_newer() {
        use std::fs::File;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, SystemTime};

        let dir = temp_dir().join("mtime-release");
        fs::create_dir_all(dir.join("target/release")).expect("mkdir release");
        fs::create_dir_all(dir.join("target/debug")).expect("mkdir debug");
        fs::write(
            dir.join("Cargo.toml"),
            r#"[package]
name = "myapp"
version = "0.1.0"
"#,
        )
        .expect("write Cargo.toml");

        fs::create_dir_all(dir.join("src")).expect("mkdir src");
        fs::write(dir.join("src/main.rs"), "fn main() {}\n").expect("write src/main.rs");
        let release_bin = dir.join("target/release/myapp");
        let debug_bin = dir.join("target/debug/myapp");
        fs::write(&release_bin, "#!/bin/sh\necho fresh").expect("write release binary");
        fs::write(&debug_bin, "#!/bin/sh\necho stale").expect("write debug binary");
        fs::set_permissions(&release_bin, fs::Permissions::from_mode(0o755))
            .expect("chmod release");
        fs::set_permissions(&debug_bin, fs::Permissions::from_mode(0o755)).expect("chmod debug");

        // Stamp debug with an old mtime; release stays at "now".
        let one_hour_ago = SystemTime::now() - Duration::from_secs(3600);
        File::options()
            .write(true)
            .open(&debug_bin)
            .expect("open debug for mtime")
            .set_modified(one_hour_ago)
            .expect("set debug mtime");

        let project = Project::discover(&dir).expect("discover test project");
        assert_eq!(project.binary_paths.len(), 1);
        assert_eq!(
            project.binary_paths[0], release_bin,
            "discover should pick release when it's newer than debug",
        );
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

    fn rust_sources(dir: &Path, include_tests: bool) -> Vec<PathBuf> {
        read_sources(dir, include_tests)
            .remove(&Language::Rust)
            .map(|files| files.into_keys().collect())
            .unwrap_or_default()
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

        let files = rust_sources(&dir, false);
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

        let files = rust_sources(&dir, true);
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

        let files = rust_sources(&dir, true);
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
