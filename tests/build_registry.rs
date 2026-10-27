//! Tests for the web-audit registry codegen (`build_support/web_registry/`
//! and its two helpers). The build script and this driver both pull the
//! modules in via `#[path]`, so `cargo test` exercises the same code that
//! `cargo build` runs against the vendored inputs.

#[path = "../build_support/js_regex.rs"]
mod js_regex;
#[path = "../build_support/ts_consts.rs"]
mod ts_consts;
#[path = "../build_support/web_registry/mod.rs"]
mod web_registry;
#[path = "../build_support/yaml.rs"]
mod yaml;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use web_registry::{EVAL_RULES, HANDLER_KINDS, normalize_registry, normalize_remediation};

const TOP: &str = r#"version: 1
mcp_discovery:
  ai_catalog: /.well-known/ai-catalog.json
  card_suffix: /server-card
  well_known: [/.well-known/mcp.json]
  common_paths: [/mcp]
  protocol_version: "2025-06-18"
category_order: [api, mcp]
categories:
  api: API
  mcp: MCP
checks:
"#;

const CHECK_HTTP: &str = r#"  - id: robots
    category: api
    tier: recommended
    principle: P7
    site_types: [all]
    antecedent: none
    weight: 2
    title: robots present
    breadcrumb: robots.txt
    handler: http
    with:
      path: /robots.txt
      expect: { status: [200] }
    hint: Publish robots.txt.
"#;

const REMEDIATION_ROBOTS: &str = r#"remediation:
  robots:
    title: robots present
    goal: Publish robots.txt
    fix: |
      Serve /robots.txt.
    resources:
      - { label: RFC 9309, url: "https://www.rfc-editor.org/rfc/rfc9309" }
"#;

fn registry(checks: &str) -> String {
    format!("{TOP}{checks}")
}

fn ua_tokens() -> Vec<(String, String)> {
    vec![
        ("{ua:cli}".to_string(), "curl/8.7.1".to_string()),
        (
            "{ua:ai-user-fetcher}".to_string(),
            "ChatGPT-User/1.0 (+https://openai.com/bot)".to_string(),
        ),
    ]
}

fn normalize(checks: &str) -> Result<web_registry::NormalizedRegistry, String> {
    normalize_registry(&registry(checks), &ua_tokens(), CARD_SCHEMA)
}

fn top_err(registry_text: &str) -> String {
    match normalize_registry(registry_text, &ua_tokens(), CARD_SCHEMA) {
        Ok(_) => panic!("expected a build error for:\n{registry_text}"),
        Err(e) => e,
    }
}

/// A minimal SEP-2127 extension schema: two required card fields, one
/// required remote field.
const CARD_SCHEMA: &str = r##"{
  "$defs": {
    "ServerCard": {
      "required": ["name", "version"],
      "properties": {
        "name": { "type": "string" },
        "version": { "type": ["string", "null"] },
        "remotes": { "type": "array", "items": { "$ref": "#/$defs/Remote" } }
      }
    },
    "Remote": {
      "required": ["url"],
      "properties": { "url": { "type": "string" } }
    }
  }
}"##;

const LANES: &str = "mcp_lanes:\n  shared:\n    label: Every MCP server\n    note: any protocol\n  legacy:\n    label: Legacy lane\n    note: initialize, then a session\n";

const CHECK_MCP: &str = r#"  - id: mcp-init
    category: mcp
    tier: required
    principle: P2
    site_types: [mcp]
    antecedent: mcp-session
    lane: legacy
    weight: 3
    title: initialize answers
    breadcrumb: initialize
    handler: mcp
    with: { op: initialize }
    hint: Answer initialize.
"#;

const CHECK_GATED: &str = r#"  - id: mcp-challenge
    category: mcp
    tier: recommended
    principle: P2
    site_types: [mcp]
    antecedent: mcp-auth-required
    lane: shared
    weight: 2
    title: challenge served
    breadcrumb: challenge
    handler: protected-resource
    with: { op: challenge }
    hint: Serve a challenge.
"#;

const CHECK_CARD: &str = r#"  - id: mcp-server-card
    category: mcp
    tier: recommended
    principle: P2
    site_types: [mcp]
    antecedent: mcp-present
    eval: retained-document
    lane: shared
    weight: 2
    title: server card complete
    breadcrumb: Server card
    handler: server-card
    with: { retained: server-card }
    hint: Publish a complete server card.
"#;

/// A registry carrying the lane map, the three MCP checks above, and `extra`
/// top-level text.
fn mcp_registry(extra: &str) -> String {
    format!("{LANES}{extra}{TOP}{CHECK_HTTP}{CHECK_MCP}{CHECK_GATED}{CHECK_CARD}")
}

