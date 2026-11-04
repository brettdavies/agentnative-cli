//! Tests for the web-audit registry codegen (`build_support/web_registry/` and the
//! regex translation). The build script and this driver both pull the modules in via
//! `#[path]`, so `cargo test` exercises the same code that `cargo build` runs against
//! the vendored contract.

#[path = "../build_support/js_regex.rs"]
mod js_regex;
#[path = "../build_support/web_registry/mod.rs"]
mod web_registry;

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use web_registry::{EVAL_RULES, HANDLER_KINDS, read_constants, read_registry, read_remediation};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn vendored(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/web_audit/vendored/contract")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn check() -> Value {
    json!({
        "id": "robots",
        "category": "api",
        "tier": "recommended",
        "keyword": "should",
        "principle": "P7",
        "site_types": ["all"],
        "antecedent": "none",
        "weight": 2,
        "title": "robots present",
        "breadcrumb": "robots.txt",
        "hint": "Publish robots.txt.",
        "handler": "http",
        "with": { "path": "/robots.txt", "expect": { "status": [200] } }
    })
}

fn registry_with(checks: Vec<Value>) -> Value {
    json!({
        "version": 1,
        "mcp_discovery": {
            "ai_catalog": "/.well-known/ai-catalog.json",
            "card_suffix": "/server-card",
            "well_known": ["/.well-known/mcp.json"],
            "common_paths": ["/mcp"],
            "protocol_version": "2025-06-18"
        },
        "category_order": ["mcp", "api"],
        "categories": { "api": "API", "mcp": "MCP" },
        "mcp_lanes": {
            "shared": { "label": "Every MCP server", "note": "shared" },
            "legacy": { "label": "Legacy lane", "note": "legacy" }
        },
        "alternatives": [{
            "group": "mcp-access",
            "variants": {
                "open": { "antecedents": [], "presented_by": ["mcp-session"] },
                "protected": {
                    "antecedents": ["mcp-auth-required"],
                    "presented_by": ["mcp-auth-required"]
                }
            }
        }],
        "retired": {
            "old-card": { "successor": "robots", "reason": "replaced" }
        },
        "checks": checks
    })
}

fn read(doc: &Value) -> Result<web_registry::NormalizedRegistry, String> {
    read_registry(&doc.to_string())
}

/// The error a registry with `edit` applied to its one check produces.
fn check_err(edit: impl FnOnce(&mut Value)) -> String {
    let mut one = check();
    edit(&mut one);
    read(&registry_with(vec![one])).expect_err("the build must refuse this check")
}

fn constants() -> Value {
    serde_json::from_str(&vendored("constants.json")).expect("constants.json parses")
}

fn emitted(reg: &web_registry::NormalizedRegistry) -> String {
    let consts = read_constants(&constants().to_string()).expect("constants read");
    web_registry::emit_rust(reg, &[], &consts, SHA, "056ec475cbcb\n")
}

// ---------------------------------------------------------------------------
// The vendored contract.
// ---------------------------------------------------------------------------

#[test]
fn the_vendored_contract_reads() {
    let reg = read_registry(&vendored("registry.json")).expect("registry.json reads");
    assert!(!reg.checks.is_empty());
    assert!(!reg.category_order.is_empty());
    let card = reg
        .checks
        .iter()
        .find(|c| c.handler == "server-card")
        .expect("the server-card check");
    let with: Value = serde_json::from_str(&card.with_json).unwrap();
    assert!(
        with["required"].as_array().is_some_and(|r| !r.is_empty())
            && with["remote_required"]
                .as_array()
                .is_some_and(|r| !r.is_empty()),
        "the server-card check carries the schema's required lists: {with}"
    );
    assert!(
        reg.checks.iter().all(|c| !c.with_json.contains("{ua:")),
        "every {{ua:...}} token expanded"
    );
    let rem = read_remediation(&vendored("remediation.json")).expect("remediation.json reads");
    assert_eq!(rem.len(), reg.checks.len() + reg.retired.len());
    let consts = read_constants(&vendored("constants.json")).expect("constants.json reads");
    assert!(consts.audit_user_agent.starts_with("anc-web-audit/"));
    assert!(!consts.probe_ua_tokens.is_empty());
}

#[test]
fn every_translated_registry_pattern_compiles() {
    let reg = read_registry(&vendored("registry.json")).expect("registry.json reads");
    let mut count = 0;
    for check in &reg.checks {
        for p in &check.patterns {
            regex::Regex::new(&p.rust).unwrap_or_else(|e| panic!("{}/{}: {e}", check.id, p.field));
            count += 1;
        }
    }
    assert!(
        count > 0,
        "expected the vendored registry to carry patterns"
    );
}

