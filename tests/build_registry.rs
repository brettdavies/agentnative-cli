//! Tests for the web-audit registry codegen (`build_support/web_registry.rs`
//! and its two helpers). The build script and this driver both pull the
//! modules in via `#[path]`, so `cargo test` exercises the same code that
//! `cargo build` runs against the vendored inputs.

#[path = "../build_support/js_regex.rs"]
mod js_regex;
#[path = "../build_support/ts_consts.rs"]
mod ts_consts;
#[path = "../build_support/web_registry.rs"]
mod web_registry;

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use web_registry::{EVAL_RULES, HANDLER_KINDS, normalize_registry, normalize_remediation};

const TOP: &str = r#"version: 1
mcp_discovery:
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
    normalize_registry(&registry(checks), &ua_tokens())
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
    let reg = normalize_registry(&vendored("registry.yaml"), &consts.probe_ua_tokens)
        .expect("vendored registry normalizes");
    assert_eq!(reg.checks.len(), 65, "check count (bump deliberately)");
    assert_eq!(
        reg.category_order.len(),
        6,
        "category count (bump deliberately)"
    );
    let handlers: BTreeSet<&str> = reg.checks.iter().map(|c| c.handler.as_str()).collect();
    assert_eq!(handlers.len(), 11, "handler kinds (bump deliberately)");
    let evals: BTreeSet<&str> = reg
        .checks
        .iter()
        .filter_map(|c| c.eval.as_deref())
        .collect();
    assert_eq!(evals.len(), 2, "eval rules (bump deliberately)");
    assert!(
        reg.checks.iter().all(|c| c.handler_supported),
        "every vendored handler is ported"
    );
    assert!(
        reg.checks.iter().all(|c| c.eval_supported),
        "every vendored eval rule is ported"
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
    let reg = normalize_registry(&vendored("registry.yaml"), &consts.probe_ua_tokens)
        .expect("vendored registry normalizes");
    let ids: Vec<&str> = reg.checks.iter().map(|c| c.id.as_str()).collect();
    let rem = normalize_remediation(&vendored("remediation.yaml"), &ids)
        .expect("vendored remediation normalizes");
    assert_eq!(rem.len(), ids.len());
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
    assert_eq!(HANDLER_KINDS.len(), 11);
    assert_eq!(EVAL_RULES.len(), 2);
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
    )
    .expect_err("mismatch fails");
    assert!(e.contains("category_order"), "{e}");
}

#[test]
fn mcp_discovery_block_is_required() {
    let e = normalize_registry(
        &registry(CHECK_HTTP).replace("  protocol_version: \"2025-06-18\"\n", ""),
        &ua_tokens(),
    )
    .expect_err("missing protocol_version fails");
    assert!(e.contains("mcp_discovery"), "{e}");
}

// ---------------------------------------------------------------------------
// Remediation catalog (mirrors normalizeWebRemediation).
// ---------------------------------------------------------------------------

#[test]
fn remediation_missing_entry_fails_naming_id() {
    let e = normalize_remediation(REMEDIATION_ROBOTS, &["robots", "sitemap"])
        .expect_err("sitemap has no entry");
    assert!(
        e.contains("\"sitemap\"") && e.contains("no remediation entry"),
        "{e}"
    );
}

#[test]
fn remediation_orphan_fails_naming_id() {
    let e = normalize_remediation(REMEDIATION_ROBOTS, &[]).expect_err("robots is an orphan");
    assert!(e.contains("\"robots\"") && e.contains("orphan"), "{e}");
}

#[test]
fn remediation_entry_needs_string_fields() {
    let e = normalize_remediation(
        &REMEDIATION_ROBOTS.replace("    goal: Publish robots.txt\n", ""),
        &["robots"],
    )
    .expect_err("missing goal");
    assert!(e.contains("\"robots\"") && e.contains("goal"), "{e}");
}

#[test]
fn remediation_retired_fields_and_evidence_slots_fail() {
    let e = normalize_remediation(
        &REMEDIATION_ROBOTS.replace("    goal:", "    body: old\n    goal:"),
        &["robots"],
    )
    .expect_err("retired body field");
    assert!(e.contains("\"robots\"") && e.contains("body"), "{e}");
    let e = normalize_remediation(
        &REMEDIATION_ROBOTS.replace("Serve /robots.txt.", "{{evidence}}"),
        &["robots"],
    )
    .expect_err("evidence slot");
    assert!(e.contains("\"robots\"") && e.contains("evidence"), "{e}");
}

#[test]
fn remediation_resource_needs_label_and_absolute_url() {
    let e = normalize_remediation(
        &REMEDIATION_ROBOTS.replace("https://www.rfc-editor.org/rfc/rfc9309", "/rfc/rfc9309"),
        &["robots"],
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
    let rem = normalize_remediation(&two, &["robots", "alpha"]).expect("normalizes");
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
    let reg = normalize_registry(&vendored("registry.yaml"), &consts.probe_ua_tokens)
        .expect("normalizes");
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
    let rem = normalize_remediation(REMEDIATION_ROBOTS, &["robots"]).expect("normalizes");
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
    let rem = normalize_remediation(REMEDIATION_ROBOTS, &["robots"]).expect("normalizes");
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
