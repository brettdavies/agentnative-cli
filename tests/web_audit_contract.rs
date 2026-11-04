//! The emitted tables against the contract they came from, row for row, and the
//! rule that every engine setting reaches the engine from `constants.json`.

#![cfg(feature = "web")]

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use agentnative::web_audit::engine::{FetchHandle, RunInput};
use agentnative::web_audit::fetch::FetchOptions;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::mock::{MockResponse, MockTransport};
use agentnative::web_audit::registry::{
    ALTERNATIVES, AUDIT_USER_AGENT, CATEGORIES, CATEGORY_ORDER, CHECKS, CONTRACT_FINGERPRINT,
    MCP_DISCOVERY, MCP_LANES, PROBE_UA_TOKENS, REGISTRY_VERSION, REMEDIATION, RETIRED_CHECK_IDS,
};
use common::corpus::PublicResolver;
use regex::Regex;
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
        assert_eq!(
            raw["variants"].as_object().unwrap().len(),
            group.variants.len(),
            "{}",
            group.group
        );
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

/// Production code only: a re-export, a comment or a test reads nothing.
fn production_code(text: &str) -> String {
    let body = text.split("#[cfg(test)]").next().unwrap_or(text);
    let mut out = String::new();
    let mut in_use = false;
    for line in body.lines() {
        let trimmed = line.trim_start();
        if in_use
            || trimmed.starts_with("use ")
            || (trimmed.starts_with("pub") && trimmed.contains(" use "))
        {
            in_use = !trimmed.contains(';');
        } else if !trimmed.starts_with("//") {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn engine_code() -> Vec<(PathBuf, String)> {
    engine_sources()
        .into_iter()
        .map(|(path, text)| (path, production_code(&text)))
        .collect()
}

fn parse_int(digits: &str) -> Option<u64> {
    digits.replace('_', "").parse().ok()
}

#[test]
fn no_engine_constant_duplicates_a_site_setting() {
    let doc = contract("constants.json");
    let definition =
        Regex::new(r"(?m)^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const|static)\s+([A-Z0-9_]+)\s*:")
            .unwrap();
    for (path, code) in engine_code() {
        for found in definition.captures_iter(&code) {
            let name = &found[1];
            assert!(
                !doc.as_object()
                    .unwrap()
                    .keys()
                    .any(|key| emitted_name(key) == name),
                "{} defines {name}; read it from the contract",
                path.display()
            );
        }
    }
}

/// A small count is too common a literal to search for, and a setting under a thousand
/// is one, so this finds the sizes and durations a copied value would show up as.
#[test]
fn no_engine_literal_repeats_a_site_setting() {
    const SEARCHED_FROM: u64 = 1_000;
    let doc = contract("constants.json");
    let settings: Vec<(&str, u64)> = doc
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(key, value)| Some((key.as_str(), value.as_u64()?)))
        .filter(|(_, value)| *value >= SEARCHED_FROM)
        .collect();
    let millis = Regex::new(r"from_millis\((\d[\d_]*)\)").unwrap();
    let secs = Regex::new(r"from_secs\((\d[\d_]*)\)").unwrap();
    let product = Regex::new(r"\b\d[\d_]*(?:\s*\*\s*\d[\d_]*)*\b").unwrap();
    // A number inside a string is text: a Unicode range, or a line of a message.
    let string = Regex::new(r#""(?:[^"\\]|\\.)*""#).unwrap();
    for (path, code) in engine_code() {
        let code = string.replace_all(&code, "\"\"");
        let mut written: Vec<u64> = Vec::new();
        written.extend(millis.captures_iter(&code).filter_map(|c| parse_int(&c[1])));
        written.extend(
            secs.captures_iter(&code)
                .filter_map(|c| parse_int(&c[1]).map(|s| s * 1_000)),
        );
        for found in product.find_iter(&code) {
            let factors: Option<Vec<u64>> = found
                .as_str()
                .split('*')
                .map(|f| parse_int(f.trim()))
                .collect();
            if let Some(value) =
                factors.and_then(|f| f.into_iter().try_fold(1_u64, u64::checked_mul))
            {
                written.push(value);
            }
        }
        for (key, value) in &settings {
            assert!(
                !written.contains(value),
                "{} writes {value}, the value of {key}; read it from the contract",
                path.display()
            );
        }
    }
}

#[test]
fn the_engine_reads_every_numeric_setting() {
    let doc = contract("constants.json");
    let code = engine_code();
    for (key, value) in doc.as_object().unwrap() {
        if !value.is_u64() {
            continue;
        }
        let name = Regex::new(&format!(r"\b{}\b", emitted_name(key))).unwrap();
        assert!(
            code.iter().any(|(_, text)| name.is_match(text)),
            "no production code under src/web_audit reads {}",
            emitted_name(key)
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

/// The corpus cannot exercise wall-clock settings, so these read what a run and a fetch
/// are actually given.
#[test]
fn a_run_and_a_fetch_take_their_defaults_from_the_contract() {
    let doc = contract("constants.json");
    let millis = |key: &str| Duration::from_millis(doc[key].as_u64().unwrap());
    let fetch = FetchOptions::default();
    assert_eq!(fetch.timeout, millis("DEFAULT_TIMEOUT_MS"));
    assert_eq!(
        fetch.max_redirects,
        doc["DEFAULT_MAX_REDIRECTS"].as_u64().unwrap() as usize
    );
    let (_, handle) = {
        let transport = Arc::new(MockTransport::new(
            Vec::new(),
            MockResponse::ok(200, &[], b""),
        ));
        let handle = FetchHandle::new(
            transport.clone(),
            Arc::new(PublicResolver),
            AUDIT_USER_AGENT,
        );
        (transport, handle)
    };
    let input = RunInput::new("https://example.com/", handle, Arc::new(default_handlers()));
    assert_eq!(
        input.concurrency,
        doc["DEFAULT_CONCURRENCY"].as_u64().unwrap() as usize
    );
    assert_eq!(
        input.per_check_timeout,
        millis("DEFAULT_PER_CHECK_TIMEOUT_MS")
    );
    assert_eq!(
        input.per_audit_deadline,
        millis("DEFAULT_PER_AUDIT_DEADLINE_MS")
    );
}

// The cross-field rules the site's normalizer owns, read here so a bump that breaks one
// goes red rather than reaching the engine.

#[test]
fn check_ids_are_unique() {
    let doc = contract("registry.json");
    let ids: Vec<&str> = doc["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    let distinct: BTreeSet<&&str> = ids.iter().collect();
    assert_eq!(distinct.len(), ids.len());
}

#[test]
fn every_retired_id_is_gone_and_names_a_live_successor() {
    let doc = contract("registry.json");
    let live: BTreeSet<&str> = doc["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    for (id, retired) in doc["retired"].as_object().unwrap() {
        assert!(!live.contains(id.as_str()), "{id} is retired and live");
        let successor = retired["successor"].as_str().unwrap();
        assert!(live.contains(successor), "{id} names {successor}");
    }
}

#[test]
fn every_alternatives_group_offers_a_choice_and_claims_an_antecedent_once() {
    for group in ALTERNATIVES {
        assert!(group.variants.len() >= 2, "{}", group.group);
        let mut claimed = BTreeSet::new();
        for variant in group.variants {
            for token in variant.antecedents {
                assert!(
                    claimed.insert(token.as_str()),
                    "{}: {}",
                    group.group,
                    token.as_str()
                );
            }
        }
    }
}

#[test]
fn every_principle_is_one_the_spec_has() {
    let principle = Regex::new(r"^P[1-8]$").unwrap();
    for check in CHECKS {
        assert!(
            principle.is_match(check.principle),
            "{}: {}",
            check.id,
            check.principle
        );
    }
}

#[test]
fn discovery_paths_are_absolute() {
    let paths = MCP_DISCOVERY
        .well_known
        .iter()
        .chain(MCP_DISCOVERY.common_paths)
        .chain([&MCP_DISCOVERY.ai_catalog, &MCP_DISCOVERY.card_suffix]);
    for path in paths {
        assert!(path.starts_with('/'), "{path}");
    }
}

#[test]
fn every_fix_links_to_an_absolute_url() {
    for fix in REMEDIATION {
        for resource in fix.resources {
            assert!(
                resource.url.starts_with("https://") || resource.url.starts_with("http://"),
                "{}: {}",
                fix.id,
                resource.url
            );
        }
    }
}

#[test]
fn nothing_is_vendored_beside_the_pin_and_the_contract() {
    let listed: BTreeSet<String> = fs::read_dir(root().join("src/web_audit/vendored"))
        .expect("the vendored directory")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        listed,
        BTreeSet::from(["SITE_SHA".to_string(), "contract".to_string()])
    );
}

/// The crate carries what `build.rs` reads and leaves behind the corpus and the card
/// schema, whose field rules the site already wrote into `registry.json`.
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
    assert!(
        !listing.contains("server-card.schema.json"),
        "nothing reads the card schema:\n{listing}"
    );
}
