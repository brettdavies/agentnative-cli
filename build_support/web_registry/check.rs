//! One `checks` entry: its fields, handler parameters, and the `keyword` derived from `tier`.

use serde_json::Value;

use super::common::{Mapping, check_err, is_check_id, unknown_key, yaml_display};
use super::patterns::collect_patterns;
use super::user_agent::expand_probe_user_agent;
use super::vocab::{
    ANTECEDENTS, BREADCRUMB_MAX, CARD_FIELD_PARAMS, CHECK_KEYS, CORS_SURFACES, LANE_CATEGORY,
    PROTECTED_RESOURCE_OPS, RETAINED_DOCUMENT_HANDLERS, RETAINED_DOCUMENT_KEYS, SITE_TYPES, TIERS,
};
use super::{EVAL_RULES, HANDLER_KINDS, NormalizedCheck};

fn required_str(check: &Mapping, id: &str, key: &str) -> Result<String, String> {
    match check.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(check_err(id, format!("missing {key}"))),
    }
}

pub(super) fn normalize_check(
    raw: &Value,
    category_keys: &[String],
    lane_ids: &[&str],
    ua_tokens: &[(String, String)],
    card_fields: &mut dyn FnMut() -> Result<Mapping, String>,
) -> Result<NormalizedCheck, String> {
    let check = raw
        .as_object()
        .ok_or_else(|| "web-audit registry: every checks entry must be a mapping".to_string())?;
    let id = match check.get("id").and_then(Value::as_str) {
        Some(id) if is_check_id(id) => id.to_string(),
        other => {
            return Err(format!(
                "web-audit registry: check id {} must match /^[a-z0-9][a-z0-9-]*$/",
                other
                    .map(|s| format!("\"{s}\""))
                    .unwrap_or_else(|| "null".to_string())
            ));
        }
    };

    let category = check
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !category_keys.contains(&category) {
        return Err(check_err(
            &id,
            format!("names unknown category \"{category}\""),
        ));
    }
    let tier = check
        .get("tier")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let keyword = TIERS
        .iter()
        .find(|(t, _)| *t == tier)
        .map(|(_, k)| k.to_string())
        .ok_or_else(|| {
            check_err(
                &id,
                format!("has invalid tier \"{tier}\" (want required|recommended|optional)"),
            )
        })?;
    if check.contains_key("keyword") {
        return Err(check_err(
            &id,
            "hand-authors a keyword field: keyword is derived from tier at build time",
        ));
    }
    if check.contains_key("applies_to") {
        return Err(check_err(
            &id,
            "carries the retired applies_to field: use site_types + antecedent",
        ));
    }
    if let Some(key) = unknown_key(check, CHECK_KEYS) {
        return Err(check_err(&id, format!("carries unknown field \"{key}\"")));
    }
    let principle = check
        .get("principle")
        .and_then(Value::as_str)
        .filter(|p| is_principle(p))
        .ok_or_else(|| {
            check_err(
                &id,
                format!(
                    "needs a principle in P1..P8 (got {})",
                    yaml_display(check.get("principle"))
                ),
            )
        })?
        .to_string();
    let site_types: Vec<String> = check
        .get("site_types")
        .and_then(Value::as_array)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| check_err(&id, "needs a non-empty site_types array"))?
        .iter()
        .map(|v| {
            let st = v.as_str().unwrap_or_default();
            if SITE_TYPES.contains(&st) {
                Ok(st.to_string())
            } else {
                Err(check_err(
                    &id,
                    format!("has invalid site_types entry \"{st}\""),
                ))
            }
        })
        .collect::<Result<_, _>>()?;
    let antecedent = check
        .get("antecedent")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if !ANTECEDENTS.contains(&antecedent.as_str()) {
        return Err(check_err(
            &id,
            format!("has unknown antecedent \"{antecedent}\""),
        ));
    }
    let eval = match check.get("eval") {
        None => None,
        Some(Value::String(e)) => Some(e.clone()),
        Some(other) => {
            return Err(check_err(
                &id,
                format!("eval must be a string (got {})", yaml_display(Some(other))),
            ));
        }
    };
    let eval_supported = eval.as_deref().is_none_or(|e| EVAL_RULES.contains(&e));
    let weight = check
        .get("weight")
        .and_then(Value::as_u64)
        .filter(|w| *w > 0)
        .ok_or_else(|| {
            check_err(
                &id,
                format!(
                    "needs a positive integer weight (got {})",
                    yaml_display(check.get("weight"))
                ),
            )
        })?;
    let title = required_str(check, &id, "title")?;
    let hint = required_str(check, &id, "hint")?;
    let breadcrumb = required_str(check, &id, "breadcrumb")?;
    if breadcrumb.chars().count() > BREADCRUMB_MAX {
        return Err(check_err(
            &id,
            format!(
                "breadcrumb is {} chars, over the {BREADCRUMB_MAX} a trail fits",
                breadcrumb.chars().count()
            ),
        ));
    }
    let handler = check
        .get("handler")
        .and_then(Value::as_str)
        .ok_or_else(|| check_err(&id, "missing handler"))?
        .to_string();
    let handler_supported = HANDLER_KINDS.contains(&handler.as_str());
    let lane = match (category == LANE_CATEGORY, check.get("lane")) {
        (true, Some(Value::String(lane))) if lane_ids.contains(&lane.as_str()) => {
            Some(lane.clone())
        }
        (true, other) => {
            return Err(check_err(
                &id,
                format!(
                    "needs a lane from mcp_lanes ({}), got {}",
                    lane_ids.join(", "),
                    yaml_display(other)
                ),
            ));
        }
        (false, None) => None,
        (false, Some(_)) => {
            return Err(check_err(
                &id,
                format!("carries a lane, which only {LANE_CATEGORY} checks take"),
            ));
        }
    };
    let with = check
        .get("with")
        .and_then(Value::as_object)
        .ok_or_else(|| check_err(&id, "missing \"with\" handler parameters"))?;
    let retained = with.get("retained").and_then(Value::as_str);
    if eval.as_deref() == Some("retained-document") {
        if !retained.is_some_and(|r| RETAINED_DOCUMENT_KEYS.contains(&r)) {
            return Err(check_err(
                &id,
                format!(
                    "declares retained-document, so with.retained must name a document discovery keeps ({}), got {}",
                    RETAINED_DOCUMENT_KEYS.join(", "),
                    yaml_display(with.get("retained"))
                ),
            ));
        }
        if !RETAINED_DOCUMENT_HANDLERS.contains(&handler.as_str()) {
            return Err(check_err(
                &id,
                format!(
                    "declares retained-document, so its handler must score a retained document ({}), got \"{handler}\"",
                    RETAINED_DOCUMENT_HANDLERS.join(", ")
                ),
            ));
        }
    }
    if handler == "server-card" {
        if eval.as_deref() != Some("retained-document") || retained != Some("server-card") {
            return Err(check_err(
                &id,
                "uses the server-card handler, so it needs eval retained-document and with.retained \"server-card\"",
            ));
        }
        if let Some(param) = CARD_FIELD_PARAMS.iter().find(|p| with.contains_key(**p)) {
            return Err(check_err(
                &id,
                format!(
                    "hand-authors with.{param}, which the build reads from the vendored server card schema; remove it"
                ),
            ));
        }
    }
    if handler == "protected-resource" {
        let op = with.get("op").and_then(Value::as_str);
        if !op.is_some_and(|o| PROTECTED_RESOURCE_OPS.contains(&o)) {
            return Err(check_err(
                &id,
                format!(
                    "needs with.op \"challenge\" or \"metadata\" (got {})",
                    yaml_display(with.get("op"))
                ),
            ));
        }
    }
    if handler == "cors-preflight" {
        let surface = with.get("surface").and_then(Value::as_str);
        if !surface.is_some_and(|s| CORS_SURFACES.contains(&s)) {
            return Err(check_err(
                &id,
                format!(
                    "needs with.surface \"preflight\" or \"actual\" (got {})",
                    yaml_display(with.get("surface"))
                ),
            ));
        }
    }
    let with = if handler == "server-card" {
        let mut spliced = with.clone();
        spliced.extend(card_fields()?);
        spliced
    } else {
        expand_probe_user_agent(&id, with, ua_tokens)?
    };
    let patterns = collect_patterns(&id, &with)?;
    let with_json = serde_json::to_string(&Value::Object(with))
        .map_err(|e| check_err(&id, format!("with cannot serialize as JSON: {e}")))?;

    Ok(NormalizedCheck {
        id,
        category,
        tier,
        keyword,
        principle,
        site_types,
        antecedent,
        eval,
        eval_supported,
        weight,
        title,
        breadcrumb,
        hint,
        lane,
        handler,
        handler_supported,
        with_json,
        patterns,
    })
}

fn is_principle(p: &str) -> bool {
    let mut chars = p.chars();
    chars.next() == Some('P')
        && chars.next().is_some_and(|d| ('1'..='8').contains(&d))
        && chars.next().is_none()
}
