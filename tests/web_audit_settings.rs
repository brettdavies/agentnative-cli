//! The rule that every engine setting reaches the engine from `constants.json`: a
//! changed site setting that the engine ignores passes every other test, because no
//! corpus scenario runs against a wall clock. These read the engine's own source, so
//! they tokenize it first, and test that reading on the shapes that could hide a literal.

#![cfg(feature = "web")]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agentnative::web_audit::engine::{FetchHandle, RunInput};
use agentnative::web_audit::fetch::FetchOptions;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::mock::{MockResponse, MockTransport};
use agentnative::web_audit::registry::AUDIT_USER_AGENT;
use common::corpus::PublicResolver;
use regex::Regex;
use serde_json::{Map, Value};

/// A number under this is too common to tell from an unrelated one, outside a position
/// that only ever takes a setting: a count of 4 is mostly not this one.
const SEARCHED_FROM: u64 = 1_000;

/// Constants whose value a setting could come to equal with no relation between them.
const UNRELATED_CONSTANTS: [(&str, &str); 2] = [
    (
        "AUDIT_ROOT_MAX_BODY_BYTES",
        "the site reads the root whole and has no such ceiling",
    ),
    ("CHUNK", "the read loop's buffer, not a limit"),
];

/// Settings the contract carries that no engine code reads, and why.
const UNREAD: [(&str, &str); 2] = [
    (
        "REGISTRY_FINGERPRINT",
        "it identifies the registry's data, and nothing in a run compares it",
    ),
    (
        "PROBE_UA_TOKENS",
        "the site expands the tokens in registry.json before this build reads it",
    ),
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn constants() -> Map<String, Value> {
    let raw = fs::read_to_string(root().join("src/web_audit/vendored/contract/constants.json"))
        .expect("constants.json");
    serde_json::from_str(&raw).expect("constants.json parses")
}

/// The name a `constants.json` key is emitted under.
fn emitted_name(key: &str) -> &str {
    key.strip_suffix("_MS").unwrap_or(key)
}

/// Source with comments removed and string and character literals emptied, so what is
/// left is code.
fn code_only(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let before = i.checked_sub(1).map(|p| chars[p]);
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 1;
            i += 2;
            while i < chars.len() && depth > 0 {
                match (chars[i], chars.get(i + 1)) {
                    ('/', Some('*')) => {
                        depth += 1;
                        i += 2;
                    }
                    ('*', Some('/')) => {
                        depth -= 1;
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
        } else if c == 'r' && matches!(next, Some('"' | '#')) && !before.is_some_and(is_ident) {
            let mut hashes = 0;
            let mut j = i + 1;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                j += 1;
                while let Some(&ch) = chars.get(j) {
                    j += 1;
                    if ch == '"' && (0..hashes).all(|n| chars.get(j + n) == Some(&'#')) {
                        j += hashes;
                        break;
                    }
                }
                out.push_str("\"\"");
                i = j;
            } else {
                out.push(c);
                i += 1;
            }
        } else if c == '"' {
            i += 1;
            while i < chars.len() {
                match chars[i] {
                    '\\' => i += 2,
                    '"' => {
                        i += 1;
                        break;
                    }
                    _ => i += 1,
                }
            }
            out.push_str("\"\"");
        } else if c == '\'' {
            // A character literal closes within a few characters; a lifetime never does.
            let close = if next == Some('\\') { i + 3 } else { i + 2 };
            if chars.get(close) == Some(&'\'') {
                out.push_str("' '");
                i = close + 1;
            } else if next == Some('\\') {
                let mut j = i + 2;
                while j < chars.len() && chars[j] != '\'' {
                    j += 1;
                }
                out.push_str("' '");
                i = j + 1;
            } else {
                out.push(c);
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Code with every `#[cfg(test)]` item and every `use` statement removed: a test or a
/// re-export reads nothing.
fn production_code(text: &str) -> String {
    let code = code_only(text);
    let mut out = String::with_capacity(code.len());
    let mut rest = code.as_str();
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let item = &rest[at + "#[cfg(test)]".len()..];
        let mut depth = 0;
        let mut end = item.len();
        for (offset, c) in item.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = offset + 1;
                        break;
                    }
                }
                ';' if depth == 0 => {
                    end = offset + 1;
                    break;
                }
                _ => {}
            }
        }
        rest = &item[end..];
    }
    out.push_str(rest);
    let uses = Regex::new(r"(?m)^\s*(?:pub(?:\([a-z]+\))?\s+)?use\s[^;]*;").unwrap();
    uses.replace_all(&out, "").into_owned()
}

