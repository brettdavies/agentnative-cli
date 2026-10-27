//! One parse per source file per run, shared by every source audit.
//!
//! A source audit that matches patterns implements [`SourceScan`]: a per-file
//! `scan` over a [`SourceFile`] parsed once, returning a typed finding. Before
//! the audit loop, [`prepare`] parses each file of a language once and runs
//! every runnable audit's scan on it, then drops the tree, so memory holds one
//! tree at a time. Each audit's `run()` takes its findings with [`findings`]
//! and aggregates them; with nothing prepared (a unit test, a direct caller),
//! [`findings`] runs the same scan over that audit alone.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use ast_grep_core::{AstGrep, Node};
use ast_grep_language::{Python, Rust};

use crate::audit::Audit;
use crate::project::{Language, Project};

/// A source file parsed once for every source audit of its language.
pub struct SourceFile<'a, L: LanguageExt> {
    path: &'a Path,
    tree: AstGrep<StrDoc<L>>,
    include_tests: bool,
}

impl<L: LanguageExt> std::fmt::Debug for SourceFile<'_, L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceFile")
            .field("path", &self.path)
            .field("include_tests", &self.include_tests)
            .finish_non_exhaustive()
    }
}

impl<'a, L: LanguageExt> SourceFile<'a, L> {
    pub fn parse(path: &'a Path, source: &str, lang: L, include_tests: bool) -> Self {
        Self {
            path,
            tree: lang.ast_grep(source),
            include_tests,
        }
    }

    pub fn path(&self) -> &Path {
        self.path
    }

    /// The path as evidence prints it.
    pub fn label(&self) -> String {
        self.path.display().to_string()
    }

    pub fn source(&self) -> &str {
        self.tree.source()
    }

    pub fn root(&self) -> Node<'_, StrDoc<L>> {
        self.tree.root()
    }

    /// Whether the run reads test code (`--include-tests`).
    pub fn include_tests(&self) -> bool {
        self.include_tests
    }
}

/// A Rust source string parsed as `file`, for unit tests of a scan.
#[cfg(test)]
pub fn rust(source: &str, file: &'static str) -> SourceFile<'static, Rust> {
    SourceFile::parse(Path::new(file), source, Rust, false)
}

/// A Python source string parsed as `file`, for unit tests of a scan.
#[cfg(test)]
pub fn python(source: &str, file: &'static str) -> SourceFile<'static, Python> {
    SourceFile::parse(Path::new(file), source, Python, false)
}

/// The per-file half of a source audit.
pub trait SourceScan<L: LanguageExt>: Sync {
    type Finding: Send + 'static;

    fn scan(&self, file: &SourceFile<'_, L>) -> Self::Finding;
}

/// [`SourceScan`] with its finding type erased, so one pass can drive
/// audits whose findings differ.
pub trait ErasedScan<L: LanguageExt>: Sync {
    fn scan_erased(&self, file: &SourceFile<'_, L>) -> Box<dyn Any + Send>;
}

impl<L: LanguageExt, T: SourceScan<L>> ErasedScan<L> for T {
    fn scan_erased(&self, file: &SourceFile<'_, L>) -> Box<dyn Any + Send> {
        Box::new(self.scan(file))
    }
}

/// The scan an audit contributes to its language's pass.
pub enum Scanner<'a> {
    Rust(&'a dyn ErasedScan<Rust>),
    Python(&'a dyn ErasedScan<Python>),
}

impl std::fmt::Debug for Scanner<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Scanner::Rust(_) => "Scanner::Rust",
            Scanner::Python(_) => "Scanner::Python",
        })
    }
}

/// One audit's finding for one file, its type erased.
type Finding = Box<dyn Any + Send>;

/// Findings prepared for one run, per language and audit id. Audit ids are
/// not unique across languages: `p6-no-color` has a Rust and a Python audit.
#[derive(Default)]
pub struct PreparedFindings {
    languages: RefCell<HashSet<Language>>,
    by_audit: RefCell<HashMap<(Language, String), Vec<Finding>>>,
}

impl std::fmt::Debug for PreparedFindings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedFindings")
            .field("languages", &self.languages.borrow())
            .field("audits", &self.by_audit.borrow().len())
            .finish()
    }
}