// ---------------------------------------------------------------------------
// Bindings and order.
// ---------------------------------------------------------------------------

#[test]
fn a_check_keeps_its_registry_position() {
    let mut zed = check();
    zed["id"] = json!("zed");
    let mut alpha = check();
    alpha["id"] = json!("alpha");
    let reg = read(&registry_with(vec![zed, alpha])).expect("reads");
    let ids: Vec<&str> = reg.checks.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        ["zed", "alpha"],
        "scorecard row order is registry order, never sorted"
    );
}

#[test]
fn maps_keep_the_contracts_display_order() {
    let reg = read(&registry_with(vec![check()])).expect("reads");
    let lanes: Vec<&str> = reg.mcp_lanes.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(lanes, ["shared", "legacy"], "not sorted by id");
    let slugs: Vec<&str> = reg.categories.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(slugs, ["mcp", "api"], "category_order, not key order");
    let variants: Vec<&str> = reg.alternatives[0]
        .variants
        .iter()
        .map(|v| v.name.as_str())
        .collect();
    assert_eq!(variants, ["open", "protected"]);
}

#[test]
fn unknown_handler_binds_unsupported_and_builds() {
    let mut one = check();
    one["handler"] = json!("frobnicate");
    let reg = read(&registry_with(vec![one])).expect("builds");
    assert_eq!(reg.checks[0].handler, "frobnicate");
    assert!(!reg.checks[0].handler_supported);
    let src = emitted(&reg);
    assert!(
        src.contains("HandlerBinding::Unsupported(\"frobnicate\")"),
        "{src}"
    );
}

#[test]
fn unknown_eval_binds_unsupported_and_builds() {
    let mut one = check();
    one["eval"] = json!("quux");
    let reg = read(&registry_with(vec![one])).expect("builds");
    assert_eq!(reg.checks[0].eval.as_deref(), Some("quux"));
    assert!(!reg.checks[0].eval_supported);
    let src = emitted(&reg);
    assert!(src.contains("EvalBinding::Unsupported(\"quux\")"), "{src}");
}

#[test]
fn known_handler_and_eval_bind_supported() {
    let mut one = check();
    one["eval"] = json!("legacy-alias-redirects");
    let reg = read(&registry_with(vec![one])).expect("builds");
    assert!(reg.checks[0].handler_supported);
    assert!(reg.checks[0].eval_supported);
    assert_eq!(HANDLER_KINDS.len(), 13);
    assert_eq!(EVAL_RULES.len(), 4);
}

// ---------------------------------------------------------------------------
// A contract this build cannot read stops it, naming the entry.
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_top_level_key_fails_naming_it() {
    let mut doc = registry_with(vec![check()]);
    doc["frob"] = json!(1);
    let e = read(&doc).expect_err("unknown key");
    assert!(e.contains("frob") && e.contains("registry.json"), "{e}");
}

#[test]
fn an_unknown_check_field_fails_naming_the_check() {
    let e = check_err(|c| c["frob"] = json!(1));
    assert!(e.contains("\"robots\"") && e.contains("frob"), "{e}");
}

#[test]
fn a_missing_check_field_fails_naming_the_check() {
    for field in ["tier", "title", "handler", "with", "weight"] {
        let e = check_err(|c| {
            c.as_object_mut().unwrap().remove(field);
        });
        assert!(
            e.contains("\"robots\"") && e.contains(field),
            "{field}: {e}"
        );
    }
}

#[test]
fn an_enumerated_value_this_build_does_not_know_fails_naming_the_check() {
    for (field, value) in [
        ("tier", json!("urgent")),
        ("keyword", json!("shall")),
        ("antecedent", json!("moon-phase")),
        ("site_types", json!(["spaceship"])),
        ("lane", json!("no-such-lane")),
    ] {
        let text = value.to_string();
        let e = check_err(|c| c[field] = value);
        let shown = text.trim_matches(|c| "[]\"".contains(c)).to_string();
        assert!(
            e.contains("\"robots\"") && e.contains(&shown),
            "{field}: {e}"
        );
    }
}

