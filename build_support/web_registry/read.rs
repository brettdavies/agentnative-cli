//! The strict read of the contract's `registry.json` and `remediation.json`.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::patterns::collect_patterns;
use super::vocab::{
    ANTECEDENTS, CARD_FIELD_PARAMS, CORS_SURFACES, EVAL_RULES, HANDLER_KINDS,
    PROTECTED_RESOURCE_OPS, RETAINED_DOCUMENT_HANDLERS, RETAINED_DOCUMENT_KEYS, SITE_TYPES, TIERS,
};
use super::{
    AlternativeGroup, AlternativeVariant, McpDiscovery, McpLane, NormalizedCheck,
    NormalizedRegistry, RemediationEntry, RetiredCheck,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u64,
    mcp_discovery: McpDiscovery,
    category_order: Vec<String>,
    categories: Map<String, Value>,
    mcp_lanes: Map<String, Value>,
    alternatives: Vec<RawGroup>,
    retired: Map<String, Value>,
    checks: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGroup {
    group: String,
    variants: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCheck {
    id: String,
    category: String,
    tier: String,
    keyword: String,
    principle: String,
    site_types: Vec<String>,
    antecedent: String,
    eval: Option<String>,
    weight: u64,
    title: String,
    breadcrumb: String,
    hint: String,
    lane: Option<String>,
    handler: String,
    with: Map<String, Value>,
}

fn entry<T: for<'de> Deserialize<'de>>(what: &str, value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| format!("registry.json: {what}: {e}"))
}

fn known(id: &str, field: &str, value: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "registry.json: check \"{id}\": {field} \"{value}\" is not one this build knows ({})",
            allowed.join(", ")
        ))
    }
}

/// Whether a server-card field rule is one the handler can apply: a field name and
/// the types that field may hold.
fn is_field_rule(rule: &Value) -> bool {
    rule.get("field").is_some_and(Value::is_string)
        && rule
            .get("type")
            .and_then(Value::as_array)
            .is_some_and(|types| !types.is_empty() && types.iter().all(Value::is_string))
}

/// What a ported handler reads from `with` it has no way to report on: a value it does
/// not know would read as a missing document or an empty rule list, a wrong verdict
/// rather than a skip. An unported handler or eval rule binds Unsupported instead.
fn check_with(id: &str, raw: &RawCheck) -> Result<(), String> {
    let text = |key: &str| {
        raw.with
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("<absent>")
    };
    if raw.eval.as_deref() == Some("retained-document") {
        known(
            id,
            "with.retained",
            text("retained"),
            RETAINED_DOCUMENT_KEYS,
        )?;
        known(
            id,
            "retained-document handler",
            &raw.handler,
            RETAINED_DOCUMENT_HANDLERS,
        )?;
    }
    match raw.handler.as_str() {
        "server-card" => {
            if raw.eval.as_deref() != Some("retained-document") || text("retained") != "server-card"
            {
                return Err(format!(
                    "registry.json: check \"{id}\": the server-card handler needs eval retained-document and with.retained \"server-card\""
                ));
            }
            for param in CARD_FIELD_PARAMS {
                let rules = raw.with.get(*param).and_then(Value::as_array);
                if !rules.is_some_and(|r| !r.is_empty() && r.iter().all(is_field_rule)) {
                    return Err(format!(
                        "registry.json: check \"{id}\": with.{param} must list field rules"
                    ));
                }
            }
        }
        "protected-resource" => known(id, "with.op", text("op"), PROTECTED_RESOURCE_OPS)?,
        "cors-preflight" => known(id, "with.surface", text("surface"), CORS_SURFACES)?,
        _ => {}
    }
    Ok(())
}

