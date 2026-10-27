//! Characterization snapshots of the flag parser over captured help text.
//!
//! `tests/fixtures/help/` holds captures of each help layout, and its
//! `index.json` lists every capture with where it came from. Each capture has
//! a snapshot in `snapshots/` with one line per definition the parser reads:
//! `{line:>4} | {names} | {description}`, the 1-based line in the capture, the
//! names as the help spells them, and the start of the description.
//!
//! `INSTA_UPDATE=always cargo test fixture_snapshots` rewrites the snapshots.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::HelpOutput;

const INDEX_FILE: &str = "index.json";

#[derive(Deserialize)]
struct Index {
    fixtures: Vec<Fixture>,
}

#[derive(Deserialize)]
struct Fixture {
    file: String,
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/help")
}

fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runner/help_probe/snapshots")
}

fn indexed_fixtures() -> Vec<String> {
    let path = fixtures_dir().join(INDEX_FILE);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let index: Index =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    index.fixtures.into_iter().map(|f| f.file).collect()
}

/// Names of the regular files directly inside `dir`.
fn file_names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry is readable"))
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect()
}

/// A capture's text, decoded the way `BinaryRunner` decodes a child's output.
fn read_capture(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The text of the capture named `file` in `tests/fixtures/help/`.
pub(crate) fn fixture(file: &str) -> String {
    read_capture(&fixtures_dir().join(file))
}

fn capture_stem(file: &str) -> &str {
    file.strip_suffix(".txt")
        .unwrap_or_else(|| panic!("capture {file} is not named *.txt"))
}

fn render_definitions(raw: &str) -> String {
    let mut out = String::new();
    for flag in HelpOutput::from_raw(raw).flags() {
        writeln!(out, "{}", flag.snapshot_line()).expect("writing to a String");
    }
    out
}

fn assert_no_orphans(left: (&str, &BTreeSet<String>), right: (&str, &BTreeSet<String>)) {
    let only_left: Vec<_> = left.1.difference(right.1).collect();
    let only_right: Vec<_> = right.1.difference(left.1).collect();
    assert!(
        only_left.is_empty() && only_right.is_empty(),
        "{} without {}: {only_left:?}; {} without {}: {only_right:?}",
        left.0,
        right.0,
        right.0,
        left.0,
    );
}

#[test]
fn every_fixture_matches_its_snapshot() {
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);
    for file in indexed_fixtures() {
        let path = fixtures_dir().join(&file);
        settings.set_input_file(&path);
        settings.bind(|| {
            insta::assert_snapshot!(
                capture_stem(&file),
                render_definitions(&read_capture(&path))
            );
        });
    }
}

#[test]
fn fixtures_index_and_snapshots_list_the_same_captures() {
    let indexed: BTreeSet<String> = indexed_fixtures().into_iter().collect();

    let mut files = file_names(&fixtures_dir());
    files.remove(INDEX_FILE);
    assert_no_orphans(("fixture file", &files), ("index entry", &indexed));

    let expected: BTreeSet<String> = indexed
        .iter()
        .map(|file| format!("{}.snap", capture_stem(file)))
        .collect();
    let snapshots: BTreeSet<String> = file_names(&snapshots_dir())
        .into_iter()
        .filter(|name| name.ends_with(".snap"))
        .collect();
    assert_no_orphans(("snapshot", &snapshots), ("indexed fixture", &expected));
}

fn env_dir(var: &str, purpose: &str) -> PathBuf {
    let value = std::env::var_os(var).unwrap_or_else(|| panic!("set {var} to {purpose}"));
    PathBuf::from(value)
}

/// Renders any directory of captures named `<binary>__<args joined by _>.txt`:
/// one `<capture stem>.defs` per capture, holding the lines a snapshot holds,
/// and `_subcommands.tsv`, a `binary<TAB>subcommand` line for each subcommand
/// read from a `<binary>__--help.txt` capture.
#[test]
#[ignore = "reads ANC_HELP_CAPTURE_DIR and writes ANC_HELP_RENDER_DIR"]
fn render_capture_directory() {
    let captures = env_dir("ANC_HELP_CAPTURE_DIR", "a directory of help captures");
    let out = env_dir("ANC_HELP_RENDER_DIR", "the directory to render into");
    fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{}: {e}", out.display()));

    let mut subcommands = String::new();
    for file in file_names(&captures) {
        let Some(stem) = file.strip_suffix(".txt") else {
            continue;
        };
        let raw = read_capture(&captures.join(&file));
        let defs = out.join(format!("{stem}.defs"));
        fs::write(&defs, render_definitions(&raw))
            .unwrap_or_else(|e| panic!("{}: {e}", defs.display()));

        if let Some(binary) = stem.strip_suffix("__--help") {
            let help = HelpOutput::from_raw_for_binary(raw, binary);
            for name in help.subcommands() {
                writeln!(subcommands, "{binary}\t{name}").expect("writing to a String");
            }
        }
    }
    let listing = out.join("_subcommands.tsv");
    fs::write(&listing, subcommands).unwrap_or_else(|e| panic!("{}: {e}", listing.display()));
}
