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

#[path = "build_support/parser.rs"]
mod parser;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let spec_dir = manifest_dir.join("src/principles/spec");
    let principles_dir = spec_dir.join("principles");

    println!("cargo:rerun-if-changed=src/principles/spec/");
    println!("cargo:rerun-if-changed=build_support/parser.rs");

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