fn err_of(checks: &str) -> String {
    match normalize(checks) {
        Ok(_) => panic!("expected a build error for:\n{checks}"),
        Err(e) => e,
    }
}

fn vendored(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/web_audit/vendored")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn vendored_registry(ua: &[(String, String)]) -> web_registry::NormalizedRegistry {
    normalize_registry(
        &vendored("registry.yaml"),
        ua,
        &vendored("server-card.schema.json"),
    )
    .expect("vendored registry normalizes")
}

// ---------------------------------------------------------------------------
// Happy path over the vendored inputs (counters carry deliberate-bump
// semantics, like the principle registry's).
// ---------------------------------------------------------------------------

#[test]
fn vendored_registry_counters() {
    let consts = ts_consts::resolve_site_constants(
        &vendored("user-agents.ts"),
        &vendored("site-url.ts"),
        &vendored("audit-routes.ts"),
    )
    .expect("site constants resolve");
    let reg = vendored_registry(&consts.probe_ua_tokens);
    assert_eq!(reg.checks.len(), 68, "check count (bump deliberately)");
    assert_eq!(
        reg.category_order.len(),
        6,
        "category count (bump deliberately)"
    );
    let handlers: BTreeSet<&str> = reg.checks.iter().map(|c| c.handler.as_str()).collect();
    assert_eq!(handlers.len(), 13, "handler kinds (bump deliberately)");
    let evals: BTreeSet<&str> = reg
        .checks
        .iter()
        .filter_map(|c| c.eval.as_deref())
        .collect();
    assert_eq!(evals.len(), 4, "eval rules (bump deliberately)");
    let unported_handlers: BTreeSet<&str> = reg
        .checks
        .iter()
        .filter(|c| !c.handler_supported)
        .map(|c| c.handler.as_str())
        .collect();
    assert_eq!(
        unported_handlers,
        BTreeSet::from(["protected-resource"]),
        "unported handler kinds (shrink as each port lands)"
    );
    let unported_evals: BTreeSet<&str> = reg
        .checks
        .iter()
        .filter(|c| !c.eval_supported)
        .filter_map(|c| c.eval.as_deref())
        .collect();
    assert_eq!(
        unported_evals,
        BTreeSet::from(["api-description"]),
        "unported eval rules (shrink as each port lands)"
    );
    assert_eq!(reg.mcp_lanes.len(), 4, "MCP lanes (bump deliberately)");
    assert_eq!(reg.retired.len(), 1, "retired ids (bump deliberately)");
    assert_eq!(
        reg.alternatives.len(),
        1,
        "alternative groups (bump deliberately)"
    );
    let card = reg
        .checks
        .iter()
        .find(|c| c.handler == "server-card")
        .expect("the server-card check");
    let with: serde_json::Value = serde_json::from_str(&card.with_json).unwrap();
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
    assert_eq!(
        consts.audit_user_agent,
        "anc-web-audit/1.0 (+https://anc.dev/audit)"
    );
}

#[test]
fn vendored_remediation_is_one_to_one() {
    let consts = ts_consts::resolve_site_constants(
        &vendored("user-agents.ts"),
        &vendored("site-url.ts"),
        &vendored("audit-routes.ts"),
    )
    .expect("site constants resolve");
    let reg = vendored_registry(&consts.probe_ua_tokens);
    let ids: Vec<&str> = reg.checks.iter().map(|c| c.id.as_str()).collect();
    let retired: Vec<&str> = reg.retired.iter().map(|r| r.id.as_str()).collect();
    let rem = normalize_remediation(&vendored("remediation.yaml"), &ids, &retired)
        .expect("vendored remediation normalizes");
    assert_eq!(rem.len(), ids.len() + retired.len());
}

#[test]
fn keyword_is_derived_from_tier() {
    let reg = normalize(
        &[
            CHECK_HTTP,
            &CHECK_HTTP
                .replace("id: robots", "id: two")
                .replace("tier: recommended", "tier: required"),
            &CHECK_HTTP
                .replace("id: robots", "id: three")
                .replace("tier: recommended", "tier: optional"),
        ]
        .concat(),
    )
    .expect("normalizes");
    let kw: Vec<&str> = reg.checks.iter().map(|c| c.keyword.as_str()).collect();
    assert_eq!(kw, ["should", "must", "may"]);
}

#[test]
fn checks_keep_registry_order() {
    let reg = normalize(
        &[
            CHECK_HTTP.replace("id: robots", "id: zed"),
            CHECK_HTTP.replace("id: robots", "id: alpha"),
        ]
        .concat(),
    )
    .expect("normalizes");
    let ids: Vec<&str> = reg.checks.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        ["zed", "alpha"],
        "scorecard row order is registry order, never sorted"
    );
}