/// Parse each `language` file once and run every scan `audits` contribute,
/// storing each audit's findings in path order for its `run()` to take.
pub fn prepare(project: &Project, language: Language, audits: &[&dyn Audit]) {
    match language {
        Language::Rust => {
            let scanners = audits.iter().filter_map(|audit| match audit.scanner() {
                Some(Scanner::Rust(scan)) => Some((audit.id(), scan)),
                _ => None,
            });
            prepare_with(project, language, Rust, scanners.collect());
        }
        Language::Python => {
            let scanners = audits.iter().filter_map(|audit| match audit.scanner() {
                Some(Scanner::Python(scan)) => Some((audit.id(), scan)),
                _ => None,
            });
            prepare_with(project, language, Python, scanners.collect());
        }
        _ => {}
    }
}

fn prepare_with<L: LanguageExt + Copy>(
    project: &Project,
    language: Language,
    lang: L,
    scanners: Vec<(&str, &dyn ErasedScan<L>)>,
) {
    let mut found: Vec<Vec<Finding>> = scanners.iter().map(|_| Vec::new()).collect();
    if !scanners.is_empty() {
        for (path, parsed) in project.parsed_files(language) {
            let file = SourceFile::parse(path, &parsed.source, lang, project.include_tests);
            for ((_, scan), out) in scanners.iter().zip(found.iter_mut()) {
                out.push(scan.scan_erased(&file));
            }
        }
    }
    let store = &project.prepared_findings;
    let mut by_audit = store.by_audit.borrow_mut();
    for ((id, _), findings) in scanners.into_iter().zip(found) {
        by_audit.insert((language, id.to_string()), findings);
    }
    store.languages.borrow_mut().insert(language);
}

