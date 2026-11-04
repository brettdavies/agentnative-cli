//! The crates `[profile.release.package]` builds for size. Giving up their speed is
//! safe because `anc audit` never compiles them, so the list has to stay inside what
//! only the `web` feature pulls in. `cargo tree` says whether it does, on every target.

use std::collections::BTreeSet;
use std::process::Command;

fn listed() -> BTreeSet<String> {
    include_str!("../Cargo.toml")
        .lines()
        .filter_map(|line| line.strip_suffix(".opt-level = \"s\""))
        .map(str::to_string)
        .collect()
}

fn graph(edges: &str, features: &[&str]) -> BTreeSet<String> {
    let output = Command::new(env!("CARGO"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "tree",
            "--locked",
            "--target",
            "all",
            "--no-default-features",
        ])
        .args(["--prefix", "none", "--format", "{p}", "-e", edges])
        .args(features)
        .output()
        .expect("spawn cargo tree");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout)
        .expect("utf-8")
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

#[test]
fn a_crate_built_for_size_is_never_compiled_without_web() {
    let without_web = graph("normal,build", &[]);
    let shared: Vec<_> = listed().intersection(&without_web).cloned().collect();
    assert!(shared.is_empty(), "{shared:?} compile without `web`");
}

#[test]
fn a_crate_built_for_size_is_in_the_web_graph() {
    let web = graph("normal", &["--features", "web"]);
    let stale: Vec<_> = listed().difference(&web).cloned().collect();
    assert!(stale.is_empty(), "{stale:?} are not in the `web` graph");
}
