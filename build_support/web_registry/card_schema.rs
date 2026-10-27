//! The server-card required fields, read from the vendored server-card JSON Schema.

use serde_json::Value;

use super::common::{Mapping, yaml_display};

/// The fields a SEP-2127 server card must carry at its top level and in each
/// `remotes[]` item, with their JSON types, read from the vendored schema the
/// way the site's `web-audit-card-schema.mjs` reads them.
pub(super) fn card_requirements(schema_text: &str) -> Result<Mapping, String> {
    let schema: Value = serde_json::from_str(schema_text).map_err(|e| {
        format!(
            "web-audit registry: server card schema is not JSON ({e}); run scripts/sync-web-audit.sh"
        )
    })?;
    let defs = schema.get("$defs").and_then(Value::as_object);
    let card = defs
        .and_then(|d| d.get("ServerCard"))
        .filter(|c| c.is_object())
        .ok_or_else(|| {
            "web-audit registry: server card schema has no $defs.ServerCard".to_string()
        })?;
    let remote = card
        .pointer("/properties/remotes/items/$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix("#/$defs/"))
        .filter(|name| !name.contains('/'))
        .and_then(|name| defs.and_then(|d| d.get(name)))
        .ok_or_else(|| {
            "web-audit registry: server card schema's ServerCard.remotes items name no local $defs entry"
                .to_string()
        })?;
    let mut out = Mapping::new();
    out.insert(
        "required".to_string(),
        required_fields(card, "$defs.ServerCard")?,
    );
    out.insert(
        "remote_required".to_string(),
        required_fields(remote, "ServerCard.remotes[]")?,
    );
    Ok(out)
}

fn required_fields(def: &Value, label: &str) -> Result<Value, String> {
    const JSON_TYPES: &[&str] = &[
        "string", "number", "integer", "boolean", "object", "array", "null",
    ];
    let required = def
        .get("required")
        .and_then(Value::as_array)
        .filter(|r| !r.is_empty())
        .ok_or_else(|| {
            format!("web-audit registry: server card schema carries no required array at {label}")
        })?;
    required
        .iter()
        .map(|field| {
            let name = field.as_str();
            let declared = name.and_then(|n| def.get("properties")?.get(n)?.get("type"));
            let types: Vec<Value> = match declared {
                Some(Value::Array(a)) => a.clone(),
                Some(v) => vec![v.clone()],
                None => Vec::new(),
            };
            let typed = !types.is_empty()
                && types
                    .iter()
                    .all(|t| t.as_str().is_some_and(|t| JSON_TYPES.contains(&t)));
            match name {
                Some(name) if typed => {
                    let mut entry = Mapping::new();
                    entry.insert("field".to_string(), Value::String(name.to_string()));
                    entry.insert("type".to_string(), Value::Array(types));
                    Ok(Value::Object(entry))
                }
                _ => Err(format!(
                    "web-audit registry: server card schema field {label}.{} declares no JSON type (got {})",
                    name.unwrap_or("?"),
                    yaml_display(declared)
                )),
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}