#[test]
fn a_with_value_a_ported_handler_could_only_misread_fails_naming_the_check() {
    let rule = json!([{ "field": "name", "type": ["string"] }]);
    for (what, handler, eval, with, named) in [
        (
            "an api op",
            "api-hygiene",
            None,
            json!({ "op": "cache-headers" }),
            "cache-headers",
        ),
        (
            "an llms op",
            "llms-txt-quality",
            None,
            json!({ "op": "sections" }),
            "sections",
        ),
        (
            "surface",
            "cors-preflight",
            None,
            json!({ "surface": "sideways" }),
            "sideways",
        ),
        (
            "no surface",
            "cors-preflight",
            None,
            json!({}),
            "with.surface",
        ),
        (
            "op",
            "protected-resource",
            None,
            json!({ "op": "dance" }),
            "dance",
        ),
        ("no op", "protected-resource", None, json!({}), "with.op"),
        (
            "retained",
            "http",
            Some("retained-document"),
            json!({ "retained": "no-such" }),
            "no-such",
        ),
        (
            "no retained",
            "http",
            Some("retained-document"),
            json!({}),
            "with.retained",
        ),
        (
            "a handler that cannot score a retained document",
            "mcp",
            Some("retained-document"),
            json!({ "retained": "server-card" }),
            "retained-document handler",
        ),
        (
            "a card with no eval",
            "server-card",
            None,
            json!({ "retained": "server-card" }),
            "needs eval",
        ),
        (
            "a card with no required",
            "server-card",
            Some("retained-document"),
            json!({ "retained": "server-card", "remote_required": rule }),
            "with.required",
        ),
        (
            "a card with no remote_required",
            "server-card",
            Some("retained-document"),
            json!({ "retained": "server-card", "required": rule }),
            "with.remote_required",
        ),
        (
            "a type no schema names",
            "server-card",
            Some("retained-document"),
            json!({ "retained": "server-card", "required": rule, "remote_required": [{ "field": "name", "type": ["String"] }] }),
            "with.remote_required",
        ),
        (
            "a rule with no types",
            "server-card",
            Some("retained-document"),
            json!({ "retained": "server-card", "required": rule, "remote_required": [{ "field": "name" }] }),
            "with.remote_required",
        ),
    ] {
        let e = check_err(|c| {
            c["handler"] = json!(handler);
            c["with"] = with;
            if let Some(eval) = eval {
                c["eval"] = json!(eval);
            }
        });
        assert!(e.contains("\"robots\"") && e.contains(named), "{what}: {e}");
    }
}

#[test]
fn a_with_a_ported_handler_can_read_builds() {
    let rule = json!([{ "field": "name", "type": ["string"] }]);
    for (handler, eval, with) in [
        ("cors-preflight", None, json!({ "surface": "actual" })),
        ("api-hygiene", None, json!({ "op": "rate-limit" })),
        ("llms-txt-quality", None, json!({ "op": "links" })),
        // Dispatch binds an unported kind before any handler reads `with`.
        (
            "card-lint",
            Some("retained-document"),
            json!({ "retained": "brand-new" }),
        ),
        ("server-card", Some("a-new-rule"), json!({})),
        ("protected-resource", None, json!({ "op": "metadata" })),
        (
            "http",
            Some("retained-document"),
            json!({ "retained": "api-catalog" }),
        ),
        (
            "server-card",
            Some("retained-document"),
            json!({ "retained": "server-card", "required": rule, "remote_required": rule }),
        ),
    ] {
        let mut one = check();
        one["handler"] = json!(handler);
        one["with"] = with;
        if let Some(eval) = eval {
            one["eval"] = json!(eval);
        }
        let result = read(&registry_with(vec![one]));
        assert!(result.is_ok(), "{handler}: {result:?}");
    }
}

#[test]
fn a_pattern_the_build_cannot_run_fails_naming_the_check() {
    for (pattern, needle) in [
        ("(?=a)", "lookaround"),
        ("(a)\\1", "backreference"),
        ("(", "compile"),
    ] {
        let e = check_err(|c| c["with"]["expect"] = json!({ "body_regex": pattern }));
        assert!(
            e.contains("\"robots\"") && e.contains(needle),
            "{pattern}: {e}"
        );
    }
}

#[test]
fn the_other_registry_blocks_are_strict_too() {
    let mut doc = registry_with(vec![check()]);
    doc["mcp_lanes"]["shared"]["frob"] = json!(1);
    let e = read(&doc).expect_err("unknown lane field");
    assert!(e.contains("mcp_lanes.shared") && e.contains("frob"), "{e}");

    let mut doc = registry_with(vec![check()]);
    doc["retired"]["old-card"]["frob"] = json!(1);
    let e = read(&doc).expect_err("unknown retired field");
    assert!(e.contains("retired.old-card") && e.contains("frob"), "{e}");

    let mut doc = registry_with(vec![check()]);
    doc["alternatives"][0]["variants"]["open"]["presented_by"] = json!(["moon-phase"]);
    let e = read(&doc).expect_err("unknown token");
    assert!(
        e.contains("alternatives.mcp-access.open") && e.contains("moon-phase"),
        "{e}"
    );

    let mut doc = registry_with(vec![check()]);
    doc["category_order"] = json!(["mcp", "nope"]);
    let e = read(&doc).expect_err("a category with no name");
    assert!(e.contains("category_order") && e.contains("nope"), "{e}");

    let mut doc = registry_with(vec![check()]);
    doc["mcp_discovery"]["frob"] = json!(1);
    let e = read(&doc).expect_err("unknown discovery field");
    assert!(e.contains("frob"), "{e}");
}

