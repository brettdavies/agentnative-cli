//! The `alternatives` block: site designs the global score counts one variant of.

use serde_json::Value;

use super::common::{is_check_id, unknown_key, yaml_display};
use super::vocab::{ANTECEDENTS, GROUP_FIELDS, VARIANT_FIELDS};
use super::{AlternativeGroup, AlternativeVariant, NormalizedCheck};

fn variant_tokens(value: Option<&Value>, name: &str, field: &str) -> Result<Vec<String>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
        format!(
            "web-audit registry: alternatives variant \"{name}\" {field} needs a non-empty array of tokens"
        )
    })?;
    items
        .iter()
        .map(|token| match token.as_str() {
            Some(t) if ANTECEDENTS.contains(&t) => Ok(t.to_string()),
            _ => Err(format!(
                "web-audit registry: alternatives variant \"{name}\" names unknown antecedent {}",
                yaml_display(Some(token))
            )),
        })
        .collect()
}

/// Mirror of the site's `normalizeAlternatives`: a variant's checks are those
/// gated on its `antecedents`, it is presented when a check gated on one of
/// its `presented_by` tokens applied, and no token belongs to two variants.
pub(super) fn normalize_alternatives(
    raw: Option<&Value>,
    checks: &[NormalizedCheck],
) -> Result<Vec<AlternativeGroup>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let groups = raw
        .as_array()
        .ok_or_else(|| "web-audit registry: alternatives must be an array of groups".to_string())?;
    let gates = |tokens: &[String]| checks.iter().any(|c| tokens.contains(&c.antecedent));
    let mut group_ids: Vec<String> = Vec::new();
    let mut claimed: Vec<(String, String)> = Vec::new();
    let mut out = Vec::with_capacity(groups.len());
    for entry in groups {
        let group = entry
            .get("group")
            .and_then(Value::as_str)
            .filter(|g| is_check_id(g))
            .ok_or_else(|| {
                format!(
                    "web-audit registry: alternatives group {} must match /^[a-z0-9][a-z0-9-]*$/",
                    yaml_display(entry.get("group"))
                )
            })?
            .to_string();
        if let Some(field) = entry.as_object().and_then(|m| unknown_key(m, GROUP_FIELDS)) {
            return Err(format!(
                "web-audit registry: alternatives group \"{group}\" carries unknown field \"{field}\""
            ));
        }
        if group_ids.contains(&group) {
            return Err(format!(
                "web-audit registry: duplicate alternatives group \"{group}\""
            ));
        }
        group_ids.push(group.clone());
        let variants = entry
            .get("variants")
            .and_then(Value::as_object)
            .filter(|v| v.len() >= 2)
            .ok_or_else(|| {
                format!(
                    "web-audit registry: alternatives group \"{group}\" needs at least two variants"
                )
            })?;
        let mut normalized = Vec::with_capacity(variants.len());
        for (variant, spec) in variants {
            let name = format!("{group}.{variant}");
            if !is_check_id(variant) {
                return Err(format!(
                    "web-audit registry: alternatives variant \"{name}\" must match /^[a-z0-9][a-z0-9-]*$/"
                ));
            }
            let spec = spec.as_object().ok_or_else(|| {
                format!("web-audit registry: alternatives variant \"{name}\" must be a mapping")
            })?;
            if let Some(field) = unknown_key(spec, VARIANT_FIELDS) {
                return Err(format!(
                    "web-audit registry: alternatives variant \"{name}\" carries unknown field \"{field}\""
                ));
            }
            let antecedents = variant_tokens(spec.get("antecedents"), &name, "antecedents")?;
            let presented_by = match spec.get("presented_by") {
                None => antecedents.clone(),
                some => variant_tokens(some, &name, "presented_by")?,
            };
            if !antecedents.is_empty() && !gates(&antecedents) {
                return Err(format!(
                    "web-audit registry: alternatives variant \"{name}\" gates no check"
                ));
            }
            if !gates(&presented_by) {
                return Err(format!(
                    "web-audit registry: alternatives variant \"{name}\" is presented by no check"
                ));
            }
            let mut tokens: Vec<&String> = antecedents.iter().chain(&presented_by).collect();
            tokens.sort();
            tokens.dedup();
            for token in tokens {
                if let Some((_, prior)) = claimed.iter().find(|(t, _)| t == token) {
                    return Err(format!(
                        "web-audit registry: antecedent \"{token}\" belongs to both \"{prior}\" and \"{name}\""
                    ));
                }
                claimed.push((token.clone(), name.clone()));
            }
            normalized.push(AlternativeVariant {
                name: variant.clone(),
                antecedents,
                presented_by,
            });
        }
        if normalized.iter().all(|v| v.antecedents.is_empty()) {
            return Err(format!(
                "web-audit registry: alternatives group \"{group}\" has no variant with checks of its own"
            ));
        }
        out.push(AlternativeGroup {
            group,
            variants: normalized,
        });
    }
    Ok(out)
}
