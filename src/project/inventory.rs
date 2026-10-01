//! The package inventory: every package under an audit root, from its
//! workspace declarations and the tree scan.

use std::fs;
use std::path::{Path, PathBuf};

use super::bins::{self, Candidate};
use super::{Language, scan, workspace};

/// One package in the audited tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// The directory holding the manifest.
    pub root: PathBuf,
    pub language: Language,
    pub manifest: PathBuf,
}

/// Every package under an audit root, and what discovery could not read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Inventory {
    /// The root's own packages, then declared workspace members in
    /// declaration order, then the rest of the scan in walk order. A
    /// directory holding two manifests is two packages.
    pub packages: Vec<Package>,
    /// The binaries those packages declare and have built.
    pub candidates: Vec<Candidate>,
    /// One message per declaration, member, or scan limit to report.
    pub warnings: Vec<String>,
}

/// Inventory the packages under `root`.
pub fn inventory(root: &Path, include_tests: bool) -> Inventory {
    let declared = workspace::read(root);
    let scanned = scan::manifests(root, include_tests);
    let mut found = Inventory {
        warnings: declared.warnings,
        ..Inventory::default()
    };
    let (own, rest): (Vec<_>, Vec<_>) = scanned
        .manifests
        .into_iter()
        .partition(|(_, manifest)| manifest.parent() == Some(root));
    for (language, manifest) in own {
        add(&mut found, language, manifest);
    }
    for (language, dir) in declared.members {
        add(&mut found, language, dir.join(language.manifest_name()));
    }
    for (language, manifest) in rest {
        add(&mut found, language, manifest);
    }
    found.warnings.extend(scanned.warnings);
    found.candidates = bins::candidates(root, &found.packages);
    found
}

fn add(found: &mut Inventory, language: Language, manifest: PathBuf) {
    let Some(root) = manifest.parent().map(Path::to_path_buf) else {
        return;
    };
    let listed = found
        .packages
        .iter()
        .any(|pkg| pkg.language == language && pkg.root == root);
    if listed || (language == Language::Rust && !declares_package(&manifest)) {
        return;
    }
    found.packages.push(Package {
        root,
        language,
        manifest,
    });
}

/// A Cargo manifest without `[package]` is a virtual workspace root, which
/// builds nothing itself.
fn declares_package(manifest: &Path) -> bool {
    fs::read_to_string(manifest)
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .is_some_and(|doc| doc.contains_key("package"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// Packages as `(language, root relative to the audit root)`.
    fn listed(root: &Path, include_tests: bool) -> Vec<(Language, String)> {
        let found = inventory(root, include_tests);
        assert!(found.warnings.is_empty(), "{:?}", found.warnings);
        found
            .packages
            .iter()
            .map(|pkg| {
                assert_eq!(pkg.manifest.parent(), Some(pkg.root.as_path()), "{pkg:?}");
                let rel = pkg.root.strip_prefix(root).expect("package under root");
                let rel = if rel.as_os_str().is_empty() {
                    ".".to_string()
                } else {
                    rel.to_string_lossy().replace('\\', "/")
                };
                (pkg.language, rel)
            })
            .collect()
    }

    /// A temp directory that git treats as a repository root, so its
    /// `.gitignore` applies.
    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(tmp.path().join(".git")).expect("mkdir .git");
        tmp
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, body).expect("write");
    }

    const CRATE: &str = "[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";
    const PROJECT: &str = "[project]\nname = \"x\"\nversion = \"0.1.0\"\n";

    #[test]
    fn mixed_repo_without_a_root_manifest_lists_both_packages() {
        assert_eq!(
            listed(&fixture("mixed-no-root"), false),
            [
                (Language::Rust, "cli".to_string()),
                (Language::Python, "py".to_string()),
            ]
        );
    }

    #[test]
    fn one_directory_with_two_manifests_is_two_packages() {
        let tmp = repo();
        write(tmp.path(), "bridge/Cargo.toml", CRATE);
        write(tmp.path(), "bridge/pyproject.toml", PROJECT);

        assert_eq!(
            listed(tmp.path(), false),
            [
                (Language::Rust, "bridge".to_string()),
                (Language::Python, "bridge".to_string()),
            ]
        );
    }

    #[test]
    fn manifests_in_dependency_build_and_ignored_trees_are_not_packages() {
        let tmp = repo();
        write(tmp.path(), "package.json", "{\"name\": \"app\"}\n");
        write(
            tmp.path(),
            "node_modules/dep/package.json",
            "{\"name\": \"dep\"}\n",
        );
        write(tmp.path(), "build/lib/pyproject.toml", PROJECT);
        write(tmp.path(), ".hidden/Cargo.toml", CRATE);
        write(tmp.path(), ".gitignore", "generated/\n");
        write(tmp.path(), "generated/pyproject.toml", PROJECT);

        assert_eq!(
            listed(tmp.path(), false),
            [(Language::Node, ".".to_string())]
        );
    }

    #[test]
    fn manifests_under_tests_count_only_with_include_tests() {
        let tmp = repo();
        write(tmp.path(), "Cargo.toml", CRATE);
        write(tmp.path(), "tests/fixtures/demo/Cargo.toml", CRATE);

        assert_eq!(
            listed(tmp.path(), false),
            [(Language::Rust, ".".to_string())]
        );
        assert_eq!(
            listed(tmp.path(), true),
            [
                (Language::Rust, ".".to_string()),
                (Language::Rust, "tests/fixtures/demo".to_string()),
            ]
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_symlinked_directory_is_not_followed() {
        let outside = tempfile::tempdir().expect("tempdir");
        write(outside.path(), "pkg/pyproject.toml", PROJECT);
        let tmp = repo();
        write(tmp.path(), "Cargo.toml", CRATE);
        std::os::unix::fs::symlink(outside.path().join("pkg"), tmp.path().join("linked"))
            .expect("symlink");

        assert_eq!(
            listed(tmp.path(), false),
            [(Language::Rust, ".".to_string())]
        );
    }

    #[test]
    fn a_declared_member_the_scan_also_finds_is_listed_once() {
        assert_eq!(
            listed(&fixture("workspaces/cargo-virtual"), false),
            [
                (Language::Rust, "crates/cli".to_string()),
                (Language::Rust, "crates/core".to_string()),
                (Language::Rust, "crates/skipped".to_string()),
            ]
        );
    }

    #[test]
    fn declaration_warnings_reach_the_inventory() {
        let tmp = repo();
        write(tmp.path(), "package.json", "{\"name\": \"app\"}\n");
        write(
            tmp.path(),
            "pnpm-workspace.yaml",
            "packages: &all\n  - 'pkgs/*'\n",
        );

        let found = inventory(tmp.path(), false);

        assert_eq!(found.warnings.len(), 1, "{:?}", found.warnings);
        assert!(
            found.warnings[0].contains("pnpm-workspace.yaml"),
            "{:?}",
            found.warnings
        );
    }

    #[test]
    fn this_repository_is_one_package() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let found = inventory(root, false);
        assert_eq!(
            found.packages,
            [Package {
                root: root.to_path_buf(),
                language: Language::Rust,
                manifest: root.join("Cargo.toml"),
            }],
            "{:?}",
            found.warnings
        );
    }
}