// ---------------------------------------------------------------------------
// The fix catalog.
// ---------------------------------------------------------------------------

#[test]
fn remediation_is_strict_and_emitted_sorted_by_id() {
    let doc = json!({
        "zed": { "title": "t", "goal": "g", "fix": "f" },
        "alpha": {
            "title": "t",
            "goal": "g",
            "fix": "f",
            "resources": [{ "label": "RFC", "url": "https://example.com" }]
        }
    });
    let rem = read_remediation(&doc.to_string()).expect("reads");
    let ids: Vec<&str> = rem.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "zed"]);
    assert_eq!(rem[0].resources[0].label, "RFC");
    assert!(rem[1].resources.is_empty());

    let doc = json!({ "zed": { "title": "t", "goal": "g", "fix": "f", "frob": 1 } });
    let e = read_remediation(&doc.to_string()).expect_err("unknown field");
    assert!(e.contains("\"zed\"") && e.contains("frob"), "{e}");
}

// ---------------------------------------------------------------------------
// Engine settings.
// ---------------------------------------------------------------------------

#[test]
fn every_constant_is_emitted_under_its_name_and_value() {
    let doc = constants();
    let src = emitted(&read(&registry_with(vec![check()])).expect("reads"));
    for (key, value) in doc.as_object().unwrap() {
        let Some(n) = value.as_u64() else {
            assert!(src.contains(&format!("pub const {key}:")), "{key}: {src}");
            continue;
        };
        let want = match key.strip_suffix("_MS") {
            Some(name) => format!(
                "pub const {name}: std::time::Duration = std::time::Duration::from_millis({n});"
            ),
            None => format!("pub const {key}: usize = {n};"),
        };
        assert!(src.contains(&want), "{want}: {src}");
    }
}

#[test]
fn a_missing_constant_fails_naming_it_and_the_sync_script() {
    let mut doc = constants();
    doc.as_object_mut().unwrap().remove("DEFAULT_CONCURRENCY");
    let e = read_constants(&doc.to_string()).expect_err("a missing key");
    assert!(
        e.contains("DEFAULT_CONCURRENCY") && e.contains("scripts/sync-web-audit.sh"),
        "{e}"
    );
}

#[test]
fn a_constant_nothing_reads_fails_naming_it() {
    let mut doc = constants();
    doc["SOME_NEW_SETTING"] = json!(1);
    let e = read_constants(&doc.to_string()).expect_err("an unbound key");
    assert!(e.contains("SOME_NEW_SETTING"), "{e}");
}

#[test]
fn a_constant_of_the_wrong_shape_fails_naming_it() {
    for (key, value) in [
        ("DEFAULT_CONCURRENCY", json!("six")),
        ("DEFAULT_TIMEOUT_MS", json!(-1)),
        ("AUDIT_USER_AGENT", json!(1)),
        ("PROBE_UA_TOKENS", json!({ "{ua:cli}": 1 })),
    ] {
        let mut doc = constants();
        doc[key] = value;
        let e = read_constants(&doc.to_string()).expect_err(key);
        assert!(e.contains(key), "{e}");
    }
}

#[test]
fn a_changed_setting_changes_what_the_build_emits() {
    let mut doc = constants();
    doc["DEFAULT_CONCURRENCY"] = json!(3);
    doc["DEFAULT_TIMEOUT_MS"] = json!(1234);
    let consts = read_constants(&doc.to_string()).expect("reads");
    let reg = read(&registry_with(vec![check()])).expect("reads");
    let src = web_registry::emit_rust(&reg, &[], &consts, SHA, "056ec475cbcb\n");
    assert!(
        src.contains("pub const DEFAULT_CONCURRENCY: usize = 3;"),
        "{src}"
    );
    assert!(src.contains("from_millis(1234)"), "{src}");
}

// ---------------------------------------------------------------------------
// JS -> Rust regex translation.
// ---------------------------------------------------------------------------

fn tr(p: &str, flags: &str) -> String {
    js_regex::translate(p, flags).unwrap_or_else(|e| panic!("{p}: {e}"))
}

