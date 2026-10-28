//! One parse per source file per run, shared by every source audit.
//!
//! A source audit that matches patterns implements [`SourceScan`]: a per-file
//! `scan` over a [`SourceFile`] parsed once, returning a typed finding. Before
//! the audit loop, [`prepare`] parses each file of a language once and runs
//! every runnable audit's scan on it, then drops the tree. The files are split
//! into contiguous runs across [`workers`] threads, each holding one tree at a
//! time, and their findings are joined back in path order. Each audit's
//! `run()` takes its findings with [`findings`] and aggregates them; with
//! nothing prepared (a unit test, a direct caller), [`findings`] runs the same
//! scan over that audit alone.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::thread;

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

/// The most workers a run starts unless `--jobs` asks for more. Each worker
/// holds one parsed tree, so peak memory grows with the count. A release build
/// reading 28.7 MB of Rust on a 16-core host took 6.9 s at 56 MB peak RSS on
/// one worker; 2.2 s at 74-77 MB on 4; 1.7 s at 82-83 MB on 5; and 1.3-1.4 s
/// at 95-97 MB on 8. Four is the largest count that stays clearly within 1.5x
/// the one-worker peak.
const DEFAULT_MAX_WORKERS: usize = 4;

/// A worker's stack: the 8 MiB a Linux main thread gets. Several scans recurse
/// once per tree level, and a spawned thread's 2 MiB default overflows on
/// deeply nested files the main thread audits.
const WORKER_STACK_BYTES: usize = 8 * 1024 * 1024;

/// How many threads scan `files` files: `jobs` when set, else the smaller of
/// the `available` cores and `DEFAULT_MAX_WORKERS`; never more than the
/// files, and at least one.
pub fn workers(jobs: Option<NonZeroUsize>, available: usize, files: usize) -> usize {
    jobs.map_or(available.min(DEFAULT_MAX_WORKERS), NonZeroUsize::get)
        .min(files)
        .max(1)
}

/// Parse each `language` file once and run every scan `audits` contribute,
/// storing each audit's findings in path order for its `run()` to take.
/// `jobs` overrides the default worker count ([`workers`]).
pub fn prepare(
    project: &Project,
    language: Language,
    audits: &[&dyn Audit],
    jobs: Option<NonZeroUsize>,
) {
    match language {
        Language::Rust => {
            let scanners = audits.iter().filter_map(|audit| match audit.scanner() {
                Some(Scanner::Rust(scan)) => Some((audit.id(), scan)),
                _ => None,
            });
            prepare_with(project, language, Rust, scanners.collect(), jobs);
        }
        Language::Python => {
            let scanners = audits.iter().filter_map(|audit| match audit.scanner() {
                Some(Scanner::Python(scan)) => Some((audit.id(), scan)),
                _ => None,
            });
            prepare_with(project, language, Python, scanners.collect(), jobs);
        }
        _ => {}
    }
}

fn prepare_with<L: LanguageExt + Copy + Send>(
    project: &Project,
    language: Language,
    lang: L,
    scanners: Vec<(&str, &dyn ErasedScan<L>)>,
    jobs: Option<NonZeroUsize>,
) {
    let scans: Vec<&dyn ErasedScan<L>> = scanners.iter().map(|(_, scan)| *scan).collect();
    let found = if scans.is_empty() {
        Vec::new()
    } else {
        let files: Vec<(&Path, &str)> = project
            .parsed_files(language)
            .iter()
            .map(|(path, parsed)| (path.as_path(), parsed.source.as_str()))
            .collect();
        let available = thread::available_parallelism().map_or(1, NonZeroUsize::get);
        let workers = workers(jobs, available, files.len());
        scan_files(&files, lang, project.include_tests, &scans, workers)
    };
    let store = &project.prepared_findings;
    let mut by_audit = store.by_audit.borrow_mut();
    for ((id, _), findings) in scanners.into_iter().zip(found) {
        by_audit.insert((language, id.to_string()), findings);
    }
    store.languages.borrow_mut().insert(language);
}

