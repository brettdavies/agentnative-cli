//! Candidate binaries: the bins each inventoried package declares that exist
//! on disk at their language's build location. Nothing is found by listing a
//! directory.

use std::env::consts::EXE_SUFFIX;
use std::fs;
use std::path::{Component, Path, PathBuf};

use super::cargo_target::{CargoTarget, TargetDirs};
use super::inventory::Package;
use super::{Language, MAX_DEPTH, is_executable};

/// A built binary that a package declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The bin name the package declares.
    pub name: String,
    /// The declaring package's own name.
    pub package: String,
    pub language: Language,
    /// Where the built binary is.
    pub path: PathBuf,
    /// The declaring package's manifest.
    pub manifest: PathBuf,
}

/// What the packages under an audit root declare.
#[derive(Debug, Default)]
pub struct Bins {
    /// Declared bins that are built, in package order and, within a
    /// package, in declaration order (Python scripts by name).
    pub candidates: Vec<Candidate>,
    /// Names of declared bins with no build output, in the same order.
    pub unbuilt: Vec<String>,
    /// The cargo target directories searched for the unbuilt Rust bins.
    pub target_dirs: Vec<PathBuf>,
}

/// Every bin the packages under `root` declare, with Rust bins looked for
/// where `cargo` says it builds them. Each package's `name` and `bins` are
/// filled in along the way.
pub fn candidates(root: &Path, packages: &mut [Package], cargo: &CargoTarget) -> Bins {
    let targets = TargetDirs::new(cargo, root);
    let mut found = Bins::default();
    for pkg in packages.iter_mut() {
        let manifest = fs::read_to_string(&pkg.manifest).unwrap_or_default();
        let declared = match pkg.language {
            Language::Rust => rust(&targets, pkg, &manifest),
            Language::Node => node(root, pkg, &manifest),
            Language::Python => python(root, pkg, &manifest),
            Language::Go => go(pkg, &manifest),
        };
        let Some((package, bins)) = declared else {
            pkg.name = dir_name(&pkg.root);
            continue;
        };
        let package = if package.is_empty() {
            dir_name(&pkg.root)
        } else {
            package
        };
        pkg.name = package.clone();
        pkg.bins = bins.iter().map(|(name, _)| name.clone()).collect();
        for (name, path) in bins {
            match path {
                Some(path) => found.candidates.push(Candidate {
                    name,
                    package: package.clone(),
                    language: pkg.language,
                    path,
                    manifest: pkg.manifest.clone(),
                }),
                None => {
                    if pkg.language == Language::Rust {
                        for dir in targets.of(&pkg.root) {
                            if !found.target_dirs.contains(&dir) {
                                found.target_dirs.push(dir);
                            }
                        }
                    }
                    if !found.unbuilt.contains(&name) {
                        found.unbuilt.push(name);
                    }
                }
            }
        }
    }
    found
}

/// A package's name and each bin it declares, with where that bin is built
/// when it exists.
type Declared = Option<(String, Vec<(String, Option<PathBuf>)>)>;

