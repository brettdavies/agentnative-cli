//! What a ported handler needs from a check's `with`, read at build time. The site
//! validates `with`, but a handler reads it at run time with no way to report a value
//! the site added that this build does not know: it would read as a missing document, an
//! empty rule list, or another probe under the check's own id, a wrong verdict rather
//! than a skip.

use serde_json::{Map, Value};

use super::patterns::check_err;
use super::vocab::{
    API_HYGIENE_OPS, CARD_FIELD_PARAMS, CORS_SURFACES, EVAL_RULES, HANDLER_KINDS, JSON_TYPES,
    LLMS_TXT_OPS, PROTECTED_RESOURCE_OPS, RETAINED_DOCUMENT_HANDLERS, RETAINED_DOCUMENT_KEYS,
};

/// `value` must be one of `allowed`, or the build stops naming the check.
pub(super) fn known(id: &str, field: &str, value: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(check_err(
            id,
            format!(
                "{field} \"{value}\" is not one this build knows ({})",
                allowed.join(", ")
            ),
        ))
    }
}

/// A field name and the JSON Schema types that field may hold.
fn is_field_rule(rule: &Value) -> bool {
    rule.get("field").is_some_and(Value::is_string)
        && rule
            .get("type")
            .and_then(Value::as_array)
            .is_some_and(|types| {
                !types.is_empty()
                    && types
                        .iter()
                        .all(|t| t.as_str().is_some_and(|t| JSON_TYPES.contains(&t)))
            })
}

/// An unported handler kind or eval rule binds Unsupported and never reads `with`, so
/// nothing here applies to it and a new kind cannot stop the build.
pub(super) fn check_with(
    id: &str,
    handler: &str,
    eval: Option<&str>,
    with: &Map<String, Value>,
) -> Result<(), String> {
    if !HANDLER_KINDS.contains(&handler) || eval.is_some_and(|rule| !EVAL_RULES.contains(&rule)) {
        return Ok(());
    }
    let text = |key: &str| with.get(key).and_then(Value::as_str).unwrap_or("<absent>");
    if eval == Some("retained-document") {
        known(
            id,
            "with.retained",
            text("retained"),
            RETAINED_DOCUMENT_KEYS,
        )?;
        known(
            id,
            "retained-document handler",
            handler,
            RETAINED_DOCUMENT_HANDLERS,
        )?;
    }
    match handler {
        "server-card" => {
            if eval != Some("retained-document") || text("retained") != "server-card" {
                return Err(check_err(
                    id,
                    "the server-card handler needs eval retained-document and with.retained \"server-card\"".to_string(),
                ));
            }
            for param in CARD_FIELD_PARAMS {
                let rules = with.get(*param).and_then(Value::as_array);
                if !rules.is_some_and(|r| !r.is_empty() && r.iter().all(is_field_rule)) {
                    return Err(check_err(
                        id,
                        format!(
                            "with.{param} must list field rules, each with a field and known types"
                        ),
                    ));
                }
            }
        }
        "protected-resource" => known(id, "with.op", text("op"), PROTECTED_RESOURCE_OPS)?,
        "api-hygiene" => known(id, "with.op", text("op"), API_HYGIENE_OPS)?,
        "llms-txt-quality" => known(id, "with.op", text("op"), LLMS_TXT_OPS)?,
        "cors-preflight" => known(id, "with.surface", text("surface"), CORS_SURFACES)?,
        _ => {}
    }
    Ok(())
}
