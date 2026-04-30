//! Build script. Two codegen pipelines plus build_info:
//!
//! 1. Vendored spec frontmatter (`src/principles/spec/principles/p*-*.md`) →
//!    `$OUT_DIR/generated_requirements.rs`. Driven by `build_support/parser.rs`.
//! 2. Vendored skill manifest (`src/skill_install/skill.json`) →
//!    `$OUT_DIR/generated_hosts.rs`. The manifest's `install` map is the
//!    single source of truth for the `SkillHost` enum, `KNOWN_HOSTS` const,
//!    and `resolve_host` fn. Updates to the JSON regenerate the Rust map
//!    on next build — no manual sync.
//!
//! Errors here are *intentionally loud* — every parse failure cites the
//! file, requirement / host id, and field. The build is the right time to
//! catch fixture drift.

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
    emit_skill_hosts(&manifest_dir);

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

/// Emit `$OUT_DIR/generated_hosts.rs` from `src/skill_install/skill.json`.
///
/// Reads the manifest's `install` map and emits, for every `<host>` key:
///
/// - a `SkillHost` enum variant (PascalCase of the snake_case key) with
///   `clap::ValueEnum` derive + `#[value(rename_all = "snake_case")]` so
///   surface names round-trip back to the JSON key verbatim;
/// - an entry in `KNOWN_HOSTS: &[&str]`;
/// - a match arm in `resolve_host(SkillHost) -> (&'static str, &'static str)`
///   returning the `(url, dest_template)` parsed from the host's install
///   command.
///
/// Each install command MUST have the canonical shape
/// `git clone --depth 1 <url> <dest>` — six whitespace-separated tokens.
/// Anything else panics the build with the offending host and command.
/// This mirrors `agentnative-site/src/build/skill.mjs` validation so the
/// two binaries reject the same malformed inputs.
fn emit_skill_hosts(manifest_dir: &std::path::Path) {
    let skill_json_path = manifest_dir.join("src/skill_install/skill.json");
    println!("cargo:rerun-if-changed=src/skill_install/skill.json");

    let content = fs::read_to_string(&skill_json_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", skill_json_path.display()));
    let manifest: serde_json::Value = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("parse {}: {e}", skill_json_path.display()));

    let install = manifest
        .get("install")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| {
            panic!(
                "{}: \"install\" must be an object (host -> command map)",
                skill_json_path.display()
            )
        });

    if install.is_empty() {
        panic!(
            "{}: install map is empty — at least one host required",
            skill_json_path.display()
        );
    }

    // Collect (json_key, variant, url, dest) — sorted by JSON key so the
    // generated source has stable byte output across runs (mirrors the
    // site emitter's sorted-keys contract).
    let mut hosts: Vec<(String, String, String, String)> = Vec::with_capacity(install.len());
    for (key, cmd_value) in install {
        let cmd = cmd_value.as_str().unwrap_or_else(|| {
            panic!(
                "{}: install.{key:?} must be a string",
                skill_json_path.display()
            )
        });
        let tokens: Vec<&str> = cmd.split_whitespace().collect();
        if tokens.len() != 6
            || tokens[0] != "git"
            || tokens[1] != "clone"
            || tokens[2] != "--depth"
            || tokens[3] != "1"
        {
            panic!(
                "{}: install.{key:?} must match `git clone --depth 1 <url> <dest>` (got {} tokens: {cmd:?})",
                skill_json_path.display(),
                tokens.len(),
            );
        }
        let url = tokens[4].to_string();
        let dest = tokens[5].to_string();
        if dest.ends_with(".git") {
            panic!(
                "{}: install.{key:?} dest {dest:?} ends in `.git` — host commands must terminate with an explicit destination, not the bare repo name",
                skill_json_path.display()
            );
        }
        let variant = pascal_case(key).unwrap_or_else(|e| {
            panic!(
                "{}: install.{key:?} is not a valid Rust identifier: {e}",
                skill_json_path.display()
            )
        });
        hosts.push((key.clone(), variant, url, dest));
    }
    hosts.sort_by(|a, b| a.0.cmp(&b.0));

    // Render Rust source.
    let mut src = String::new();
    src.push_str(
        "// @generated by build.rs from src/skill_install/skill.json. Do not edit by hand.\n",
    );
    src.push_str(
        "// Add or remove hosts via the JSON file (or `bash scripts/sync-skill-fixture.sh`)\n",
    );
    src.push_str("// and `cargo build` regenerates this file.\n\n");

    src.push_str("/// Hosts the binary knows how to install into. Surface names match\n");
    src.push_str("/// `agentnative-site/src/data/skill.json` keys verbatim via\n");
    src.push_str("/// `rename_all = \"snake_case\"`.\n");
    src.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq, ::clap::ValueEnum)]\n");
    src.push_str("#[value(rename_all = \"snake_case\")]\n");
    src.push_str("pub enum SkillHost {\n");
    for (_, variant, _, _) in &hosts {
        src.push_str(&format!("    {variant},\n"));
    }
    src.push_str("}\n\n");

    src.push_str(
        "/// Host names accepted by `anc skill install <host>`, in JSON-key sort order.\n",
    );
    src.push_str("/// Surfaces externally for shell-completion enumeration and as the seed\n");
    src.push_str("/// for a future `anc skill list` verb. Stays in lockstep with [`SkillHost`]\n");
    src.push_str("/// variants because both are generated from the same source.\n");
    src.push_str("#[allow(dead_code)]\n");
    src.push_str("pub const KNOWN_HOSTS: &[&str] = &[\n");
    for (key, _, _, _) in &hosts {
        src.push_str(&format!("    {key:?},\n"));
    }
    src.push_str("];\n\n");

    src.push_str("/// Resolve a host enum to its `(url, dest_template)` pair, parsed\n");
    src.push_str("/// at build time from the install command in src/skill_install/skill.json.\n");
    src.push_str("/// Pure function — no I/O, no side effects.\n");
    src.push_str("pub fn resolve_host(host: SkillHost) -> (&'static str, &'static str) {\n");
    src.push_str("    match host {\n");
    for (_, variant, url, dest) in &hosts {
        src.push_str(&format!(
            "        SkillHost::{variant} => ({url:?}, {dest:?}),\n"
        ));
    }
    src.push_str("    }\n");
    src.push_str("}\n\n");

    src.push_str("/// JSON-key string for the envelope's `host` field. Generated alongside\n");
    src.push_str("/// the enum so the surface stays in lockstep with the JSON contract.\n");
    src.push_str("pub fn host_envelope_str(host: SkillHost) -> &'static str {\n");
    src.push_str("    match host {\n");
    for (key, variant, _, _) in &hosts {
        src.push_str(&format!("        SkillHost::{variant} => {key:?},\n"));
    }
    src.push_str("    }\n");
    src.push_str("}\n");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let out_path = out_dir.join("generated_hosts.rs");
    fs::write(&out_path, src)
        .unwrap_or_else(|e| panic!("cannot write {}: {e}", out_path.display()));
}

/// Convert a snake_case ASCII identifier to PascalCase. Rejects empty
/// strings, leading digits, and any character outside `[a-z0-9_]` so the
/// emitted variant is always a valid Rust identifier.
fn pascal_case(snake: &str) -> Result<String, String> {
    if snake.is_empty() {
        return Err("empty identifier".into());
    }
    if snake.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(format!("{snake:?} starts with a digit"));
    }
    if !snake
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(format!(
            "{snake:?} contains non-snake_case ASCII characters"
        ));
    }
    let mut out = String::with_capacity(snake.len());
    for word in snake.split('_') {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(chars.as_str());
        }
    }
    Ok(out)
}