/// `audit`'s findings for every `language` file, in path order: the ones
/// [`prepare`] stored for it, or a scan over this audit alone when none were.
pub fn findings<'p, L, A>(
    project: &'p Project,
    language: Language,
    lang: L,
    audit: &A,
) -> Vec<(&'p Path, A::Finding)>
where
    L: LanguageExt + Copy,
    A: Audit + SourceScan<L>,
{
    let files = project.parsed_files(language);
    let store = &project.prepared_findings;
    let prepared = store
        .by_audit
        .borrow_mut()
        .remove(&(language, audit.id().to_string()));
    if let Some(prepared) = prepared {
        let typed: Option<Vec<A::Finding>> = prepared
            .into_iter()
            .map(|finding| finding.downcast::<A::Finding>().ok().map(|f| *f))
            .collect();
        match typed {
            Some(typed) if typed.len() == files.len() => {
                return files.keys().map(PathBuf::as_path).zip(typed).collect();
            }
            _ => debug_assert!(false, "prepared findings for {} do not fit it", audit.id()),
        }
    } else {
        debug_assert!(
            !store.languages.borrow().contains(&language),
            "{} ran after its language was prepared without it",
            audit.id()
        );
    }
    files
        .iter()
        .map(|(path, parsed)| {
            let file = SourceFile::parse(path, &parsed.source, lang, project.include_tests);
            (path.as_path(), audit.scan(&file))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::types::{AuditGroup, AuditLayer, AuditResult};

    /// A source audit that records each file it scans.
    struct Counted {
        id: &'static str,
        language: Language,
        scans: AtomicUsize,
    }

    impl Counted {
        fn new(id: &'static str, language: Language) -> Self {
            Self {
                id,
                language,
                scans: AtomicUsize::new(0),
            }
        }

        fn scans(&self) -> usize {
            self.scans.load(Ordering::SeqCst)
        }
    }

    impl Audit for Counted {
        fn id(&self) -> &str {
            self.id
        }
        fn label(&self) -> &'static str {
            "counted"
        }
        fn group(&self) -> AuditGroup {
            AuditGroup::P6
        }
        fn layer(&self) -> AuditLayer {
            AuditLayer::Source
        }
        fn applicable(&self, _project: &Project) -> bool {
            true
        }
        fn run(&self, _project: &Project) -> anyhow::Result<AuditResult> {
            Ok(self.skip(String::new()))
        }
        fn scanner(&self) -> Option<Scanner<'_>> {
            Some(match self.language {
                Language::Python => Scanner::Python(self),
                _ => Scanner::Rust(self),
            })
        }
    }

    impl SourceScan<Rust> for Counted {
        type Finding = String;
        fn scan(&self, file: &SourceFile<'_, Rust>) -> String {
            self.scans.fetch_add(1, Ordering::SeqCst);
            file.source().trim().to_string()
        }
    }

    impl SourceScan<Python> for Counted {
        type Finding = String;
        fn scan(&self, file: &SourceFile<'_, Python>) -> String {
            self.scans.fetch_add(1, Ordering::SeqCst);
            file.source().trim().to_string()
        }
    }

    fn project_with(name: &str, files: &[(&str, &str)]) -> Project {
        let dir = std::env::temp_dir().join(format!("anc-pass-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).expect("create test dir");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"t\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        for (file, source) in files {
            std::fs::write(dir.join("src").join(file), source).expect("write source");
        }
        Project::discover(&dir).expect("discover test project")
    }

    fn names(found: &[(&Path, String)]) -> Vec<String> {
        found
            .iter()
            .map(|(path, finding)| {
                let name = path.file_name().expect("file name").to_string_lossy();
                format!("{name}={finding}")
            })
            .collect()
    }

    #[test]
    fn prepare_scans_each_file_once_for_runnable_audits_only() {
        let project = project_with("runnable", &[("a.rs", "// a"), ("b.rs", "// b")]);
        let runnable = Counted::new("x", Language::Rust);
        let excluded = Counted::new("y", Language::Rust);
        prepare(&project, Language::Rust, &[&runnable]);
        assert_eq!(runnable.scans(), 2);
        assert_eq!(excluded.scans(), 0);
    }

    #[test]
    fn findings_take_prepared_results_that_match_a_solo_pass() {
        let files = [("a.rs", "// a"), ("b.rs", "// b")];
        let prepared_project = project_with("prepared", &files);
        let audit = Counted::new("x", Language::Rust);
        prepare(&prepared_project, Language::Rust, &[&audit]);
        let prepared = findings(&prepared_project, Language::Rust, Rust, &audit);
        assert_eq!(
            audit.scans(),
            2,
            "prepared findings are taken, not rescanned"
        );

        let solo_project = project_with("solo", &files);
        let solo = findings(&solo_project, Language::Rust, Rust, &audit);
        assert_eq!(
            audit.scans(),
            4,
            "nothing prepared, so the audit scans alone"
        );
        assert_eq!(names(&prepared), ["a.rs=// a", "b.rs=// b"]);
        assert_eq!(names(&prepared), names(&solo));
    }

    #[test]
    fn audits_sharing_an_id_across_languages_take_their_own_findings() {
        let project = project_with("shared-id", &[("a.rs", "// rust"), ("b.py", "# python")]);
        let rust_audit = Counted::new("p6-no-color", Language::Rust);
        let python_audit = Counted::new("p6-no-color", Language::Python);
        prepare(&project, Language::Rust, &[&rust_audit]);
        prepare(&project, Language::Python, &[&python_audit]);

        let rust_found = findings(&project, Language::Rust, Rust, &rust_audit);
        let python_found = findings(&project, Language::Python, Python, &python_audit);
        assert_eq!(names(&rust_found), ["a.rs=// rust"]);
        assert_eq!(names(&python_found), ["b.py=# python"]);
        assert_eq!(rust_audit.scans() + python_audit.scans(), 2);
    }
}
