//! Audits of this repository run a few at a time. Every test that audits it
//! goes through `common::self_audit`, which holds one of
//! `common::self_audit::SLOTS` machine-wide slots per run, and the slots hold
//! their bound.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use common::self_audit::{SLOTS, audit_slot};

mod common;

const SPAWNS: [&str; 4] = [
    "cmd()",
    "anc()",
    r#"Command::cargo_bin("anc")"#,
    r#"Command::new(env!("CARGO_BIN_EXE_anc"))"#,
];

/// Line numbers of the `anc` spawns in `source` that audit this repository
/// outside `common::self_audit`: a spawn statement that passes `"."` and sets
/// no working directory, so `.` is the package root.
fn unbounded_self_audits(source: &str) -> Vec<usize> {
    let mut lines: Vec<usize> = SPAWNS
        .iter()
        .flat_map(|spawn| source.match_indices(spawn))
        .filter(|&(at, _)| {
            let statement_end = source[at..].find(';').map_or(source.len(), |end| at + end);
            let statement = &source[at..statement_end];
            statement.contains(r#"".""#) && !statement.contains(".current_dir(")
        })
        .map(|(at, _)| source[..at].matches('\n').count() + 1)
        .collect();
    lines.sort_unstable();
    lines.dedup();
    lines
}

#[test]
fn every_audit_of_this_repository_goes_through_self_audit() {
    let tests_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut found: Vec<(String, usize)> = Vec::new();
    for entry in fs::read_dir(&tests_dir).expect("read tests/") {
        let path = entry.expect("tests/ entry").path();
        let name = path.file_name().expect("file name").to_string_lossy();
        // A guard holds the spawn forms it scans for, and sample sources, as data.
        if name.ends_with("_guard.rs") || path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read test source");
        for line in unbounded_self_audits(&source) {
            found.push((name.to_string(), line));
        }
    }
    found.sort();
    let unbounded: Vec<String> = found
        .iter()
        .map(|(name, line)| format!("tests/{name}:{line}"))
        .collect();
    assert!(
        unbounded.is_empty(),
        "these spawns audit this repository without a slot, so a test run starts one per \
         core and parallel runs stack them by the hundreds: {unbounded:?}. Call \
         `common::self_audit::self_audit(&[...])` instead."
    );
}

#[test]
fn a_spawn_auditing_the_package_root_is_flagged() {
    let source = r#"
fn quiet() {
    cmd()
        .args(["-q", "."])
        .assert();
}
"#;
    assert_eq!(unbounded_self_audits(source), [3]);
}

#[test]
fn a_spawn_with_its_own_working_directory_passes() {
    let source = r#"
fn workspace() {
    cmd()
        .current_dir(tmp.path())
        .args(["audit", "."])
        .assert();
}
"#;
    assert!(unbounded_self_audits(source).is_empty());
}

#[test]
fn a_self_audit_call_passes() {
    let source = r#"
fn quiet() {
    let out = self_audit(&["audit", ".", "-q"]);
}
"#;
    assert!(unbounded_self_audits(source).is_empty());
}

#[test]
fn no_more_than_slots_holders_run_at_once() {
    let holding = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..SLOTS * 3 {
            scope.spawn(|| {
                let _slot = audit_slot();
                let now = holding.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(100));
                holding.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    let peak = peak.into_inner();
    assert!(
        (1..=SLOTS).contains(&peak),
        "{peak} threads held a slot at once; the bound is {SLOTS}"
    );
}