#[test]
fn flags_become_inline_prefixes() {
    assert_eq!(tr("json", "i"), "(?i)json");
    assert_eq!(tr("^#", "im"), "(?imR)^#");
}

#[test]
fn ascii_classes_replace_js_shorthands() {
    assert_eq!(tr(r"\d+", "i"), "(?i)[0-9]+");
    assert_eq!(tr(r"\w\W", "i"), "(?i)[0-9A-Za-z_][^0-9A-Za-z_]");
    assert_eq!(tr(r"[\d-]", "i"), "(?i)[0-9-]");
    assert_eq!(tr(r"\bx\B", "i"), r"(?i)(?-u:\b)x(?-u:\B)");
    assert_eq!(tr(r"[\b]", "i"), r"(?i)[\x08]");
}

#[test]
fn dot_excludes_every_js_line_terminator() {
    assert_eq!(tr("a.b", "i"), r"(?i)a[^\n\r\u{2028}\u{2029}]b");
    assert_eq!(tr(r"a\.b", "i"), r"(?i)a\.b");
    assert_eq!(tr("[.]", "i"), "(?i)[.]");
}

#[test]
fn js_only_escapes_are_rewritten() {
    assert_eq!(tr(r"a\/b", "i"), "(?i)a/b");
    assert_eq!(tr(r"\0", "i"), r"(?i)\x00");
    assert_eq!(tr(r"\u2028", "i"), r"(?i)\u{2028}");
    assert_eq!(tr(r"\cJ", "i"), r"(?i)\x0A");
}

#[test]
fn lookaround_and_backreferences_are_rejected() {
    for p in ["(?=a)", "(?!a)", "(?<=a)", "(?<!a)"] {
        let e = js_regex::translate(p, "i").expect_err(p);
        assert!(e.contains("lookaround"), "{p}: {e}");
    }
    for p in [r"(a)\1", r"(?<n>a)\k<n>"] {
        let e = js_regex::translate(p, "i").expect_err(p);
        assert!(e.contains("backreference"), "{p}: {e}");
    }
}

#[test]
fn unknown_flags_are_rejected() {
    let e = js_regex::translate("a", "g").expect_err("g is not a flag assert.ts uses");
    assert!(e.contains("flag"), "{e}");
}

#[test]
fn translation_matches_js_on_representative_probes() {
    let re = regex::Regex::new(&tr(r"^\s*User-agent:\s*(\*|GPTBot)", "im")).unwrap();
    assert!(re.is_match("# robots\r\nuser-agent: *\r\n"));
    assert!(re.is_match("User-Agent: GPTBot"));
    assert!(!re.is_match("X-User-agent: *"));
    let re = regex::Regex::new(&tr("markdown|text/plain", "i")).unwrap();
    assert!(re.is_match("TEXT/PLAIN; charset=utf-8"));
    let re = regex::Regex::new(&tr(r#"<meta[^>]+name=["']description["']"#, "im")).unwrap();
    assert!(re.is_match("<META NAME='description' content='x'>"));
}

// ---------------------------------------------------------------------------
// Emission.
// ---------------------------------------------------------------------------

#[test]
fn emission_is_byte_stable_and_carries_every_table() {
    let reg = read(&registry_with(vec![check()])).expect("reads");
    let a = emitted(&reg);
    assert_eq!(a, emitted(&reg));
    for needle in [
        "pub const REGISTRY_VERSION: u64 = 1;",
        "pub const SITE_SHA: &str = \"0123456789abcdef0123456789abcdef01234567\";",
        "pub const AUDIT_USER_AGENT: &str = \"anc-web-audit/",
        "pub const PROBE_UA_TOKENS:",
        "pub const MCP_DISCOVERY:",
        "pub const CATEGORY_ORDER:",
        "pub const CATEGORIES:",
        "pub const MCP_LANES:",
        "pub const CHECKS:",
        "pub const ALTERNATIVES:",
        "pub const RETIRED_CHECK_IDS: &[&str] = &[\"old-card\"];",
        "pub const REMEDIATION:",
        "HandlerBinding::Http",
        "WebCheckKeyword::Should",
        "AntecedentToken::None",
    ] {
        assert!(a.contains(needle), "missing {needle:?} in:\n{a}");
    }
}

#[test]
fn the_contract_fingerprint_is_emitted_trimmed() {
    let src = emitted(&read(&registry_with(vec![check()])).expect("reads"));
    assert!(
        src.contains("pub const CONTRACT_FINGERPRINT: &str = \"056ec475cbcb\";"),
        "the file ends in a newline and the site's signal serves the value trimmed: {src}"
    );
}
