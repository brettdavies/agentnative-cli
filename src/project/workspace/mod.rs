//! Workspace declarations at the audit root: the member packages that a
//! Cargo, npm or yarn, pnpm, go.work, or uv workspace file names.

use std::fs;
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};

use super::{Language, MAX_DEPTH};

mod syntax;

use syntax::{go_work_uses, pnpm_packages};

const PNPM_FILE: &str = "pnpm-workspace.yaml";

/// The packages a root's workspace files declare, and what could not be read.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Declared {
    /// Member package directories with the language their workspace family
    /// implies, in declaration order: Cargo, npm or yarn, pnpm, go.work, uv.
    pub members: Vec<(Language, PathBuf)>,
    /// One message per declaration or member anc could not use.
    pub warnings: Vec<String>,
}

/// Read every workspace declaration at `root`.
pub fn read(root: &Path) -> Declared {
    let mut out = Declared::default();
    if let Some(text) = read_file(root, "Cargo.toml", &mut out) {
        cargo(root, &text, &mut out);
    }
    if let Some(text) = read_file(root, "package.json", &mut out) {
        node(root, &text, &mut out);
    }
    if let Some(text) = read_file(root, PNPM_FILE, &mut out) {
        pnpm(root, &text, &mut out);
    }
    if let Some(text) = read_file(root, "go.work", &mut out) {
        go_work(root, &text, &mut out);
    }
    if let Some(text) = read_file(root, "pyproject.toml", &mut out) {
        uv(root, &text, &mut out);
    }
    out
}

/// The file's text; `None` when it is absent or unreadable, warning for the
/// latter.
fn read_file(root: &Path, name: &str, out: &mut Declared) -> Option<String> {
    match fs::read_to_string(root.join(name)) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            out.warnings.push(unreadable(name, &e.to_string()));
            None
        }
    }
}

fn unreadable(file: &str, reason: &str) -> String {
    format!(
        "could not read the workspace in {file} ({reason}); anc finds its packages by scanning instead"
    )
}

fn cargo(root: &Path, text: &str, out: &mut Declared) {
    let doc: toml::Table = match text.parse() {
        Ok(doc) => doc,
        Err(e) => return out.warnings.push(unreadable("Cargo.toml", &e.to_string())),
    };
    let Some(workspace) = doc.get("workspace").and_then(toml::Value::as_table) else {
        return;
    };
    if doc.contains_key("package") {
        push(out, Language::Rust, root.to_path_buf());
    }
    let members = toml_strings(workspace.get("members"));
    let exclude = toml_strings(workspace.get("exclude"));
    expand(root, "Cargo.toml", &members, &exclude, Language::Rust, out);
}