/// An integer expression as code writes one: `6_000`, `6000u64`, `0x800`, `2 * 3`,
/// `1 << 11`. Anything else is not a literal.
fn literal_value(expr: &str) -> Option<u64> {
    let one = |text: &str| -> Option<u64> {
        let clean = text.trim().replace('_', "");
        let digits = [
            "usize", "u128", "u64", "u32", "u16", "u8", "isize", "i64", "i32",
        ]
        .iter()
        .find_map(|suffix| clean.strip_suffix(suffix))
        .unwrap_or(&clean);
        match digits.strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16).ok(),
            None => digits.parse().ok(),
        }
    };
    expr.split('*').try_fold(1_u64, |product, factor| {
        let value = match factor.split_once("<<") {
            Some((base, shift)) => one(base)?.checked_shl(u32::try_from(one(shift)?).ok()?)?,
            None => one(factor)?,
        };
        product.checked_mul(value)
    })
}

/// Each place `code` writes a value a setting holds, as `<value>: <what>`. `code` has
/// been through `production_code`.
fn repeated_settings(code: &str, settings: &[(&str, u64)]) -> Vec<String> {
    // A position that only ever takes a setting counts at any size; elsewhere a small
    // number says nothing.
    let mut found: Vec<(u64, String, bool)> = Vec::new();
    let duration =
        Regex::new(r"from_(millis|secs|secs_f64)\(([^()]*)\)|Duration::new\(([^,()]*),").unwrap();
    for caps in duration.captures_iter(code) {
        let what = caps[0].to_string();
        let millis = match (caps.get(1).map(|m| m.as_str()), caps.get(2), caps.get(3)) {
            (Some("millis"), Some(expr), _) => literal_value(expr.as_str()),
            (Some("secs"), Some(expr), _) => literal_value(expr.as_str()).map(|s| s * 1_000),
            (Some("secs_f64"), Some(expr), _) => expr
                .as_str()
                .trim()
                .parse::<f64>()
                .ok()
                .map(|s| (s * 1_000.0) as u64),
            (_, _, Some(expr)) => literal_value(expr.as_str()).map(|s| s * 1_000),
            _ => None,
        };
        if let Some(millis) = millis {
            found.push((millis, what, false));
        }
    }
    let constant = Regex::new(r"(?:const|static)\s+([A-Z0-9_]+)\s*:[^=;]*=\s*([^;]+);").unwrap();
    for caps in constant.captures_iter(code) {
        if UNRELATED_CONSTANTS
            .iter()
            .any(|(name, _)| *name == &caps[1])
        {
            continue;
        }
        if let Some(value) = literal_value(&caps[2]) {
            found.push((value, caps[0].to_string(), false));
        }
    }
    let field = Regex::new(
        r"\b(max_body_bytes|max_redirects|concurrency):\s*(?:Some\()?([^,()\n]+)\)?\s*,",
    )
    .unwrap();
    for caps in field.captures_iter(code) {
        if let Some(value) = literal_value(&caps[2]) {
            found.push((value, caps[0].to_string(), true));
        }
    }
    found
        .into_iter()
        .flat_map(|(value, what, any_size)| {
            settings
                .iter()
                .filter(move |(_, setting)| {
                    *setting == value && (any_size || value >= SEARCHED_FROM)
                })
                .map(move |(key, _)| format!("{what} repeats {key}"))
        })
        .collect()
}

/// Production code under `dir`, relative to the repository.
fn code_under(dir: &str) -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
        for entry in fs::read_dir(dir).expect("a source directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                if path.file_name().is_none_or(|name| name != "vendored") {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = fs::read_to_string(&path).expect("a source file");
                out.push((path, production_code(&text)));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root().join(dir), &mut out);
    assert!(out.len() > 10, "the walk found {dir}");
    out
}

fn engine_code() -> Vec<(PathBuf, String)> {
    code_under("src/web_audit")
}

fn numeric_settings(doc: &Map<String, Value>) -> Vec<(&str, u64)> {
    doc.iter()
        .filter_map(|(key, value)| Some((key.as_str(), value.as_u64()?)))
        .collect()
}

#[test]
fn no_engine_constant_is_named_for_a_site_setting() {
    let doc = constants();
    let definition =
        Regex::new(r"(?m)^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const|static)\s+([A-Z0-9_]+)\s*:")
            .unwrap();
    for (path, code) in engine_code() {
        for found in definition.captures_iter(&code) {
            let name = &found[1];
            assert!(
                !doc.keys()
                    .any(|key| key == name || emitted_name(key) == name),
                "{} defines {name}; read it from the contract",
                path.display()
            );
        }
    }
}

#[test]
fn no_engine_literal_repeats_a_site_setting() {
    let doc = constants();
    let settings = numeric_settings(&doc);
    for (path, code) in engine_code() {
        let repeats = repeated_settings(&code, &settings);
        assert!(repeats.is_empty(), "{}: {repeats:?}", path.display());
    }
}

