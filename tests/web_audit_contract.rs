//! The emitted tables against the contract they came from, row for row, and the
//! rule that every engine setting reaches the engine from `constants.json`.

#![cfg(feature = "web")]

use std::fs;
use std::path::{Path, PathBuf};

use agentnative::web_audit::registry::{
    ALTERNATIVES, AUDIT_USER_AGENT, CATEGORIES, CATEGORY_ORDER, CHECKS, CONTRACT_FINGERPRINT,
    MCP_DISCOVERY, MCP_LANES, PROBE_UA_TOKENS, REGISTRY_VERSION, REMEDIATION, RETIRED_CHECK_IDS,
};
use serde_json::{Map, Value};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn contract(name: &str) -> Value {
    let path = root().join("src/web_audit/vendored/contract").join(name);
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn strings(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|v| v.as_str().expect("a string"))
        .collect()
}

#[test]
fn the_check_table_equals_registry_json_row_for_row() {
    let doc = contract("registry.json");
    let rows = doc["checks"].as_array().unwrap();
    assert_eq!(rows.len(), CHECKS.len());
    for (row, check) in rows.iter().zip(CHECKS) {
        let id = check.id;
        assert_eq!(row["id"], check.id);
        assert_eq!(row["category"], check.category, "{id}");
        assert_eq!(row["tier"], check.tier.as_str(), "{id}");
        assert_eq!(row["keyword"], check.keyword.as_str(), "{id}");
        assert_eq!(row["principle"], check.principle, "{id}");
        assert_eq!(row["antecedent"], check.antecedent.as_str(), "{id}");
        assert_eq!(row["weight"], check.weight, "{id}");
        assert_eq!(row["title"], check.title, "{id}");
        assert_eq!(row["breadcrumb"], check.breadcrumb, "{id}");
        assert_eq!(row["hint"], check.hint, "{id}");
        assert_eq!(row["handler"], check.handler.as_str(), "{id}");
        assert_eq!(row["eval"].as_str(), check.eval.map(|e| e.as_str()), "{id}");
        assert_eq!(row["lane"].as_str(), check.lane, "{id}");
        let site_types: Vec<&str> = check.site_types.iter().map(|t| t.as_str()).collect();
        assert_eq!(strings(&row["site_types"]), site_types, "{id}");
        let with: Value = serde_json::from_str(check.with_json).unwrap();
        assert_eq!(row["with"], with, "{id}");
    }
}

#[test]
fn the_other_tables_equal_registry_json() {
    let doc = contract("registry.json");
    assert_eq!(doc["version"], REGISTRY_VERSION);
    assert_eq!(strings(&doc["category_order"]), CATEGORY_ORDER);
    for (slug, name) in CATEGORIES {
        assert_eq!(doc["categories"][*slug], *name, "{slug}");
    }
    assert_eq!(
        doc["categories"].as_object().unwrap().len(),
        CATEGORIES.len()
    );

    let discovery = &doc["mcp_discovery"];
    assert_eq!(discovery["ai_catalog"], MCP_DISCOVERY.ai_catalog);
    assert_eq!(discovery["card_suffix"], MCP_DISCOVERY.card_suffix);
    assert_eq!(strings(&discovery["well_known"]), MCP_DISCOVERY.well_known);
    assert_eq!(
        strings(&discovery["common_paths"]),
        MCP_DISCOVERY.common_paths
    );
    assert_eq!(
        discovery["protocol_version"],
        MCP_DISCOVERY.protocol_version
    );

    let lanes: &Map<String, Value> = doc["mcp_lanes"].as_object().unwrap();
    let ids: Vec<&str> = lanes.keys().map(String::as_str).collect();
    let emitted: Vec<&str> = MCP_LANES.iter().map(|l| l.id).collect();
    assert_eq!(ids, emitted, "lanes keep the contract's display order");
    for lane in MCP_LANES {
        assert_eq!(lanes[lane.id]["label"], lane.label);
        assert_eq!(lanes[lane.id]["note"], lane.note);
    }

    let retired: Vec<&str> = doc["retired"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(retired, RETIRED_CHECK_IDS);

    let groups = doc["alternatives"].as_array().unwrap();
    assert_eq!(groups.len(), ALTERNATIVES.len());
    for (raw, group) in groups.iter().zip(ALTERNATIVES) {
        assert_eq!(raw["group"], group.group);
        for variant in group.variants {
            let row = &raw["variants"][variant.name];
            let tokens = |token: &[_]| -> Vec<&'static str> {
                token
                    .iter()
                    .map(|t: &agentnative::web_audit::registry::AntecedentToken| t.as_str())
                    .collect()
            };
            assert_eq!(strings(&row["antecedents"]), tokens(variant.antecedents));
            assert_eq!(strings(&row["presented_by"]), tokens(variant.presented_by));
        }
    }
}

