//! Which `.anc.toml` files apply to an audit, in precedence order.
//!
//! The chain is a function of paths alone: the directory the audit starts
//! from, the user-level file, and an optional explicit root. Whether each
//! candidate exists is the loader's concern.

use std::path::{Path, PathBuf};

use super::ANC_TOML_FILENAME;

/// The ordered `.anc.toml` candidates for one audit.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Chain {
    /// The user-level file, lowest precedence, when one is configured.
    pub home: Option<PathBuf>,
    /// The repository the layers belong to: the git root found by the walk,
    /// or the explicit root.
    pub repo_root: Option<PathBuf>,
    /// Files above the home layer, root-first, ending at the start directory.
    pub layers: Vec<PathBuf>,
}

impl Chain {
    /// Every candidate, lowest precedence first.
    pub fn candidates(&self) -> impl Iterator<Item = &Path> {
        self.home.iter().chain(&self.layers).map(PathBuf::as_path)
    }
}

/// Build the chain for an audit starting in `start`.
///
/// `home_file` is the user-level file; its directory bounds the walk for a
/// start outside any repository. `explicit_root` replaces the walk with that
/// directory's file alone.
pub fn resolve(start: &Path, home_file: Option<&Path>, explicit_root: Option<&Path>) -> Chain {
    let home = home_file.map(canonical_file);
    let (repo_root, dirs) = match explicit_root {
        Some(root) => {
            let root = canonical(root);
            (Some(root.clone()), vec![root])
        }
        None => walk(&canonical(start), home.as_deref().and_then(Path::parent)),
    };
    let layers = dirs
        .into_iter()
        .map(|dir| dir.join(ANC_TOML_FILENAME))
        .filter(|file| Some(file) != home.as_ref())
        .collect();
    Chain {
        home,
        repo_root,
        layers,
    }
}

/// The directories whose files apply above the home layer, root-first, and
/// the git root when the walk found one.
fn walk(start: &Path, home_dir: Option<&Path>) -> (Option<PathBuf>, Vec<PathBuf>) {
    if let Some(root) = start.ancestors().find(|dir| dir.join(".git").exists()) {
        return (Some(root.to_path_buf()), down_from(root, start));
    }
    match home_dir {
        Some(home_dir) if start.starts_with(home_dir) => (None, down_from(home_dir, start)),
        _ => (None, vec![start.to_path_buf()]),
    }
}

/// `top` and each directory below it on the way to `start`, `top` first.
fn down_from(top: &Path, start: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = start
        .ancestors()
        .take_while(|dir| dir.starts_with(top))
        .map(Path::to_path_buf)
        .collect();
    dirs.reverse();
    dirs
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// A file that may not exist yet, canonicalized through its directory.
fn canonical_file(file: &Path) -> PathBuf {
    match (file.parent(), file.file_name()) {
        (Some(dir), Some(name)) if !dir.as_os_str().is_empty() => canonical(dir).join(name),
        _ => file.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anc_toml::ANC_TOML_FILENAME;
    use std::fs;

    struct Fixture {
        _tmp: tempfile::TempDir,
        base: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = tempfile::tempdir().expect("tempdir");
            let base = fs::canonicalize(tmp.path()).expect("canonical tempdir");
            Self { _tmp: tmp, base }
        }

        fn dir(&self, rel: &str) -> PathBuf {
            let dir = self.base.join(rel);
            fs::create_dir_all(&dir).expect("create dir");
            dir
        }

        fn git_dir(&self, rel: &str) -> PathBuf {
            let root = self.dir(rel);
            fs::create_dir_all(root.join(".git")).expect("create .git dir");
            root
        }

        fn git_file(&self, rel: &str) -> PathBuf {
            let root = self.dir(rel);
            fs::write(root.join(".git"), "gitdir: elsewhere\n").expect("write .git file");
            root
        }

        fn home_file(&self) -> PathBuf {
            self.dir("home").join(ANC_TOML_FILENAME)
        }
    }

    fn files(dirs: &[&PathBuf]) -> Vec<PathBuf> {
        dirs.iter().map(|d| d.join(ANC_TOML_FILENAME)).collect()
    }

    #[test]
    fn repo_walk_runs_root_first_down_to_the_start() {
        let fx = Fixture::new();
        let root = fx.git_dir("repo");
        let a = fx.dir("repo/a");
        let b = fx.dir("repo/a/b");
        let start = fx.dir("repo/a/b/c");
        let home = fx.home_file();

        let chain = resolve(&start, Some(&home), None);

        let mut expected = vec![home.clone()];
        expected.extend(files(&[&root, &a, &b, &start]));
        assert_eq!(chain.candidates().collect::<Vec<_>>(), expected);
        assert_eq!(chain.repo_root.as_deref(), Some(root.as_path()));
    }

    #[test]
    fn worktree_inside_an_outer_repo_is_its_own_root() {
        let fx = Fixture::new();
        fx.git_dir("outer");
        let wt = fx.git_file("outer/wt");
        let start = fx.dir("outer/wt/src");

        let chain = resolve(&start, None, None);

        assert_eq!(chain.layers, files(&[&wt, &start]));
        assert_eq!(chain.repo_root.as_deref(), Some(wt.as_path()));
    }

    #[test]
    fn outside_any_repo_under_home_walks_from_home_down() {
        let fx = Fixture::new();
        let home = fx.home_file();
        let projects = fx.dir("home/projects");
        let start = fx.dir("home/projects/tool");

        let chain = resolve(&start, Some(&home), None);

        let mut expected = vec![home.clone()];
        expected.extend(files(&[&projects, &start]));
        assert_eq!(chain.candidates().collect::<Vec<_>>(), expected);
        assert_eq!(chain.repo_root, None);
    }

    #[test]
    fn outside_repo_and_home_reads_the_start_only() {
        let fx = Fixture::new();
        let home = fx.home_file();
        let start = fx.dir("elsewhere/tool");

        let chain = resolve(&start, Some(&home), None);

        let mut expected = vec![home.clone()];
        expected.extend(files(&[&start]));
        assert_eq!(chain.candidates().collect::<Vec<_>>(), expected);
        assert_eq!(chain.repo_root, None);
    }

    #[test]
    fn explicit_root_replaces_the_walk() {
        let fx = Fixture::new();
        fx.git_dir("repo");
        let start = fx.dir("repo/a");
        let fetched = fx.dir("fetched");
        let home = fx.home_file();

        let chain = resolve(&start, Some(&home), Some(&fetched));

        let mut expected = vec![home.clone()];
        expected.extend(files(&[&fetched]));
        assert_eq!(chain.candidates().collect::<Vec<_>>(), expected);
        assert_eq!(chain.repo_root.as_deref(), Some(fetched.as_path()));
    }

    #[test]
    fn no_home_file_means_no_home_entry() {
        let fx = Fixture::new();
        let root = fx.git_dir("repo");
        let start = fx.dir("repo/a");

        let chain = resolve(&start, None, None);

        assert_eq!(chain.home, None);
        assert_eq!(
            chain.candidates().collect::<Vec<_>>(),
            files(&[&root, &start])
        );
    }

    #[test]
    fn home_that_is_the_repo_root_appears_once() {
        let fx = Fixture::new();
        let home_dir = fx.git_dir("home");
        let home = home_dir.join(ANC_TOML_FILENAME);
        let start = fx.dir("home/tool");

        let chain = resolve(&start, Some(&home), None);

        let mut expected = vec![home.clone()];
        expected.extend(files(&[&start]));
        assert_eq!(chain.candidates().collect::<Vec<_>>(), expected);
    }
}