#[test]
fn ua_token_expands_to_the_vendored_literal() {
    let reg = normalize(&CHECK_HTTP.replace(
        "      path: /robots.txt\n",
        "      path: /\n      headers: { User-Agent: \"{ua:cli}\", Accept: \"*/*\" }\n",
    ))
    .expect("normalizes");
    let with: serde_json::Value =
        serde_json::from_str(&reg.checks[0].with_json).expect("with is JSON");
    assert_eq!(with["headers"]["User-Agent"], "curl/8.7.1");
    assert_eq!(with["headers"]["Accept"], "*/*");
}

#[test]
fn unknown_handler_binds_unsupported_and_builds() {
    let reg =
        normalize(&CHECK_HTTP.replace("handler: http", "handler: frobnicate")).expect("builds");
    assert_eq!(reg.checks[0].handler, "frobnicate");
    assert!(!reg.checks[0].handler_supported);
}

#[test]
fn unknown_eval_binds_unsupported_and_builds() {
    let reg =
        normalize(&CHECK_HTTP.replace("    weight: 2\n", "    eval: frobnicate\n    weight: 2\n"))
            .expect("builds");
    assert_eq!(reg.checks[0].eval.as_deref(), Some("frobnicate"));
    assert!(!reg.checks[0].eval_supported);
}

#[test]
fn known_handler_and_eval_bind_supported() {
    let reg = normalize(&CHECK_HTTP.replace(
        "    weight: 2\n",
        "    eval: legacy-alias-redirects\n    weight: 2\n",
    ))
    .expect("builds");
    assert!(reg.checks[0].handler_supported);
    assert!(reg.checks[0].eval_supported);
    assert_eq!(HANDLER_KINDS.len(), 12);
    assert_eq!(EVAL_RULES.len(), 3);
}

// ---------------------------------------------------------------------------
// Structural failures, each naming the entry id (mirrors 13-web-audit-registry.mjs).
// ---------------------------------------------------------------------------

#[test]
fn hand_authored_keyword_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace("    weight: 2\n", "    keyword: should\n    weight: 2\n"));
    assert!(e.contains("\"robots\"") && e.contains("keyword"), "{e}");
}

#[test]
fn invalid_tier_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace("tier: recommended", "tier: bogus"));
    assert!(e.contains("\"robots\"") && e.contains("tier"), "{e}");
}

#[test]
fn missing_tier_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace("    tier: recommended\n", ""));
    assert!(e.contains("\"robots\"") && e.contains("tier"), "{e}");
}

#[test]
fn malformed_with_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace(
        "    with:\n      path: /robots.txt\n      expect: { status: [200] }\n",
        "    with: nope\n",
    ));
    assert!(e.contains("\"robots\"") && e.contains("with"), "{e}");
}

#[test]
fn missing_with_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace(
        "    with:\n      path: /robots.txt\n      expect: { status: [200] }\n",
        "",
    ));
    assert!(e.contains("\"robots\"") && e.contains("with"), "{e}");
}

#[test]
fn unknown_ua_token_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace(
        "      path: /robots.txt\n",
        "      path: /\n      headers: { user-agent: \"{ua:nope}\" }\n",
    ));
    assert!(e.contains("\"robots\"") && e.contains("{ua:nope}"), "{e}");
}

#[test]
fn literal_user_agent_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace(
        "      path: /robots.txt\n",
        "      path: /\n      headers: { User-Agent: \"curl/1.0\" }\n",
    ));
    assert!(e.contains("\"robots\"") && e.contains("curl/1.0"), "{e}");
}

#[test]
fn lookaround_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace(
        "expect: { status: [200] }",
        "expect: { header_regex: { name: vary, pattern: \"(?=.*accept(?!-))(?=.*user-agent)\" } }",
    ));
    assert!(e.contains("\"robots\"") && e.contains("lookaround"), "{e}");
}

#[test]
fn backreference_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace(
        "expect: { status: [200] }",
        "expect: { body_regex: \"(a)\\\\1\" }",
    ));
    assert!(
        e.contains("\"robots\"") && e.contains("backreference"),
        "{e}"
    );
}

#[test]
fn pattern_the_regex_crate_rejects_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace(
        "expect: { status: [200] }",
        "expect: { body_regex: \"a{2,1}\" }",
    ));
    assert!(e.contains("\"robots\"") && e.contains("body_regex"), "{e}");
}

#[test]
fn duplicate_id_fails_naming_id() {
    let e = err_of(&[CHECK_HTTP, CHECK_HTTP].concat());
    assert!(e.contains("duplicate") && e.contains("\"robots\""), "{e}");
}

#[test]
fn id_grammar_is_enforced() {
    let e = err_of(&CHECK_HTTP.replace("id: robots", "id: Robots_TXT"));
    assert!(e.contains("Robots_TXT"), "{e}");
}