fn read_check(value: Value, lanes: &[McpLane]) -> Result<NormalizedCheck, String> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("<no id>")
        .to_string();
    let raw: RawCheck = entry(&format!("check \"{id}\""), value)?;
    let tiers: Vec<&str> = TIERS.iter().map(|(tier, _)| *tier).collect();
    let keywords: Vec<&str> = TIERS.iter().map(|(_, keyword)| *keyword).collect();
    known(&id, "tier", &raw.tier, &tiers)?;
    known(&id, "keyword", &raw.keyword, &keywords)?;
    known(&id, "antecedent", &raw.antecedent, ANTECEDENTS)?;
    for site_type in &raw.site_types {
        known(&id, "site type", site_type, SITE_TYPES)?;
    }
    if let Some(lane) = &raw.lane {
        let ids: Vec<&str> = lanes.iter().map(|l| l.id.as_str()).collect();
        known(&id, "lane", lane, &ids)?;
    }
    check_with(&id, &raw)?;
    let patterns = collect_patterns(&id, &raw.with)?;
    let with_json = serde_json::to_string(&Value::Object(raw.with))
        .map_err(|e| format!("registry.json: check \"{id}\": with: {e}"))?;
    Ok(NormalizedCheck {
        eval_supported: raw
            .eval
            .as_deref()
            .is_none_or(|rule| EVAL_RULES.contains(&rule)),
        handler_supported: HANDLER_KINDS.contains(&raw.handler.as_str()),
        id: raw.id,
        category: raw.category,
        tier: raw.tier,
        keyword: raw.keyword,
        principle: raw.principle,
        site_types: raw.site_types,
        antecedent: raw.antecedent,
        eval: raw.eval,
        weight: raw.weight,
        title: raw.title,
        breadcrumb: raw.breadcrumb,
        hint: raw.hint,
        lane: raw.lane,
        handler: raw.handler,
        with_json,
        patterns,
    })
}

/// Read `registry.json`, which the site has already normalized.
pub fn read_registry(json: &str) -> Result<NormalizedRegistry, String> {
    let raw: RawRegistry = serde_json::from_str(json).map_err(|e| format!("registry.json: {e}"))?;
    let mcp_lanes = raw
        .mcp_lanes
        .into_iter()
        .map(|(id, value)| {
            let lane: McpLane = entry(&format!("mcp_lanes.{id}"), value)?;
            Ok(McpLane { id, ..lane })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let retired = raw
        .retired
        .into_iter()
        .map(|(id, value)| {
            let retired: RetiredCheck = entry(&format!("retired.{id}"), value)?;
            Ok(RetiredCheck { id, ..retired })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let alternatives = raw
        .alternatives
        .into_iter()
        .map(|group| {
            let variants = group
                .variants
                .into_iter()
                .map(|(name, value)| {
                    let what = format!("alternatives.{}.{name}", group.group);
                    let variant: AlternativeVariant = entry(&what, value)?;
                    for token in variant.antecedents.iter().chain(&variant.presented_by) {
                        if !ANTECEDENTS.contains(&token.as_str()) {
                            return Err(format!(
                                "registry.json: {what}: \"{token}\" is not an antecedent this build knows"
                            ));
                        }
                    }
                    Ok(AlternativeVariant { name, ..variant })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(AlternativeGroup {
                group: group.group,
                variants,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut categories = Vec::new();
    for slug in &raw.category_order {
        let name = raw
            .categories
            .get(slug)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!("registry.json: category_order names \"{slug}\", which categories does not")
            })?;
        categories.push((slug.clone(), name.to_string()));
    }
    let checks = raw
        .checks
        .into_iter()
        .map(|value| read_check(value, &mcp_lanes))
        .collect::<Result<Vec<_>, String>>()?;
    Ok(NormalizedRegistry {
        version: raw.version,
        mcp_discovery: raw.mcp_discovery,
        category_order: raw.category_order,
        categories,
        checks,
        mcp_lanes,
        retired,
        alternatives,
    })
}

/// Read `remediation.json`, sorted by id for emission.
pub fn read_remediation(json: &str) -> Result<Vec<RemediationEntry>, String> {
    let raw: Map<String, Value> =
        serde_json::from_str(json).map_err(|e| format!("remediation.json: {e}"))?;
    let mut entries = raw
        .into_iter()
        .map(|(id, value)| {
            let raw: RemediationEntry = serde_json::from_value(value)
                .map_err(|e| format!("remediation.json: \"{id}\": {e}"))?;
            Ok(RemediationEntry { id, ..raw })
        })
        .collect::<Result<Vec<_>, String>>()?;
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(entries)
}
