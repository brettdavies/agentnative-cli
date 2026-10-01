//! Every helper that spawns `anc` in a test file that runs `anc audit`
//! points `AGENTNATIVE_HOME_CONFIG` at a file the tests own. Every audit
//! applies the user-level `~/.anc.toml`, so a spawn without it reads the
//! developer's own file and passes or fails by machine.

use std::fs;
use std::path::Path;

const SPAWNS: [&str; 2] = [
    r#"Command::cargo_bin("anc")"#,
    r#"Command::new(env!("CARGO_BIN_EXE_anc"))"#,
];

/// Line numbers of the `anc` spawns in `source` whose enclosing top-level
/// item never sets `AGENTNATIVE_HOME_CONFIG`.
fn unguarded_spawns(source: &str) -> Vec<usize> {
    if !source.contains(r#""audit""#) {
        return Vec::new();
    }
    let mut lines: Vec<usize> = SPAWNS
        .iter()
        .flat_map(|spawn| source.match_indices(spawn))
        .filter(|&(at, _)| {
            let item_end = source[at..]
                .find("\n}")
                .map_or(source.len(), |end| at + end);
            !source[at..item_end].contains(r#""AGENTNATIVE_HOME_CONFIG""#)
        })
        .map(|(at, _)| source[..at].matches('\n').count() + 1)
        .collect();
    lines.sort_unstable();
    lines
}

#[test]
fn every_audit_spawning_helper_sets_the_home_config() {
    let tests_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let this_file = Path::new(file!()).file_name().expect("guard file name");
    let mut unguarded: Vec<String> = Vec::new();
    for entry in fs::read_dir(&tests_dir).expect("read tests/") {
        let path = entry.expect("tests/ entry").path();
        let name = path.file_name().expect("file name");
        if name == this_file || path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read test source");
        for line in unguarded_spawns(&source) {
            unguarded.push(format!("tests/{}:{line}", name.to_string_lossy()));
        }
    }
    unguarded.sort();
    assert!(
        unguarded.is_empty(),
        "these helpers spawn `anc` for a test file that runs `anc audit` without setting \
         AGENTNATIVE_HOME_CONFIG, so their audits read the developer's ~/.anc.toml: \
         {unguarded:?}. Set the variable in the helper to `common::empty_home_config()`."
    );
}

#[test]
fn a_helper_without_the_variable_is_flagged() {
    let source = r#"
fn cmd() -> Command {
    Command::cargo_bin("anc").expect("anc")
}

fn audit() { cmd().args(["audit", "."]); }
"#;
    assert_eq!(unguarded_spawns(source), [3]);
}

#[test]
fn a_helper_setting_the_variable_passes() {
    let source = r#"
fn cmd() -> Command {
    let mut cmd = Command::cargo_bin("anc").expect("anc");
    cmd.env("AGENTNATIVE_HOME_CONFIG", "/nowhere/.anc.toml");
    cmd
}

fn audit() { cmd().args(["audit", "."]); }
"#;
    assert!(unguarded_spawns(source).is_empty());
}

#[test]
fn a_file_that_never_audits_is_exempt() {
    let source = r#"
fn cmd() -> Command {
    Command::cargo_bin("anc").expect("anc")
}

fn install() { cmd().args(["skill", "install"]); }
"#;
    assert!(unguarded_spawns(source).is_empty());
}