#[test]
fn the_fix_catalog_equals_remediation_json() {
    let doc = contract("remediation.json");
    let entries = doc.as_object().unwrap();
    assert_eq!(entries.len(), REMEDIATION.len());
    for entry in REMEDIATION {
        let row = &entries[entry.id];
        assert_eq!(row["title"], entry.title, "{}", entry.id);
        assert_eq!(row["goal"], entry.goal, "{}", entry.id);
        assert_eq!(row["fix"], entry.fix, "{}", entry.id);
        let resources = row["resources"].as_array().map_or(&[][..], Vec::as_slice);
        assert_eq!(resources.len(), entry.resources.len(), "{}", entry.id);
        for (raw, resource) in resources.iter().zip(entry.resources) {
            assert_eq!(raw["label"], resource.label, "{}", entry.id);
            assert_eq!(raw["url"], resource.url, "{}", entry.id);
        }
    }
}

#[test]
fn the_user_agents_equal_constants_json() {
    let doc = contract("constants.json");
    assert_eq!(doc["AUDIT_USER_AGENT"], AUDIT_USER_AGENT);
    let tokens = doc["PROBE_UA_TOKENS"].as_object().unwrap();
    assert_eq!(tokens.len(), PROBE_UA_TOKENS.len());
    for (token, literal) in PROBE_UA_TOKENS {
        assert_eq!(tokens[*token], *literal, "{token}");
    }
}

/// The file ends in a newline and the site's signal serves the value trimmed, so the
/// staleness note compares this constant and never the file's bytes.
#[test]
fn the_contract_fingerprint_is_the_files_value_trimmed() {
    let raw = fs::read_to_string(root().join("src/web_audit/vendored/contract/FINGERPRINT"))
        .expect("FINGERPRINT");
    assert_ne!(raw, raw.trim(), "the file carries its trailing newline");
    assert_eq!(CONTRACT_FINGERPRINT, raw.trim());
    assert!(!CONTRACT_FINGERPRINT.is_empty());
    assert!(CONTRACT_FINGERPRINT.chars().all(|c| c.is_ascii_hexdigit()));
}

fn engine_sources() -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
        for entry in fs::read_dir(dir).expect("a source directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                if path.file_name().is_none_or(|name| name != "vendored") {
                    walk(&path, out);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = fs::read_to_string(&path).expect("a source file");
                out.push((path, text));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root().join("src/web_audit"), &mut out);
    out
}

/// The name a `constants.json` key is emitted under.
fn emitted_name(key: &str) -> &str {
    key.strip_suffix("_MS").unwrap_or(key)
}

#[test]
fn no_engine_literal_duplicates_a_site_setting() {
    let doc = contract("constants.json");
    let sources = engine_sources();
    for key in doc.as_object().unwrap().keys() {
        let definition = format!("const {}:", emitted_name(key));
        for (path, text) in &sources {
            assert!(
                !text.contains(&definition),
                "{} defines {definition}; read it from the contract",
                path.display()
            );
        }
    }
}

#[test]
fn the_engine_reads_every_numeric_setting() {
    let doc = contract("constants.json");
    let sources = engine_sources();
    for (key, value) in doc.as_object().unwrap() {
        if !value.is_u64() {
            continue;
        }
        let name = emitted_name(key);
        assert!(
            sources.iter().any(|(_, text)| text.contains(name)),
            "nothing under src/web_audit reads {name}"
        );
    }
}

#[test]
fn the_settings_reach_the_engine_from_the_contract() {
    use agentnative::web_audit::engine::{DEFAULT_CONCURRENCY, DEFAULT_PER_CHECK_TIMEOUT};
    let doc = contract("constants.json");
    assert_eq!(doc["DEFAULT_CONCURRENCY"], DEFAULT_CONCURRENCY);
    assert_eq!(
        doc["DEFAULT_PER_CHECK_TIMEOUT_MS"].as_u64(),
        u64::try_from(DEFAULT_PER_CHECK_TIMEOUT.as_millis()).ok()
    );
}

/// The crate carries what `build.rs` reads and leaves the test corpus behind.
#[test]
fn the_published_crate_holds_the_build_inputs_and_not_the_corpus() {
    let cargo = std::env::var("CARGO").expect("cargo sets CARGO for a test");
    let output = std::process::Command::new(cargo)
        .args([
            "package",
            "--list",
            "--allow-dirty",
            "--offline",
            "--features",
            "web",
        ])
        .current_dir(root())
        .output()
        .expect("run cargo package --list");
    assert!(output.status.success(), "{output:?}");
    let listing = String::from_utf8_lossy(&output.stdout);
    for name in [
        "registry.json",
        "remediation.json",
        "constants.json",
        "server-card.schema.json",
        "FINGERPRINT",
    ] {
        let path = format!("src/web_audit/vendored/contract/{name}");
        assert!(
            listing.lines().any(|line| line == path),
            "{path} missing:\n{listing}"
        );
    }
    assert!(
        !listing.contains("web-audit-contract"),
        "the corpus stays out of the crate:\n{listing}"
    );
}