#[test]
fn unknown_category_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace("category: api", "category: nope"));
    assert!(e.contains("\"robots\"") && e.contains("category"), "{e}");
}

#[test]
fn invalid_principle_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace("principle: P7", "principle: P9"));
    assert!(e.contains("\"robots\"") && e.contains("principle"), "{e}");
}

#[test]
fn retired_applies_to_fails_naming_id() {
    let e =
        err_of(&CHECK_HTTP.replace("    weight: 2\n", "    applies_to: [all]\n    weight: 2\n"));
    assert!(e.contains("\"robots\"") && e.contains("applies_to"), "{e}");
}

#[test]
fn site_types_must_be_non_empty_and_known() {
    let e = err_of(&CHECK_HTTP.replace("site_types: [all]", "site_types: []"));
    assert!(e.contains("\"robots\"") && e.contains("site_types"), "{e}");
    let e = err_of(&CHECK_HTTP.replace("site_types: [all]", "site_types: [web]"));
    assert!(e.contains("\"robots\"") && e.contains("web"), "{e}");
}

#[test]
fn unknown_antecedent_fails_naming_id() {
    let e = err_of(&CHECK_HTTP.replace("antecedent: none", "antecedent: moon-phase"));
    assert!(e.contains("\"robots\"") && e.contains("moon-phase"), "{e}");
}

#[test]
fn weight_must_be_a_positive_integer() {
    let e = err_of(&CHECK_HTTP.replace("weight: 2", "weight: 0"));
    assert!(e.contains("\"robots\"") && e.contains("weight"), "{e}");
    let e = err_of(&CHECK_HTTP.replace("weight: 2", "weight: 1.5"));
    assert!(e.contains("\"robots\"") && e.contains("weight"), "{e}");
}

#[test]
fn title_hint_and_breadcrumb_are_required() {
    for field in [
        "title: robots present",
        "hint: Publish robots.txt.",
        "breadcrumb: robots.txt",
    ] {
        let e = err_of(&CHECK_HTTP.replace(&format!("    {field}\n"), ""));
        let name = field.split(':').next().unwrap();
        assert!(e.contains("\"robots\"") && e.contains(name), "{field}: {e}");
    }
}

#[test]
fn breadcrumb_over_forty_chars_fails_naming_id() {
    let long = "x".repeat(41);
    let e = err_of(&CHECK_HTTP.replace("breadcrumb: robots.txt", &format!("breadcrumb: {long}")));
    assert!(
        e.contains("\"robots\"") && e.contains("breadcrumb") && e.contains("41"),
        "{e}"
    );
}

#[test]
fn cors_preflight_needs_a_surface() {
    let e = err_of(&CHECK_HTTP.replace("handler: http", "handler: cors-preflight"));
    assert!(e.contains("\"robots\"") && e.contains("surface"), "{e}");
}

#[test]
fn category_order_must_list_every_category_once() {
    let e = normalize_registry(
        &registry(CHECK_HTTP).replace("category_order: [api, mcp]", "category_order: [api]"),
        &ua_tokens(),
        CARD_SCHEMA,
    )
    .expect_err("mismatch fails");
    assert!(e.contains("category_order"), "{e}");
}

#[test]
fn mcp_discovery_block_is_required() {
    let e = normalize_registry(
        &registry(CHECK_HTTP).replace("  protocol_version: \"2025-06-18\"\n", ""),
        &ua_tokens(),
        CARD_SCHEMA,
    )
    .expect_err("missing protocol_version fails");
    assert!(e.contains("mcp_discovery"), "{e}");
}

// ---------------------------------------------------------------------------
// YAML loading: YAML 1.2 typing, and input with no JSON form fails the build.
// ---------------------------------------------------------------------------

#[test]
fn yaml_1_1_boolean_words_stay_strings() {
    let reg = normalize(
        &CHECK_HTTP
            .replace("title: robots present", "title: yes")
            .replace("hint: Publish robots.txt.", "hint: off"),
    )
    .expect("normalizes");
    assert_eq!(reg.checks[0].title, "yes");
    assert_eq!(reg.checks[0].hint, "off");
}

#[test]
fn merge_keys_stay_ordinary_and_non_finite_floats_load_as_null() {
    let reg = normalize(&CHECK_HTTP.replace(
        "      path: /robots.txt\n",
        "      path: /robots.txt\n      <<: { extra: 1 }\n      ratio: .inf\n",
    ))
    .expect("normalizes");
    let with: serde_json::Value =
        serde_json::from_str(&reg.checks[0].with_json).expect("with is JSON");
    assert_eq!(with["<<"]["extra"], 1);
    assert!(with.get("extra").is_none(), "{with}");
    assert!(with["ratio"].is_null(), "{with}");
}