fn node(root: &Path, text: &str, out: &mut Declared) {
    let doc: serde_json::Value = match serde_json::from_str(text) {
        Ok(doc) => doc,
        Err(e) => {
            return out
                .warnings
                .push(unreadable("package.json", &e.to_string()));
        }
    };
    let patterns = match &doc["workspaces"] {
        serde_json::Value::Array(items) => json_strings(items),
        serde_json::Value::Object(yarn) => match &yarn.get("packages") {
            Some(serde_json::Value::Array(items)) => json_strings(items),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    expand(root, "package.json", &patterns, &[], Language::Node, out);
}

fn pnpm(root: &Path, text: &str, out: &mut Declared) {
    match pnpm_packages(text) {
        Ok(patterns) => {
            let (exclude, include): (Vec<String>, Vec<String>) =
                patterns.into_iter().partition(|p| p.starts_with('!'));
            let exclude: Vec<String> = exclude.iter().map(|p| p[1..].to_string()).collect();
            expand(root, PNPM_FILE, &include, &exclude, Language::Node, out);
        }
        Err(reason) => out.warnings.push(unreadable(PNPM_FILE, &reason)),
    }
}

fn go_work(root: &Path, text: &str, out: &mut Declared) {
    expand(root, "go.work", &go_work_uses(text), &[], Language::Go, out);
}

fn uv(root: &Path, text: &str, out: &mut Declared) {
    let doc: toml::Table = match text.parse() {
        Ok(doc) => doc,
        Err(e) => {
            return out
                .warnings
                .push(unreadable("pyproject.toml", &e.to_string()));
        }
    };
    let workspace = doc
        .get("tool")
        .and_then(|tool| tool.get("uv"))
        .and_then(|uv| uv.get("workspace"))
        .and_then(toml::Value::as_table);
    let Some(workspace) = workspace else {
        return;
    };
    if doc.contains_key("project") {
        push(out, Language::Python, root.to_path_buf());
    }
    let members = toml_strings(workspace.get("members"));
    let exclude = toml_strings(workspace.get("exclude"));
    expand(
        root,
        "pyproject.toml",
        &members,
        &exclude,
        Language::Python,
        out,
    );
}

fn toml_strings(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn json_strings(items: &[serde_json::Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.as_str().map(str::to_string))
        .collect()
}

/// The manifest that makes a directory a package of `lang`.
fn manifest(lang: Language) -> &'static str {
    match lang {
        Language::Rust => "Cargo.toml",
        Language::Python => "pyproject.toml",
        Language::Go => "go.mod",
        Language::Node => "package.json",
    }
}

/// Add the member directories `include` names under `root`, minus those
/// `exclude` names. A glob keeps only the directories holding the family's
/// manifest; a literal path without one is skipped with a warning.
fn expand(
    root: &Path,
    source: &str,
    include: &[String],
    exclude: &[String],
    lang: Language,
    out: &mut Declared,
) {
    let manifest = manifest(lang);
    let excludes: Vec<Pattern> = exclude
        .iter()
        .filter_map(|p| Pattern::new(p, source, out))
        .collect();
    let excluded = |rel: &Path| excludes.iter().any(|ex| ex.covers(rel));
    let mut tree: Option<Vec<PathBuf>> = None;
    for raw in include {
        let Some(pattern) = Pattern::new(raw, source, out) else {
            continue;
        };
        match &pattern {
            Pattern::Glob(matcher) => {
                let dirs = tree.get_or_insert_with(|| subdirectories(root));
                for rel in dirs.iter() {
                    if matcher.is_match(rel)
                        && !excluded(rel)
                        && root.join(rel).join(manifest).is_file()
                    {
                        push(out, lang, root.join(rel));
                    }
                }
            }
            Pattern::Path(rel) => {
                if excluded(rel) {
                    continue;
                }
                if !root.join(rel).join(manifest).is_file() {
                    out.warnings.push(format!(
                        "{source} names workspace member `{raw}`, but it holds no {manifest}; anc skips it"
                    ));
                    continue;
                }
                push(out, lang, root.join(rel));
            }
        }
    }
}

/// One workspace member or exclusion entry, relative to the root.
enum Pattern {
    Path(PathBuf),
    Glob(GlobMatcher),
}

impl Pattern {
    fn new(raw: &str, source: &str, out: &mut Declared) -> Option<Self> {
        let rel: PathBuf = Path::new(raw)
            .components()
            .filter(|part| !matches!(part, Component::CurDir))
            .collect();
        let text = rel.to_string_lossy().replace('\\', "/");
        if !text.contains(['*', '?', '[', '{']) {
            return Some(Self::Path(rel));
        }
        match GlobBuilder::new(&text).literal_separator(true).build() {
            Ok(glob) => Some(Self::Glob(glob.compile_matcher())),
            Err(e) => {
                out.warnings.push(format!(
                    "{source} names workspace pattern `{raw}`, which is not a valid glob ({e}); anc skips it"
                ));
                None
            }
        }
    }

    /// Whether this exclusion covers `rel`: the same path, a path below it,
    /// or a glob match.
    fn covers(&self, rel: &Path) -> bool {
        match self {
            Self::Path(path) => rel.starts_with(path),
            Self::Glob(matcher) => matcher.is_match(rel),
        }
    }
}

/// Every directory under `root`, relative to it and sorted, skipping hidden
/// directories and the build and dependency trees no workspace member lives
/// in. Symlinked directories are not followed.
fn subdirectories(root: &Path) -> Vec<PathBuf> {
    fn walk(root: &Path, rel: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        if depth > MAX_DEPTH {
            return;
        }
        let Ok(entries) = fs::read_dir(root.join(rel)) else {
            return;
        };
        for entry in entries.flatten() {
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !is_dir || name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            let child = rel.join(&*name);
            found.push(child.clone());
            walk(root, &child, depth + 1, found);
        }
    }
    let mut found = Vec::new();
    walk(root, Path::new(""), 0, &mut found);
    found.sort();
    found
}

fn push(out: &mut Declared, lang: Language, dir: PathBuf) {
    if !out.members.iter().any(|(l, d)| *l == lang && *d == dir) {
        out.members.push((lang, dir));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/workspaces")
            .join(name)
    }

    /// Members relative to `root`, with `.` for the root itself.
    fn relative(root: &Path, declared: &Declared) -> Vec<(Language, String)> {
        declared
            .members
            .iter()
            .map(|(lang, dir)| {
                let rel = dir.strip_prefix(root).expect("member under root");
                let rel = if rel.as_os_str().is_empty() {
                    ".".to_string()
                } else {
                    rel.to_string_lossy().replace('\\', "/")
                };
                (*lang, rel)
            })
            .collect()
    }

    fn members(name: &str) -> Vec<(Language, String)> {
        let root = fixture(name);
        let declared = read(&root);
        assert!(declared.warnings.is_empty(), "{:?}", declared.warnings);
        relative(&root, &declared)
    }

    fn rust(rel: &str) -> (Language, String) {
        (Language::Rust, rel.to_string())
    }

    fn node(rel: &str) -> (Language, String) {
        (Language::Node, rel.to_string())
    }

    #[test]
    fn cargo_virtual_workspace_expands_members_and_drops_the_excluded_one() {
        assert_eq!(
            members("cargo-virtual"),
            [rust("crates/cli"), rust("crates/core")]
        );
    }

    #[test]
    fn cargo_root_package_lists_itself_and_its_members() {
        assert_eq!(
            members("cargo-root-package"),
            [rust("."), rust("crates/helper")]
        );
    }

    #[test]
    fn npm_array_and_yarn_object_name_the_same_members() {
        let expected = [node("packages/a"), node("packages/b")];
        assert_eq!(members("npm"), expected);
        assert_eq!(members("yarn"), expected);
    }

    #[test]
    fn pnpm_block_and_flow_lists_with_an_exclusion_name_the_same_members() {
        let root = fixture("pnpm");
        let expected = [node("packages/a"), node("packages/b")];
        assert_eq!(members("pnpm"), expected);

        let mut flow = Declared::default();
        pnpm(
            &root,
            "packages: ['packages/*', \"!packages/skipped\"]\n",
            &mut flow,
        );
        assert!(flow.warnings.is_empty(), "{:?}", flow.warnings);
        assert_eq!(relative(&root, &flow), expected);
    }

    #[test]
    fn pnpm_yaml_beyond_the_packages_sequence_warns_and_lists_nothing() {
        let root = fixture("pnpm");
        for text in [
            "packages: &pkgs\n  - 'packages/*'\n",
            "packages:\n  - path: packages/*\n",
            "packages:\n  - !packages/skipped\n",
        ] {
            let mut out = Declared::default();
            pnpm(&root, text, &mut out);
            assert!(out.members.is_empty(), "{text:?} gave {:?}", out.members);
            assert_eq!(out.warnings.len(), 1, "{text:?} gave {:?}", out.warnings);
            assert!(
                out.warnings[0].contains("pnpm-workspace.yaml"),
                "{:?}",
                out.warnings
            );
        }
    }

    #[test]
    fn go_work_single_line_and_block_use_name_the_same_members() {
        let root = fixture("go-work");
        let expected = [
            (Language::Go, "cli".to_string()),
            (Language::Go, "lib".to_string()),
        ];
        assert_eq!(members("go-work"), expected);

        let mut single = Declared::default();
        go_work(
            &root,
            "go 1.22\n\nuse ./cli // the CLI\nuse \"./lib\"\n",
            &mut single,
        );
        assert!(single.warnings.is_empty(), "{:?}", single.warnings);
        assert_eq!(relative(&root, &single), expected);
    }

    #[test]
    fn uv_members_glob_drops_the_excluded_one() {
        assert_eq!(
            members("uv"),
            [
                (Language::Python, ".".to_string()),
                (Language::Python, "packages/a".to_string()),
                (Language::Python, "packages/b".to_string()),
            ]
        );
    }

    #[test]
    fn declared_member_that_does_not_exist_is_skipped_with_a_warning() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/missing\"]\n",
        )
        .expect("write Cargo.toml");

        let declared = read(root);

        assert!(declared.members.is_empty(), "{:?}", declared.members);
        assert_eq!(declared.warnings.len(), 1, "{:?}", declared.warnings);
        assert!(
            declared.warnings[0].contains("crates/missing"),
            "{:?}",
            declared.warnings
        );
    }

    #[test]
    fn a_root_without_workspace_files_declares_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert_eq!(read(tmp.path()), Declared::default());
    }
}
