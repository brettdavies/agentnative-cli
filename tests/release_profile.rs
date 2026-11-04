//! The release profile and the crate's panic handling have to agree: the test
//! profile always unwinds, so every other test passes whichever way a shipped
//! build handles a panic.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read a source directory") {
        let path = entry.expect("read a directory entry").path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn a_release_build_unwinds_while_the_crate_catches_panics() {
    let manifest: toml::Table =
        toml::from_str(&fs::read_to_string("Cargo.toml").expect("read the manifest"))
            .expect("parse the manifest");
    let strategy = manifest["profile"]["release"]
        .get("panic")
        .and_then(toml::Value::as_str)
        .unwrap_or("unwind");

    let mut sources = Vec::new();
    rust_files(Path::new("src"), &mut sources);
    let catching: Vec<PathBuf> = sources
        .into_iter()
        .filter(|path| {
            fs::read_to_string(path)
                .expect("read a source file")
                .contains("catch_unwind")
        })
        .collect();

    assert!(
        strategy == "unwind" || catching.is_empty(),
        "[profile.release] panic = {strategy:?}, and catch_unwind catches nothing there: {catching:?}"
    );
}