#[test]
fn non_string_mapping_keys_fail() {
    let category_key = normalize_registry(
        &registry(CHECK_HTTP).replace("  mcp: MCP\n", "  1: MCP\n"),
        &ua_tokens(),
        CARD_SCHEMA,
    )
    .expect_err("integer categories key");
    let with_key = err_of(&CHECK_HTTP.replace("expect: { status: [200] }", "expect: { 200: ok }"));
    let remediation_key = normalize_remediation(
        &REMEDIATION_ROBOTS.replace("  robots:", "  7:"),
        &["robots"],
        &[],
    )
    .expect_err("integer remediation key");
    for e in [category_key, with_key, remediation_key] {
        assert!(
            e.contains("YAML parse error") && e.contains("expected a string mapping key"),
            "{e}"
        );
    }
}

#[test]
fn yaml_tags_fail() {
    let e = err_of(&CHECK_HTTP.replace(
        "expect: { status: [200] }",
        "expect: !custom { status: [200] }",
    ));
    assert!(
        e.contains("YAML parse error") && e.contains("unsupported tag"),
        "{e}"
    );
}

// ---------------------------------------------------------------------------
// Remediation catalog (mirrors normalizeWebRemediation).
// ---------------------------------------------------------------------------

#[test]
fn remediation_missing_entry_fails_naming_id() {
    let e = normalize_remediation(REMEDIATION_ROBOTS, &["robots", "sitemap"], &[])
        .expect_err("sitemap has no entry");
    assert!(
        e.contains("\"sitemap\"") && e.contains("no remediation entry"),
        "{e}"
    );
}

#[test]
fn remediation_orphan_fails_naming_id() {
    let e = normalize_remediation(REMEDIATION_ROBOTS, &[], &[]).expect_err("robots is an orphan");
    assert!(e.contains("\"robots\"") && e.contains("orphan"), "{e}");
}

#[test]
fn remediation_entry_needs_string_fields() {
    let e = normalize_remediation(
        &REMEDIATION_ROBOTS.replace("    goal: Publish robots.txt\n", ""),
        &["robots"],
        &[],
    )
    .expect_err("missing goal");
    assert!(e.contains("\"robots\"") && e.contains("goal"), "{e}");
}

#[test]
fn remediation_retired_fields_and_evidence_slots_fail() {
    let e = normalize_remediation(
        &REMEDIATION_ROBOTS.replace("    goal:", "    body: old\n    goal:"),
        &["robots"],
        &[],
    )
    .expect_err("retired body field");
    assert!(e.contains("\"robots\"") && e.contains("body"), "{e}");
    let e = normalize_remediation(
        &REMEDIATION_ROBOTS.replace("Serve /robots.txt.", "{{evidence}}"),
        &["robots"],
        &[],
    )
    .expect_err("evidence slot");
    assert!(e.contains("\"robots\"") && e.contains("evidence"), "{e}");
}

#[test]
fn remediation_resource_needs_label_and_absolute_url() {
    let e = normalize_remediation(
        &REMEDIATION_ROBOTS.replace("https://www.rfc-editor.org/rfc/rfc9309", "/rfc/rfc9309"),
        &["robots"],
        &[],
    )
    .expect_err("relative url");
    assert!(
        e.contains("\"robots\"") && e.contains("absolute url"),
        "{e}"
    );
}

#[test]
fn remediation_is_emitted_sorted_by_id() {
    let two = format!("{REMEDIATION_ROBOTS}  alpha:\n    title: a\n    goal: g\n    fix: f\n");
    let rem = normalize_remediation(&two, &["robots", "alpha"], &[]).expect("normalizes");
    let ids: Vec<&str> = rem.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "robots"]);
}

// ---------------------------------------------------------------------------
// Site constants parsed from the vendored TS modules.
// ---------------------------------------------------------------------------

const UA_TS: &str = r#"
import { AUDIT_PATH } from './audit-routes';
import { CANONICAL_SITE_URL } from './site-url';
export const AUDIT_USER_AGENT = `anc-web-audit/1.0 (+${CANONICAL_SITE_URL}${AUDIT_PATH})`;
export const AI_USER_FETCHER_PROBE_UA = 'ChatGPT-User/1.0 (+https://openai.com/bot)';
export const CLI_PROBE_UA = 'curl/8.7.1';
export const PROBE_UA_TOKENS = Object.freeze({
  '{ua:ai-user-fetcher}': AI_USER_FETCHER_PROBE_UA,
  '{ua:cli}': CLI_PROBE_UA,
});
"#;
const SITE_URL_TS: &str = "export const CANONICAL_SITE_URL = 'https://anc.dev';\n";
const ROUTES_TS: &str =
    "export const SCORING_PATH = '/scoring';\nexport const AUDIT_PATH = '/audit';\n";

