//! The `retired` block: ids outside `checks`, each naming a live successor check.

use serde_json::Value;

use super::common::{is_check_id, unknown_key, yaml_display};
use super::vocab::RETIRED_FIELDS;
use super::{NormalizedCheck, RetiredCheck};

pub(super) fn normalize_retired(
    raw: Option<&Value>,
    checks: &[NormalizedCheck],
) -> Result<Vec<RetiredCheck>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let retired = raw.as_object().ok_or_else(|| {
        "web-audit registry: retired must be a mapping of check id to { successor, reason }"
            .to_string()
    })?;
    let live = |id: &str| checks.iter().any(|c| c.id == id);
    retired
        .iter()
        .map(|(id, entry)| {
            if !is_check_id(id) {
                return Err(format!(
                    "web-audit registry: retired id \"{id}\" must match /^[a-z0-9][a-z0-9-]*$/"
                ));
            }
            if live(id) {
                return Err(format!(
                    "web-audit registry: retired id \"{id}\" is still a registry check"
                ));
            }
            let entry = entry.as_object().ok_or_else(|| {
                format!(
                    "web-audit registry: retired id \"{id}\" must map to {{ successor, reason }}"
                )
            })?;
            if let Some(field) = unknown_key(entry, RETIRED_FIELDS) {
                return Err(format!(
                    "web-audit registry: retired id \"{id}\" carries unknown field \"{field}\""
                ));
            }
            let successor = entry
                .get("successor")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !live(successor) {
                return Err(format!(
                    "web-audit registry: retired id \"{id}\" names successor {}, which is not a registry check",
                    yaml_display(entry.get("successor"))
                ));
            }
            let reason = entry
                .get("reason")
                .and_then(Value::as_str)
                .filter(|r| !r.is_empty())
                .ok_or_else(|| {
                    format!("web-audit registry: retired id \"{id}\" needs a reason")
                })?;
            Ok(RetiredCheck {
                id: id.clone(),
                successor: successor.to_string(),
                reason: reason.to_string(),
            })
        })
        .collect()
}