/// `[[bin]]` targets, then the implicit `src/main.rs` (named for the
/// package) and `src/bin/<name>.rs` or `src/bin/<name>/main.rs` that no
/// explicit target claims, unless `autobins = false`. Each is built at
/// `release` or `debug` in the package's target directories
/// ([`TargetDirs::of`]), the newer of the two when both exist.
fn rust(targets: &TargetDirs, pkg: &Package, manifest: &str) -> Declared {
    let doc: toml::Table = manifest.parse().ok()?;
    let package = doc.get("package")?.as_table()?;
    let package_name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    let mut claimed: Vec<PathBuf> = Vec::new();
    for bin in doc
        .get("bin")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = bin.get("name").and_then(toml::Value::as_str) else {
            continue;
        };
        let path = match bin.get("path").and_then(toml::Value::as_str) {
            Some(path) => PathBuf::from(path),
            None if name == package_name => PathBuf::from("src/main.rs"),
            None => PathBuf::from(format!("src/bin/{name}.rs")),
        };
        claimed.push(normalized(&path));
        names.push(name.to_string());
    }
    if package.get("autobins").and_then(toml::Value::as_bool) != Some(false) {
        for (path, name) in implicit_rust_bins(&pkg.root, package_name) {
            if !claimed.contains(&path) && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let dirs = if names.is_empty() {
        Vec::new()
    } else {
        targets.of(&pkg.root)
    };
    let bins = names
        .into_iter()
        .map(|name| {
            let built = rust_artifact(&dirs, &name);
            (name, built)
        })
        .collect();
    Some((package_name.to_string(), bins))
}

fn implicit_rust_bins(dir: &Path, package_name: &str) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    if dir.join("src/main.rs").is_file() && !package_name.is_empty() {
        found.push((PathBuf::from("src/main.rs"), package_name.to_string()));
    }
    let mut extra = Vec::new();
    for entry in fs::read_dir(dir.join("src/bin"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if path.is_file()
            && let Some(stem) = file_name.strip_suffix(".rs")
        {
            extra.push((PathBuf::from("src/bin").join(&file_name), stem.to_string()));
        } else if path.join("main.rs").is_file() {
            extra.push((
                PathBuf::from("src/bin").join(&file_name).join("main.rs"),
                file_name,
            ));
        }
    }
    extra.sort();
    found.extend(extra);
    found
}

fn rust_artifact(target_dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    let file = format!("{name}{EXE_SUFFIX}");
    target_dirs.iter().find_map(|dir| {
        let release = dir.join("release").join(&file);
        let debug = dir.join("debug").join(&file);
        match (release.is_file(), debug.is_file()) {
            (true, true) => Some(pick_newer_artifact(&release, &debug)),
            (true, false) => Some(release),
            (false, true) => Some(debug),
            (false, false) => None,
        }
    })
}

/// Return the path with the newer mtime. Ties and missing-mtime fall back
/// to `b` (called with the debug path) — matches cargo's dev-flow default
/// where debug is the canonical fresh artifact. Documented at
/// docs/solutions/test-failures/stale-release-binary-dogfood-fail-2026-05-07.md.
fn pick_newer_artifact(a: &Path, b: &Path) -> PathBuf {
    let a_m = fs::metadata(a).and_then(|m| m.modified()).ok();
    let b_m = fs::metadata(b).and_then(|m| m.modified()).ok();
    match (a_m, b_m) {
        (Some(am), Some(bm)) if am > bm => a.to_path_buf(),
        _ => b.to_path_buf(),
    }
}

/// A `bin` string names the package without its scope; a `bin` object names
/// its keys. Each is the declared file when it is executable, else the
/// entry point linked into `node_modules/.bin` in the package or at the
/// audit root.
fn node(root: &Path, pkg: &Package, manifest: &str) -> Declared {
    let doc: serde_json::Value = serde_json::from_str(manifest).ok()?;
    let package_name = doc["name"].as_str().unwrap_or_default().to_string();
    let declared: Vec<(String, String)> = match &doc["bin"] {
        serde_json::Value::String(file) => {
            let unscoped = package_name.rsplit('/').next().unwrap_or_default();
            vec![(unscoped.to_string(), file.clone())]
        }
        serde_json::Value::Object(map) => map
            .iter()
            .filter_map(|(name, file)| file.as_str().map(|f| (name.clone(), f.to_string())))
            .collect(),
        _ => Vec::new(),
    };
    let bins = declared
        .into_iter()
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, file)| {
            let own = pkg.root.join(file);
            let built = if executable(&own) {
                Some(own)
            } else {
                bases(&pkg.root, root)
                    .into_iter()
                    .map(|base| base.join("node_modules/.bin").join(&name))
                    .find(|path| path.is_file())
            };
            (name, built)
        })
        .collect();
    Some((package_name, bins))
}

/// `[project.scripts]` keys, built as the virtualenv's console scripts in
/// the package or at the audit root.
fn python(root: &Path, pkg: &Package, manifest: &str) -> Declared {
    let doc: toml::Table = manifest.parse().ok()?;
    let project = doc.get("project")?;
    let package_name = project
        .get("name")
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let scripts = project.get("scripts").and_then(toml::Value::as_table);
    let venv_bin = if cfg!(windows) {
        ".venv/Scripts"
    } else {
        ".venv/bin"
    };
    let bins = scripts
        .into_iter()
        .flat_map(|table| table.keys())
        .map(|name| {
            let file = format!("{name}{EXE_SUFFIX}");
            let built = bases(&pkg.root, root)
                .into_iter()
                .map(|base| base.join(venv_bin).join(&file))
                .find(|path| path.is_file());
            (name.clone(), built)
        })
        .collect();
    Some((package_name, bins))
}