#[test]
fn this_build_reads_every_setting() {
    let doc = constants();
    let code = code_under("src");
    for key in doc.keys() {
        if UNREAD.iter().any(|(name, _)| *name == key) {
            continue;
        }
        let name = Regex::new(&format!(r"\b{}\b", emitted_name(key))).unwrap();
        assert!(
            code.iter().any(|(_, text)| name.is_match(text)),
            "no production code under src reads {}",
            emitted_name(key)
        );
    }
    for (name, why) in UNREAD {
        assert!(doc.contains_key(name), "{name} is gone, and so is: {why}");
    }
}

/// The corpus cannot exercise wall-clock settings, so these read what a run and a fetch
/// are actually given.
#[test]
fn a_run_and_a_fetch_take_their_defaults_from_the_contract() {
    let doc = constants();
    let millis = |key: &str| Duration::from_millis(doc[key].as_u64().unwrap());
    let count = |key: &str| usize::try_from(doc[key].as_u64().unwrap()).unwrap();
    let fetch = FetchOptions::default();
    assert_eq!(fetch.timeout, millis("DEFAULT_TIMEOUT_MS"));
    assert_eq!(fetch.max_redirects, count("DEFAULT_MAX_REDIRECTS"));
    let transport = Arc::new(MockTransport::new(
        Vec::new(),
        MockResponse::ok(200, &[], b""),
    ));
    let handle = FetchHandle::new(transport, Arc::new(PublicResolver), AUDIT_USER_AGENT);
    let input = RunInput::new("https://example.com/", handle, Arc::new(default_handlers()));
    assert_eq!(input.concurrency, count("DEFAULT_CONCURRENCY"));
    assert_eq!(
        input.per_check_timeout,
        millis("DEFAULT_PER_CHECK_TIMEOUT_MS")
    );
    assert_eq!(
        input.per_audit_deadline,
        millis("DEFAULT_PER_AUDIT_DEADLINE_MS")
    );
}

// The reading itself, on the shapes that could hide a setting from these tests.

const SETTING: [(&str, u64); 1] = [("A_SETTING_MS", 6_000)];

fn finds(source: &str) -> bool {
    !repeated_settings(&production_code(source), &SETTING).is_empty()
}

#[test]
fn a_setting_is_found_in_every_way_code_writes_one() {
    for source in [
        "fn a() { from_millis(6_000); }",
        "fn a() { from_millis(6000u64); }",
        "fn a() { from_millis(6_000_u64); }",
        "fn a() { from_secs(6); }",
        "fn a() { from_secs(2 * 3); }",
        "fn a() { from_secs_f64(6.0); }",
        "fn a() { Duration::new(6, 0); }",
        "const LIMIT: u64 = 6_000;",
        "static LIMIT: u64 = 6000;",
        "const LIMIT: u64 = 0x1770;",
        "const LIMIT: u64 = 3 << 1 * 1_000;",
        "fn a() { Options { max_body_bytes: Some(6_000), }; }",
    ] {
        assert!(finds(source), "{source}");
    }
}

#[test]
fn a_setting_past_a_string_that_could_lose_the_reader_is_still_found() {
    for source in [
        "const A: &str = r#\"one \" quote\"#;\nconst LIMIT: u64 = 6_000;",
        "const A: &str = r\"odd \\\";\nconst LIMIT: u64 = 6_000;",
        "const A: &str = \"line \\\n  continued\";\nconst LIMIT: u64 = 6_000;",
        "const A: char = '\"';\nconst LIMIT: u64 = 6_000;",
        "const A: char = '\\'';\nconst LIMIT: u64 = 6_000;",
        "fn a<'a>(b: &'a str) {}\nconst LIMIT: u64 = 6_000;",
        "/* a \" quote */\nconst LIMIT: u64 = 6_000;",
        "// a \" quote\nconst LIMIT: u64 = 6_000;",
        "#[cfg(test)]\nuse std::fs;\nconst LIMIT: u64 = 6_000;",
        "#[cfg(test)]\nfn helper() { let a = 1; }\nconst LIMIT: u64 = 6_000;",
    ] {
        assert!(finds(source), "{source}");
    }
}

#[test]
fn what_is_not_code_or_not_a_setting_is_not_found() {
    for source in [
        "const A: &str = \"6000\";",
        "const A: &str = r\"\\u{1770}from_millis(6000)\";",
        "// from_millis(6000)",
        "/* const A: u64 = 6000; */",
        "#[cfg(test)]\nmod tests { fn a() { from_millis(6000); } }",
        "const CHUNK: usize = 6_000;",
        "fn a(b: u64) -> u64 { b / 6000 }",
        "fn a() { from_millis(5_999); }",
    ] {
        assert!(!finds(source), "{source}");
    }
}

#[test]
fn a_reexport_or_a_comment_is_not_a_read() {
    let code = production_code(
        "pub use crate::registry::A_SETTING;\nuse crate::{\n    A_SETTING,\n};\n// A_SETTING\nfn a() {}\n",
    );
    assert!(!code.contains("A_SETTING"), "{code}");
}
