//! The tree scan: every package manifest under the audit root that the
//! repository does not ignore.

use std::path::{Path, PathBuf};

use ignore::{DirEntry, WalkBuilder};

use super::{Language, MAX_DEPTH, MAX_FILES};

/// Directories the scan never enters, beside hidden ones and whatever the
/// repository's ignore files exclude: build output and installed
/// dependencies, never a package's own source.
const SKIPPED_DIRS: &[&str] = &["target", "node_modules", "vendor", "venv", "dist", "build"];

/// A walker over `root` honoring the repository's `.gitignore` files and
/// `.git/info/exclude`, but not the operator's global excludes file, so the
/// same repository walks the same way on every machine. It skips hidden
/// directories, [`SKIPPED_DIRS`], and `tests` unless `include_tests`, does
/// not follow symlinks, and stops at [`MAX_DEPTH`].
pub fn walker(root: &Path, include_tests: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(false)
        .hidden(true)
        .parents(true)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(true)
        .follow_links(false)
        .max_depth(Some(MAX_DEPTH))
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(move |entry| !skipped(entry, include_tests));
    builder
}

fn skipped(entry: &DirEntry, include_tests: bool) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_some_and(|kind| kind.is_dir()) {
        return false;
    }
    let name = entry.file_name().to_string_lossy();
    SKIPPED_DIRS.contains(&name.as_ref()) || (!include_tests && name == "tests")
}

/// Manifests the scan found, and why it stopped early if it did.
#[derive(Debug, Default)]
pub struct Scanned {
    /// Each manifest with its language, in walk order.
    pub manifests: Vec<(Language, PathBuf)>,
    pub warnings: Vec<String>,
}

/// Every package manifest under `root`.
pub fn manifests(root: &Path, include_tests: bool) -> Scanned {
    let mut found = Scanned::default();
    let mut files = 0usize;
    for entry in walker(root, include_tests).build().flatten() {
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        files += 1;
        if files > MAX_FILES {
            found.warnings.push(format!(
                "stopped looking for packages after {MAX_FILES} files; narrow the audit with `anc audit <dir>`"
            ));
            break;
        }
        if let Some(language) = Language::of_manifest(entry.file_name()) {
            found.manifests.push((language, entry.into_path()));
        }
    }
    found
}