/// Each directory of the module holding `package main` names a bin after
/// itself, built in that directory or at the module root.
fn go(pkg: &Package, manifest: &str) -> Declared {
    let module = manifest
        .lines()
        .find_map(|line| line.trim().strip_prefix("module "))
        .map(|module| module.trim().trim_matches('"').to_string())
        .unwrap_or_default();
    let bins = main_package_dirs(&pkg.root)
        .into_iter()
        .map(|dir| {
            let name = dir_name(&dir);
            let file = format!("{name}{EXE_SUFFIX}");
            let built = [dir.as_path(), pkg.root.as_path()]
                .into_iter()
                .map(|base| base.join(&file))
                .find(|path| path.is_file());
            (name, built)
        })
        .collect();
    Some((module, bins))
}

/// Directories under a Go module holding a non-test file in `package main`,
/// sorted. Nested modules, vendored code, test data, and hidden directories
/// are not part of this module's commands.
fn main_package_dirs(module_root: &Path) -> Vec<PathBuf> {
    fn visit(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        if depth > MAX_DEPTH {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut is_main = false;
        let mut children = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                let skipped = name.starts_with('.')
                    || name == "vendor"
                    || name == "testdata"
                    || path.join("go.mod").is_file();
                if !skipped {
                    children.push(path);
                }
            } else if name.ends_with(".go") && !name.ends_with("_test.go") && declares_main(&path) {
                is_main = true;
            }
        }
        if is_main {
            found.push(dir.to_path_buf());
        }
        for child in children {
            visit(&child, depth + 1, found);
        }
    }
    let mut found = Vec::new();
    visit(module_root, 0, &mut found);
    found.sort();
    found
}

fn declares_main(file: &Path) -> bool {
    fs::read_to_string(file).is_ok_and(|text| {
        text.lines()
            .map(|line| line.split("//").next().unwrap_or_default().trim())
            .find(|line| line.starts_with("package "))
            == Some("package main")
    })
}

/// The package directory, then the audit root when it differs: the places a
/// workspace member's build output can be.
fn bases<'a>(package_root: &'a Path, root: &'a Path) -> Vec<&'a Path> {
    if package_root == root {
        vec![package_root]
    } else {
        vec![package_root, root]
    }
}

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && is_executable(&meta))
}

