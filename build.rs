//! Build script: parse vendored spec frontmatter into `$OUT_DIR/generated_requirements.rs`.
//!
//! See `build_support/parser.rs` for the parser; this file is a thin driver
//! that reads `src/principles/spec/principles/p*-*.md`, hands each file to the
//! parser, aggregates, and writes the generated Rust source.
//!
//! Errors here are *intentionally loud* — every parse failure cites the file,
//! requirement id, and field. The build is the right time to catch spec drift.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[path = "build_support/parser.rs"]
mod parser;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let spec_dir = manifest_dir.join("src/principles/spec");
    let principles_dir = spec_dir.join("principles");

    println!("cargo:rerun-if-changed=src/principles/spec/");
    println!("cargo:rerun-if-changed=build_support/parser.rs");

    emit_build_info(&manifest_dir);

    let spec_version = match fs::read_to_string(spec_dir.join("VERSION")) {
        Ok(s) => s.trim().to_string(),
        Err(_) => {
            println!(
                "cargo:warning=src/principles/spec/VERSION missing — emitting SPEC_VERSION = \"unknown\""
            );
            "unknown".to_string()
        }
    };

    let entries = fs::read_dir(&principles_dir).unwrap_or_else(|e| {
        panic!("cannot read {}: {e}", principles_dir.display());
    });

    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = match p.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => return false,
            };
            name.starts_with('p') && name.contains('-') && p.extension().is_some_and(|x| x == "md")
        })
        .collect();
    files.sort();

    if files.is_empty() {
        panic!(
            "no `p*-*.md` files in {} — did you run scripts/sync-spec.sh?",
            principles_dir.display()
        );
    }

    let mut parsed_per_file = Vec::with_capacity(files.len());
    for path in &files {
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let content = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let reqs = parser::parse_principle_file(&name, &content)
            .unwrap_or_else(|e| panic!("\n  spec parse error: {e}\n"));
        parsed_per_file.push((name, reqs));
    }

    let aggregated = parser::aggregate(parsed_per_file)
        .unwrap_or_else(|e| panic!("\n  spec aggregate error: {e}\n"));

    let rust_src = parser::emit_rust(&aggregated, &spec_version);

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let out_path = out_dir.join("generated_requirements.rs");
    fs::write(&out_path, rust_src)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", out_path.display()));
}

/// Emit `$OUT_DIR/build_info.rs` with `ANC_VERSION` and `ANC_COMMIT` constants.
///
/// `ANC_VERSION` is the crate version (always present via `CARGO_PKG_VERSION`).
/// `ANC_COMMIT` is the short Git SHA at build time, or `None` for builds outside
/// a Git checkout (e.g. `cargo install` from crates.io). Both fields surface in
/// the scorecard's `anc.{version, commit}` block so a consumer can identify the
/// `anc` build that produced a scorecard.
///
/// **Stale-SHA mitigation.** Without explicit `cargo:rerun-if-changed` directives
/// covering the Git refs, cached builds silently embed whatever SHA was current
/// at the last full rebuild. Three watches cover the common cases: `.git/HEAD`
/// flips on branch switches and detached-HEAD updates; `.git/refs/heads/<branch>`
/// updates on a fresh commit on the current branch (when refs are loose);
/// `.git/packed-refs` covers repos that have packed their refs.
fn emit_build_info(manifest_dir: &std::path::Path) {
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".to_string());

    let git_dir = manifest_dir.join(".git");
    let commit = if git_dir.exists() {
        // Watch HEAD itself — branch switches, detached-HEAD updates.
        println!("cargo:rerun-if-changed=.git/HEAD");
        // Watch packed-refs — repos that have packed their refs after gc.
        println!("cargo:rerun-if-changed=.git/packed-refs");
        // Resolve the current branch by reading HEAD; if it's a symbolic ref to
        // refs/heads/<branch>, watch that ref so a fresh commit triggers rebuild.
        if let Ok(head) = fs::read_to_string(git_dir.join("HEAD")) {
            let head = head.trim();
            if let Some(ref_path) = head.strip_prefix("ref: ") {
                println!("cargo:rerun-if-changed=.git/{ref_path}");
            }
            // Detached HEAD (head is a SHA, not "ref: ..."): no refs/heads/ to watch.
        }

        match Command::new("git")
            .arg("rev-parse")
            .arg("--short")
            .arg("HEAD")
            .current_dir(manifest_dir)
            .output()
        {
            Ok(out) if out.status.success() => {
                let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if sha.is_empty() { None } else { Some(sha) }
            }
            _ => {
                println!("cargo:warning=git rev-parse failed — emitting ANC_COMMIT = None");
                None
            }
        }
    } else {
        println!(
            "cargo:warning=.git missing — emitting ANC_COMMIT = None (released-from-tarball case)"
        );
        None
    };

    let mut src = String::new();
    src.push_str("// @generated by build.rs. Do not edit by hand.\n\n");
    src.push_str("/// Crate version (`CARGO_PKG_VERSION`) at build time.\n");
    src.push_str(&format!("pub const ANC_VERSION: &str = \"{version}\";\n\n"));
    src.push_str("/// Short Git SHA at build time. `None` for builds outside a Git checkout.\n");
    match commit {
        Some(sha) => src.push_str(&format!(
            "pub const ANC_COMMIT: Option<&str> = Some(\"{sha}\");\n"
        )),
        None => src.push_str("pub const ANC_COMMIT: Option<&str> = None;\n"),
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let out_path = out_dir.join("build_info.rs");
    fs::write(&out_path, src)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", out_path.display()));
}
