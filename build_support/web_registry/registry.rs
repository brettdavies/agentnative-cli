//! The registry document: top-level keys, `mcp_discovery`, categories, `mcp_lanes`, the checks.

use serde_json::Value;

use super::alternatives::normalize_alternatives;
use super::card_schema::card_requirements;
use super::check::normalize_check;
use super::common::{Mapping, unknown_key, yaml_display};
use super::retired::normalize_retired;
use super::vocab::{DISCOVERY_KEYS, LANE_FIELDS, TOP_LEVEL_KEYS};
use super::{McpDiscovery, McpLane, NormalizedRegistry};
use crate::yaml;

/// Validate and normalize the registry YAML text. `ua_tokens` is the vendored
/// `{ua:...}` map from `ts_consts`; `card_schema` is the vendored server-card
/// JSON Schema text, parsed only when a `server-card` check needs it.
pub fn normalize_registry(
    yaml: &str,
    ua_tokens: &[(String, String)],
    card_schema: &str,
) -> Result<NormalizedRegistry, String> {
    let doc = yaml::from_str::<yaml::StringKeyed>(yaml)
        .map_err(|e| format!("web-audit registry: YAML parse error: {e}"))?
        .0;
    let doc = doc.as_object().ok_or_else(|| {
        "web-audit registry: expected a YAML mapping at the top level".to_string()
    })?;
    if let Some(key) = unknown_key(doc, TOP_LEVEL_KEYS) {
        return Err(format!(
            "web-audit registry: unknown top-level key \"{key}\""
        ));
    }

    let mcp_discovery = normalize_discovery(doc)?;
    let categories_map = doc
        .get("categories")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            "web-audit registry: expected a top-level \"categories\" mapping".to_string()
        })?;
    let category_order: Vec<String> = doc
        .get("category_order")
        .and_then(Value::as_array)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "web-audit registry: expected a top-level \"category_order\" array".to_string()
        })?
        .iter()
        .map(|v| {
            v.as_str().map(str::to_string).ok_or_else(|| {
                "web-audit registry: category_order entries must be strings".to_string()
            })
        })
        .collect::<Result<_, _>>()?;
    let category_keys: Vec<String> = categories_map.keys().cloned().collect();
    if category_order.len() != category_keys.len()
        || !category_order
            .iter()
            .all(|slug| category_keys.contains(slug))
    {
        return Err(
            "web-audit registry: category_order must list every categories key exactly once"
                .to_string(),
        );
    }
    let categories: Vec<(String, String)> = category_order
        .iter()
        .map(|slug| {
            let name = categories_map
                .get(slug)
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    format!("web-audit registry: category {slug:?} needs a string display name")
                })?;
            Ok((slug.clone(), name.to_string()))
        })
        .collect::<Result<_, String>>()?;

    let checks = doc
        .get("checks")
        .and_then(Value::as_array)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "web-audit registry: expected a non-empty top-level \"checks\" array".to_string()
        })?;
    let mcp_lanes = normalize_lanes(doc.get("mcp_lanes"))?;
    let lane_ids: Vec<&str> = mcp_lanes.iter().map(|l| l.id.as_str()).collect();
    let mut card_fields: Option<Mapping> = None;
    let mut seen: Vec<String> = Vec::new();
    let mut normalized = Vec::with_capacity(checks.len());
    for check in checks {
        let mut card = || -> Result<Mapping, String> {
            if card_fields.is_none() {
                card_fields = Some(card_requirements(card_schema)?);
            }
            Ok(card_fields.clone().unwrap_or_default())
        };
        let check = normalize_check(check, &category_keys, &lane_ids, ua_tokens, &mut card)?;
        if seen.contains(&check.id) {
            return Err(format!(
                "web-audit registry: duplicate check id \"{}\"",
                check.id
            ));
        }
        seen.push(check.id.clone());
        normalized.push(check);
    }

    let version = match doc.get("version") {
        None => 1,
        Some(v) => v.as_u64().ok_or_else(|| {
            "web-audit registry: version must be a non-negative integer".to_string()
        })?,
    };
    let retired = normalize_retired(doc.get("retired"), &normalized)?;
    let alternatives = normalize_alternatives(doc.get("alternatives"), &normalized)?;
    Ok(NormalizedRegistry {
        version,
        mcp_discovery,
        category_order,
        categories,
        checks: normalized,
        mcp_lanes,
        retired,
        alternatives,
    })
}

fn normalize_lanes(raw: Option<&Value>) -> Result<Vec<McpLane>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let lanes = raw.as_object().ok_or_else(|| {
        "web-audit registry: mcp_lanes must be a mapping of lane to { label, note }".to_string()
    })?;
    lanes
        .iter()
        .map(|(id, spec)| {
            let err = || {
                format!(
                    "web-audit registry: mcp_lanes entry \"{id}\" needs a string label and note"
                )
            };
            let spec = spec.as_object().ok_or_else(err)?;
            if let Some(field) = unknown_key(spec, LANE_FIELDS) {
                return Err(format!(
                    "web-audit registry: mcp_lanes entry \"{id}\" carries unknown field \"{field}\""
                ));
            }
            let label = spec
                .get("label")
                .and_then(Value::as_str)
                .filter(|l| !l.is_empty())
                .ok_or_else(err)?;
            let note = spec.get("note").and_then(Value::as_str).ok_or_else(err)?;
            Ok(McpLane {
                id: id.clone(),
                label: label.to_string(),
                note: note.to_string(),
            })
        })
        .collect()
}

fn normalize_discovery(doc: &Mapping) -> Result<McpDiscovery, String> {
    let err = || {
        "web-audit registry: mcp_discovery must carry well_known[], common_paths[], protocol_version".to_string()
    };
    let discovery = doc
        .get("mcp_discovery")
        .and_then(Value::as_object)
        .ok_or_else(err)?;
    if let Some(key) = unknown_key(discovery, DISCOVERY_KEYS) {
        return Err(format!(
            "web-audit registry: mcp_discovery carries unknown key \"{key}\""
        ));
    }
    let path = |key: &str| -> Result<String, String> {
        match discovery.get(key) {
            Some(Value::String(p)) if p.starts_with('/') => Ok(p.clone()),
            other => Err(format!(
                "web-audit registry: mcp_discovery.{key} must be a path starting with \"/\" (got {})",
                yaml_display(other)
            )),
        }
    };
    let strings = |key: &str| -> Result<Vec<String>, String> {
        discovery
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(err)?
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(err))
            .collect()
    };
    Ok(McpDiscovery {
        ai_catalog: path("ai_catalog")?,
        card_suffix: path("card_suffix")?,
        well_known: strings("well_known")?,
        common_paths: strings("common_paths")?,
        protocol_version: discovery
            .get("protocol_version")
            .and_then(Value::as_str)
            .ok_or_else(err)?
            .to_string(),
    })
}