/// Each scan's findings over `files`, in file order, from `workers` threads
/// that each take one contiguous run of files.
fn scan_files<L: LanguageExt + Copy + Send>(
    files: &[(&Path, &str)],
    lang: L,
    include_tests: bool,
    scans: &[&dyn ErasedScan<L>],
    workers: usize,
) -> Vec<Vec<Finding>> {
    if workers <= 1 {
        return scan_run(files, lang, include_tests, scans);
    }
    let run_len = files.len().div_ceil(workers);
    let runs: Vec<Vec<Vec<Finding>>> = thread::scope(|scope| {
        let handles: Vec<_> = files
            .chunks(run_len)
            .map(|run| {
                thread::Builder::new()
                    .stack_size(WORKER_STACK_BYTES)
                    .spawn_scoped(scope, move || scan_run(run, lang, include_tests, scans))
                    .expect("spawn a source-audit worker")
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    let mut found: Vec<Vec<Finding>> = scans
        .iter()
        .map(|_| Vec::with_capacity(files.len()))
        .collect();
    for run in runs {
        for (out, part) in found.iter_mut().zip(run) {
            out.extend(part);
        }
    }
    found
}

/// Each scan's findings over `files`, in order, on the calling thread.
fn scan_run<L: LanguageExt + Copy>(
    files: &[(&Path, &str)],
    lang: L,
    include_tests: bool,
    scans: &[&dyn ErasedScan<L>],
) -> Vec<Vec<Finding>> {
    let mut found: Vec<Vec<Finding>> = scans
        .iter()
        .map(|_| Vec::with_capacity(files.len()))
        .collect();
    for (path, source) in files {
        let file = SourceFile::parse(path, source, lang, include_tests);
        for (scan, out) in scans.iter().zip(found.iter_mut()) {
            out.push(scan.scan_erased(&file));
        }
    }
    found
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

    fn jobs(n: usize) -> Option<NonZeroUsize> {
        NonZeroUsize::new(n)
    }

    #[test]
    fn workers_take_the_requested_jobs_up_to_the_file_count() {
        assert_eq!(workers(jobs(3), 16, 100), 3);
        assert_eq!(workers(jobs(3), 1, 100), 3, "jobs is not held to the cores");
        assert_eq!(workers(jobs(32), 16, 100), 32, "nor to the default cap");
        assert_eq!(workers(jobs(3), 16, 2), 2);
    }

    #[test]
    fn default_workers_are_the_cores_up_to_the_cap_and_the_file_count() {
        assert_eq!(workers(None, 2, 100), 2);
        assert_eq!(workers(None, 64, 100), DEFAULT_MAX_WORKERS);
        assert_eq!(workers(None, 16, 1), 1, "one file starts one worker");
        assert_eq!(workers(None, 16, 0), 1);
    }

    #[test]
    fn findings_from_several_workers_stay_in_path_order() {
        let files = [
            ("a.rs", "// a"),
            ("b.rs", "// b"),
            ("c.rs", "// c"),
            ("d.rs", "// d"),
            ("e.rs", "// e"),
        ];
        let project = project_with("workers", &files);
        let audit = Counted::new("x", Language::Rust);
        prepare(&project, Language::Rust, &[&audit], jobs(3));
        let found = findings(&project, Language::Rust, Rust, &audit);
        assert_eq!(audit.scans(), 5);
        assert_eq!(
            names(&found),
            [
                "a.rs=// a",
                "b.rs=// b",
                "c.rs=// c",
                "d.rs=// d",
                "e.rs=// e"
            ]
        );
    }

    #[test]
    fn prepare_scans_each_file_once_per_audit() {
        let project = project_with("runnable", &[("a.rs", "// a"), ("b.rs", "// b")]);
        let first = Counted::new("x", Language::Rust);
        let second = Counted::new("y", Language::Rust);
        prepare(&project, Language::Rust, &[&first, &second], None);
        assert_eq!((first.scans(), second.scans()), (2, 2));
    }

    #[test]
    fn findings_take_prepared_results_that_match_a_solo_pass() {
        let files = [("a.rs", "// a"), ("b.rs", "// b")];
        let prepared_project = project_with("prepared", &files);
        let audit = Counted::new("x", Language::Rust);
        prepare(&prepared_project, Language::Rust, &[&audit], None);
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
        prepare(&project, Language::Rust, &[&rust_audit], None);
        prepare(&project, Language::Python, &[&python_audit], None);

        let rust_found = findings(&project, Language::Rust, Rust, &rust_audit);
        let python_found = findings(&project, Language::Python, Python, &python_audit);
        assert_eq!(names(&rust_found), ["a.rs=// rust"]);
        assert_eq!(names(&python_found), ["b.py=# python"]);
        assert_eq!(rust_audit.scans() + python_audit.scans(), 2);
    }
}