fn normalized(path: &Path) -> PathBuf {
    path.components()
        .filter(|part| !matches!(part, Component::CurDir))
        .collect()
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::inventory::inventory;
    use std::fs;

    /// A copy of `tests/fixtures/<name>` in a fresh temp directory, so a test
    /// can add build output without touching the committed fixture.
    fn staged(name: &str) -> tempfile::TempDir {
        fn copy(from: &Path, to: &Path) {
            fs::create_dir_all(to).expect("mkdir");
            for entry in fs::read_dir(from).expect("read fixture") {
                let entry = entry.expect("fixture entry");
                let target = to.join(entry.file_name());
                if entry.file_type().expect("file type").is_dir() {
                    copy(&entry.path(), &target);
                } else {
                    fs::copy(entry.path(), &target).expect("copy fixture file");
                }
            }
        }
        let tmp = tempfile::tempdir().expect("tempdir");
        copy(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
            tmp.path(),
        );
        tmp
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, body).expect("write");
    }

    /// Write an executable stand-in for a built binary.
    fn built(root: &Path, rel: &str) {
        write(root, rel, "#!/bin/sh\necho built\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.join(rel), fs::Permissions::from_mode(0o755)).expect("chmod");
        }
    }

    /// The candidates under `root`, with Rust bins built in `root/target`.
    fn found(root: &Path) -> Vec<Candidate> {
        inventory(root, false, &CargoTarget::at(root.join("target"))).candidates
    }

    fn names(root: &Path) -> Vec<String> {
        found(root).into_iter().map(|c| c.name).collect()
    }

    const EXE: &str = std::env::consts::EXE_SUFFIX;

    #[test]
    fn rust_bins_come_from_bin_tables_main_rs_and_src_bin() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        write(
            root,
            "Cargo.toml",
            "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
             [[bin]]\nname = \"explicit\"\npath = \"src/explicit.rs\"\n",
        );
        for src in [
            "src/explicit.rs",
            "src/main.rs",
            "src/bin/extra.rs",
            "src/bin/nested/main.rs",
        ] {
            write(root, src, "fn main() {}\n");
        }
        for name in ["explicit", "pkg", "extra", "nested"] {
            built(root, &format!("target/debug/{name}{EXE}"));
        }

        assert_eq!(names(root), ["explicit", "pkg", "extra", "nested"]);
    }

    #[test]
    fn autobins_false_drops_the_implicit_bins() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        write(
            root,
            "Cargo.toml",
            "[package]\nname = \"pkg\"\nversion = \"0.1.0\"\nedition = \"2024\"\nautobins = false\n\n\
             [[bin]]\nname = \"explicit\"\npath = \"src/explicit.rs\"\n",
        );
        for src in ["src/explicit.rs", "src/main.rs", "src/bin/extra.rs"] {
            write(root, src, "fn main() {}\n");
        }
        for name in ["explicit", "pkg", "extra"] {
            built(root, &format!("target/debug/{name}{EXE}"));
        }

        assert_eq!(names(root), ["explicit"]);
    }

    #[test]
    fn a_member_crate_bin_is_found_in_the_workspace_root_target() {
        let tmp = staged("workspaces/cargo-virtual");
        built(tmp.path(), &format!("target/release/cli{EXE}"));

        let found = found(tmp.path());

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].name, "cli");
        assert_eq!(found[0].package, "cli");
        assert_eq!(found[0].language, Language::Rust);
        assert_eq!(
            found[0].path,
            tmp.path().join(format!("target/release/cli{EXE}"))
        );
    }

    #[test]
    #[cfg(unix)]
    fn node_bin_object_names_its_keys_and_never_a_dependency_tool() {
        let tmp = staged("bins/node-bin-object");
        built(tmp.path(), "node_modules/.bin/tsc");

        let found = found(tmp.path());

        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].name, "mytool");
        assert_eq!(found[0].package, "mytool-pkg");
        assert_eq!(found[0].path, tmp.path().join("bin/cli.js"));
    }

    #[test]
    #[cfg(unix)]
    fn node_bin_string_on_a_scoped_package_drops_the_scope() {
        let tmp = staged("bins/node-bin-scoped");

        assert_eq!(names(tmp.path()), ["tool"]);
    }

    #[test]
    #[cfg(unix)]
    fn node_bin_falls_back_to_the_linked_entry_point() {
        let tmp = staged("bins/node-bin-object");
        let cli = tmp.path().join("bin/cli.js");
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(names(tmp.path()).is_empty());

        built(tmp.path(), "node_modules/.bin/mytool");

        assert_eq!(names(tmp.path()), ["mytool"]);
    }

    #[test]
    fn python_scripts_count_once_their_venv_entry_point_exists() {
        let tmp = staged("bins/python-scripts");
        assert!(names(tmp.path()).is_empty());

        let venv_bin = if cfg!(windows) {
            ".venv/Scripts"
        } else {
            ".venv/bin"
        };
        built(tmp.path(), &format!("{venv_bin}/pytool{EXE}"));

        assert_eq!(names(tmp.path()), ["pytool"]);
    }

    #[test]
    fn go_main_package_counts_once_built_beside_its_source() {
        let tmp = staged("bins/go-main");
        assert!(names(tmp.path()).is_empty());

        built(tmp.path(), &format!("cmd/tool/tool{EXE}"));

        let found = found(tmp.path());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].name, "tool");
        assert_eq!(found[0].package, "example.com/gomain");
    }

    #[test]
    fn a_library_only_package_declares_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        write(
            root,
            "Cargo.toml",
            "[package]\nname = \"lib-only\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        write(root, "src/lib.rs", "");
        built(root, &format!("target/debug/lib-only{EXE}"));

        assert!(names(root).is_empty());
    }

    #[test]
    fn a_declared_but_unbuilt_bin_is_not_a_candidate() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        write(
            root,
            "Cargo.toml",
            "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        write(root, "src/main.rs", "fn main() {}\n");

        assert!(names(root).is_empty());
    }
}