#[test]
fn site_constants_resolve_from_the_three_modules() {
    let c = ts_consts::resolve_site_constants(UA_TS, SITE_URL_TS, ROUTES_TS).expect("resolves");
    assert_eq!(
        c.audit_user_agent,
        "anc-web-audit/1.0 (+https://anc.dev/audit)"
    );
    assert_eq!(
        c.probe_ua_tokens,
        vec![
            (
                "{ua:ai-user-fetcher}".to_string(),
                "ChatGPT-User/1.0 (+https://openai.com/bot)".to_string()
            ),
            ("{ua:cli}".to_string(), "curl/8.7.1".to_string()),
        ]
    );
}

#[test]
fn missing_site_constant_fails_naming_it() {
    let e = ts_consts::resolve_site_constants(
        UA_TS,
        SITE_URL_TS,
        "export const SCORING_PATH = '/scoring';\n",
    )
    .expect_err("AUDIT_PATH missing");
    assert!(e.contains("AUDIT_PATH"), "{e}");
    let e = ts_consts::resolve_site_constants(
        &UA_TS.replace("export const CLI_PROBE_UA", "const CLI_PROBE_UA"),
        SITE_URL_TS,
        ROUTES_TS,
    )
    .expect_err("token target missing");
    assert!(e.contains("CLI_PROBE_UA"), "{e}");
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
fn every_translated_registry_pattern_compiles() {
    let consts = ts_consts::resolve_site_constants(
        &vendored("user-agents.ts"),
        &vendored("site-url.ts"),
        &vendored("audit-routes.ts"),
    )
    .expect("site constants resolve");
    let reg = vendored_registry(&consts.probe_ua_tokens);
    let mut count = 0;
    for check in &reg.checks {
        for p in &check.patterns {
            regex::Regex::new(&p.rust).unwrap_or_else(|e| panic!("{}/{}: {e}", check.id, p.field));
            count += 1;
        }
    }
    assert!(
        count > 20,
        "expected the vendored registry to carry patterns, got {count}"
    );
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
    let consts =
        ts_consts::resolve_site_constants(UA_TS, SITE_URL_TS, ROUTES_TS).expect("resolves");
    let reg = normalize(CHECK_HTTP).expect("normalizes");
    let rem = normalize_remediation(REMEDIATION_ROBOTS, &["robots"], &[]).expect("normalizes");
    let a = web_registry::emit_rust(
        &reg,
        &rem,
        &consts,
        "0123456789abcdef0123456789abcdef01234567",
    );
    let b = web_registry::emit_rust(
        &reg,
        &rem,
        &consts,
        "0123456789abcdef0123456789abcdef01234567",
    );
    assert_eq!(a, b);
    for needle in [
        "pub const REGISTRY_VERSION: u64 = 1;",
        "pub const SITE_SHA: &str = \"0123456789abcdef0123456789abcdef01234567\";",
        "pub const AUDIT_USER_AGENT: &str = \"anc-web-audit/1.0 (+https://anc.dev/audit)\";",
        "pub const PROBE_UA_TOKENS:",
        "pub const MCP_DISCOVERY:",
        "pub const CATEGORY_ORDER:",
        "pub const CATEGORIES:",
        "pub const CHECKS:",
        "pub const REMEDIATION:",
        "HandlerBinding::Http",
        "WebCheckKeyword::Should",
        "AntecedentToken::None",
    ] {
        assert!(a.contains(needle), "missing {needle:?} in:\n{a}");
    }
}

#[test]
fn unsupported_bindings_are_emitted_with_their_name() {
    let consts =
        ts_consts::resolve_site_constants(UA_TS, SITE_URL_TS, ROUTES_TS).expect("resolves");
    let reg = normalize(
        &CHECK_HTTP
            .replace("handler: http", "handler: frobnicate")
            .replace("    weight: 2\n", "    eval: quux\n    weight: 2\n"),
    )
    .expect("builds");
    let rem = normalize_remediation(REMEDIATION_ROBOTS, &["robots"], &[]).expect("normalizes");
    let src = web_registry::emit_rust(
        &reg,
        &rem,
        &consts,
        "0123456789abcdef0123456789abcdef01234567",
    );
    assert!(
        src.contains("HandlerBinding::Unsupported(\"frobnicate\")"),
        "{src}"
    );
    assert!(src.contains("EvalBinding::Unsupported(\"quux\")"), "{src}");
}

// ---------------------------------------------------------------------------
// Registry blocks beyond `checks`: lanes, retired ids, alternatives, the
// discovery paths, and the server-card schema splice.
// ---------------------------------------------------------------------------

#[test]
fn the_mcp_blocks_normalize() {
    let reg = normalize_registry(&mcp_registry(""), &ua_tokens(), CARD_SCHEMA).expect("normalizes");
    assert_eq!(reg.mcp_discovery.ai_catalog, "/.well-known/ai-catalog.json");
    assert_eq!(reg.mcp_discovery.card_suffix, "/server-card");
    let lanes: Vec<&str> = reg.mcp_lanes.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(lanes, ["shared", "legacy"]);
    assert_eq!(reg.checks[0].lane, None, "a non-mcp check has no lane");
    assert_eq!(reg.checks[1].lane.as_deref(), Some("legacy"));
}

#[test]
fn unknown_top_level_key_fails_naming_it() {
    let e = top_err(&mcp_registry("frobs: 1\n"));
    assert!(e.contains("\"frobs\"") && e.contains("top-level"), "{e}");
}

#[test]
fn unknown_check_field_fails_naming_the_check() {
    let e = err_of(&CHECK_HTTP.replace("    weight: 2\n", "    weight: 2\n    colour: red\n"));
    assert!(e.contains("\"robots\"") && e.contains("\"colour\""), "{e}");
}

#[test]
fn discovery_paths_are_required_and_rooted_and_unknown_keys_fail() {
    for (from, to, needle) in [
        (
            "  ai_catalog: /.well-known/ai-catalog.json\n",
            "",
            "mcp_discovery.ai_catalog",
        ),
        (
            "  card_suffix: /server-card\n",
            "  card_suffix: server-card\n",
            "mcp_discovery.card_suffix",
        ),
        (
            "  card_suffix: /server-card\n",
            "  card_suffix: /server-card\n  extra: 1\n",
            "\"extra\"",
        ),
    ] {
        let e = top_err(&mcp_registry("").replace(from, to));
        assert!(e.contains(needle), "{needle}: {e}");
    }
}

#[test]
fn mcp_lane_entries_need_label_and_note_and_nothing_else() {
    for (from, to, needle) in [
        (
            "    label: Legacy lane\n",
            "",
            "needs a string label and note",
        ),
        (
            "    note: any protocol\n",
            "    note: any protocol\n    colour: red\n",
            "\"colour\"",
        ),
    ] {
        let e = top_err(&mcp_registry("").replace(from, to));
        assert!(
            e.contains("mcp_lanes") && e.contains(needle),
            "{needle}: {e}"
        );
    }
}

#[test]
fn lane_is_required_on_mcp_checks_and_refused_elsewhere() {
    let missing = top_err(&mcp_registry("").replace("    lane: legacy\n", ""));
    assert!(
        missing.contains("\"mcp-init\"") && missing.contains("needs a lane"),
        "{missing}"
    );
    let unknown = top_err(&mcp_registry("").replace("lane: legacy", "lane: express"));
    assert!(
        unknown.contains("\"mcp-init\"") && unknown.contains("\"express\""),
        "{unknown}"
    );
    let stray = err_of(&CHECK_HTTP.replace("    weight: 2\n", "    weight: 2\n    lane: legacy\n"));
    assert!(
        stray.contains("\"robots\"") && stray.contains("lane"),
        "{stray}"
    );
}

const RETIRED: &str =
    "retired:\n  old-card:\n    successor: mcp-server-card\n    reason: replaced\n";

#[test]
fn retired_ids_normalize_and_validate() {
    let reg =
        normalize_registry(&mcp_registry(RETIRED), &ua_tokens(), CARD_SCHEMA).expect("normalizes");
    assert_eq!(reg.retired.len(), 1);
    assert_eq!(reg.retired[0].id, "old-card");
    assert_eq!(reg.retired[0].successor, "mcp-server-card");
    for (from, to, needle) in [
        ("  old-card:", "  robots:", "is still a registry check"),
        (
            "successor: mcp-server-card",
            "successor: gone",
            "not a registry check",
        ),
        ("    reason: replaced\n", "", "needs a reason"),
        (
            "    reason: replaced\n",
            "    reason: replaced\n    since: 2026\n",
            "\"since\"",
        ),
        ("  old-card:", "  Old_Card:", "must match"),
    ] {
        let e = top_err(&mcp_registry(&RETIRED.replace(from, to)));
        assert!(e.contains("retired") && e.contains(needle), "{needle}: {e}");
    }
}

const ALTERNATIVES: &str = "alternatives:\n  - group: mcp-access\n    variants:\n      open: { presented_by: [mcp-session] }\n      protected: { antecedents: [mcp-auth-required] }\n";

#[test]
fn alternatives_normalize_and_validate() {
    let reg = normalize_registry(&mcp_registry(ALTERNATIVES), &ua_tokens(), CARD_SCHEMA)
        .expect("normalizes");
    let group = &reg.alternatives[0];
    assert_eq!(group.group, "mcp-access");
    assert_eq!(group.variants[0].name, "open");
    assert!(group.variants[0].antecedents.is_empty());
    assert_eq!(group.variants[0].presented_by, ["mcp-session"]);
    assert_eq!(
        group.variants[1].presented_by,
        ["mcp-auth-required"],
        "presented_by defaults to antecedents"
    );
    for (from, to, needle) in [
        (
            "      open: { presented_by: [mcp-session] }\n",
            "",
            "at least two variants",
        ),
        ("[mcp-session]", "[mcp-sessions]", "unknown antecedent"),
        ("[mcp-session]", "[mcp-session], colour: red", "\"colour\""),
        ("[mcp-session]", "[auth-present]", "presented by no check"),
        (
            "{ antecedents: [mcp-auth-required] }",
            "{ presented_by: [mcp-auth-required] }",
            "no variant with checks of its own",
        ),
        (
            "{ antecedents: [mcp-auth-required] }",
            "{ antecedents: [mcp-session] }",
            "belongs to both",
        ),
        (
            "    variants:",
            "    colour: red\n    variants:",
            "\"colour\"",
        ),
        ("group: mcp-access", "group: MCP", "must match"),
    ] {
        let e = top_err(&mcp_registry(&ALTERNATIVES.replace(from, to)));
        assert!(
            e.contains("alternatives") || e.contains("antecedent"),
            "{needle}: {e}"
        );
        assert!(e.contains(needle), "{needle}: {e}");
    }
}

#[test]
fn the_server_card_check_takes_its_required_lists_from_the_schema() {
    let reg = normalize_registry(&mcp_registry(""), &ua_tokens(), CARD_SCHEMA).expect("normalizes");
    let card = reg
        .checks
        .iter()
        .find(|c| c.id == "mcp-server-card")
        .unwrap();
    let with: serde_json::Value = serde_json::from_str(&card.with_json).unwrap();
    assert_eq!(
        with,
        serde_json::json!({
            "retained": "server-card",
            "required": [
                { "field": "name", "type": ["string"] },
                { "field": "version", "type": ["string", "null"] }
            ],
            "remote_required": [{ "field": "url", "type": ["string"] }]
        })
    );
    assert!(card.handler_supported, "server-card is ported");
}

#[test]
fn server_card_and_retained_document_rules_fail_naming_the_check() {
    for (from, to, needle) in [
        (
            "with: { retained: server-card }",
            "with: { retained: server-card, required: [] }",
            "hand-authors with.required",
        ),
        (
            "with: { retained: server-card }",
            "with: { retained: api-catalog }",
            "with.retained \"server-card\"",
        ),
        (
            "with: { retained: server-card }",
            "with: { retained: sitemap }",
            "must name a document discovery keeps",
        ),
        (
            "    eval: retained-document\n    lane: shared",
            "    lane: shared",
            "needs eval retained-document",
        ),
    ] {
        let e = top_err(&mcp_registry("").replace(from, to));
        assert!(
            e.contains("\"mcp-server-card\"") && e.contains(needle),
            "{needle}: {e}"
        );
    }
    let wrong_handler = err_of(
        &CHECK_HTTP
            .replace(
                "    weight: 2\n",
                "    eval: retained-document\n    weight: 2\n",
            )
            .replace(
                "      path: /robots.txt\n",
                "      path: /robots.txt\n      retained: api-catalog\n",
            )
            .replace("handler: http", "handler: dns-doh"),
    );
    assert!(
        wrong_handler.contains("\"robots\"") && wrong_handler.contains("retained document"),
        "{wrong_handler}"
    );
}

#[test]
fn a_malformed_card_schema_fails_only_when_a_card_check_needs_it() {
    normalize_registry(&registry(CHECK_HTTP), &ua_tokens(), "not json")
        .expect("no card check, no read");
    let e = normalize_registry(&mcp_registry(""), &ua_tokens(), "not json")
        .expect_err("card check reads it");
    assert!(e.contains("server card schema is not JSON"), "{e}");
    let e = normalize_registry(&mcp_registry(""), &ua_tokens(), r#"{"$defs":{}}"#)
        .expect_err("no ServerCard");
    assert!(e.contains("$defs.ServerCard"), "{e}");
    let untyped = CARD_SCHEMA.replace(r#""url": { "type": "string" }"#, r#""url": {}"#);
    let e =
        normalize_registry(&mcp_registry(""), &ua_tokens(), &untyped).expect_err("untyped field");
    assert!(
        e.contains("remotes[].url") && e.contains("no JSON type"),
        "{e}"
    );
}

#[test]
fn protected_resource_needs_a_known_op() {
    let e = top_err(&mcp_registry("").replace("with: { op: challenge }", "with: { op: peek }"));
    assert!(
        e.contains("\"mcp-challenge\"") && e.contains("with.op"),
        "{e}"
    );
}
