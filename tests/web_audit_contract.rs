//! The emitted tables against the contract they came from, row for row, and the
//! cross-field rules the site's normalizer owns.

#![cfg(feature = "web")]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use agentnative::web_audit::registry::{
    ALTERNATIVES, AUDIT_USER_AGENT, AntecedentToken, CATEGORIES, CATEGORY_ORDER, CHECKS,
    CONTRACT_FINGERPRINT, MCP_DISCOVERY, MCP_LANES, PROBE_UA_TOKENS, REGISTRY_VERSION, REMEDIATION,
    RETIRED_CHECK_IDS,
};
use regex::Regex;
use serde_json::{Map, Value};

/// What a report's trail has room for.
const BREADCRUMB_FITS: usize = 40;

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

/// The file ends in a newline and the site's version signal serves the value trimmed, so
/// a comparison reads this constant and never the file's bytes.
#[test]
fn the_contract_fingerprint_is_the_files_value_trimmed() {
    let raw = fs::read_to_string(root().join("src/web_audit/vendored/contract/FINGERPRINT"))
        .expect("FINGERPRINT");
    assert_ne!(raw, raw.trim(), "the file carries its trailing newline");
    assert_eq!(CONTRACT_FINGERPRINT, raw.trim());
    assert!(!CONTRACT_FINGERPRINT.is_empty());
    assert!(CONTRACT_FINGERPRINT.chars().all(|c| c.is_ascii_hexdigit()));
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
fn every_alternatives_group_offers_a_choice_of_designs_with_checks_behind_them() {
    let mut groups = BTreeSet::new();
    let mut claimed: BTreeMap<&str, String> = BTreeMap::new();
    let gates_a_check = |token: &AntecedentToken| CHECKS.iter().any(|c| c.antecedent == *token);
    for group in ALTERNATIVES {
        assert!(groups.insert(group.group), "{} twice", group.group);
        assert!(group.variants.len() >= 2, "{}", group.group);
        assert!(
            group
                .variants
                .iter()
                .any(|variant| variant.antecedents.iter().any(gates_a_check)),
            "{} has no design with checks of its own",
            group.group
        );
        for variant in group.variants {
            let name = format!("{}.{}", group.group, variant.name);
            assert!(
                !variant.presented_by.is_empty(),
                "{name} is presented by nothing"
            );
            for token in variant.antecedents.iter().chain(variant.presented_by) {
                assert!(
                    gates_a_check(token),
                    "{name}: {} gates no check",
                    token.as_str()
                );
                let holder = claimed
                    .entry(token.as_str())
                    .or_insert_with(|| name.clone());
                assert_eq!(*holder, name, "{} belongs to two designs", token.as_str());
            }
        }
    }
}

#[test]
fn every_check_is_well_formed() {
    let id = Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap();
    for check in CHECKS {
        assert!(id.is_match(check.id), "{}", check.id);
        assert!(check.weight > 0, "{}: a weight of nothing", check.id);
        assert!(
            !check.site_types.is_empty(),
            "{}: it would never apply",
            check.id
        );
        assert!(!check.title.is_empty(), "{}: no title", check.id);
        assert!(!check.hint.is_empty(), "{}: no hint", check.id);
        assert!(
            check.breadcrumb.chars().count() <= BREADCRUMB_FITS,
            "{}: the trail fits {BREADCRUMB_FITS} characters",
            check.id
        );
    }
}

#[test]
fn every_retired_id_says_why() {
    let doc = contract("registry.json");
    for (id, retired) in doc["retired"].as_object().unwrap() {
        assert!(!retired["reason"].as_str().unwrap().is_empty(), "{id}");
    }
}

/// `{{evidence}}` is the site's template slot, which the report would print as written.
#[test]
fn no_fix_holds_a_template_slot() {
    for fix in REMEDIATION {
        assert!(!fix.fix.contains("{{evidence}}"), "{}", fix.id);
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
