//! The remediation catalog, held 1:1 against the live and retired check ids.

use serde_json::Value;

use super::{RemediationEntry, RemediationResource};
use crate::yaml;

/// Validate the remediation catalog against the check ids and return the
/// entries sorted by id. Mirrors `normalizeWebRemediation`.
pub fn normalize_remediation(
    yaml: &str,
    check_ids: &[&str],
    retired_ids: &[&str],
) -> Result<Vec<RemediationEntry>, String> {
    let doc = yaml::from_str::<yaml::StringKeyed>(yaml)
        .map_err(|e| format!("web-audit remediation.yaml: YAML parse error: {e}"))?
        .0;
    let remediation = doc
        .as_object()
        .and_then(|d| d.get("remediation"))
        .and_then(Value::as_object)
        .ok_or_else(|| {
            "web-audit remediation.yaml: expected a top-level \"remediation\" mapping".to_string()
        })?;
    let mut out = Vec::with_capacity(remediation.len());
    for (id, entry) in remediation {
        let entry = entry.as_object().ok_or_else(|| {
            format!(
                "web-audit remediation: entry \"{id}\" needs string title, goal, and fix fields"
            )
        })?;
        let field = |name: &str| -> Result<String, String> {
            entry
                .get(name)
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    format!("web-audit remediation: entry \"{id}\" needs string title, goal, and fix fields")
                })
        };
        let title = field("title")?;
        let goal = field("goal")?;
        let fix = field("fix")?;
        if entry.contains_key("body") || entry.contains_key("evidence_template") {
            return Err(format!(
                "web-audit remediation: entry \"{id}\" carries a retired body/evidence_template field: use goal/fix/resources"
            ));
        }
        if fix.contains("{{evidence}}") {
            return Err(format!(
                "web-audit remediation: entry \"{id}\" carries an evidence slot: evidence is assembled at audit time"
            ));
        }
        let resources = match entry.get("resources") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(seq)) => seq
                .iter()
                .map(|r| {
                    let r = r.as_object();
                    let label = r.and_then(|r| r.get("label")).and_then(Value::as_str);
                    let url = r.and_then(|r| r.get("url")).and_then(Value::as_str);
                    match (label, url) {
                        (Some(label), Some(url)) if url.starts_with("http://") || url.starts_with("https://") => {
                            Ok(RemediationResource { label: label.to_string(), url: url.to_string() })
                        }
                        _ => Err(format!(
                            "web-audit remediation: entry \"{id}\" resource needs a label and an absolute url"
                        )),
                    }
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(format!("web-audit remediation: entry \"{id}\" resources must be an array")),
        };
        out.push(RemediationEntry {
            id: id.to_string(),
            title,
            goal,
            fix,
            resources,
        });
    }
    for id in check_ids {
        if !out.iter().any(|r| r.id == *id) {
            return Err(format!(
                "web-audit remediation: check \"{id}\" has no remediation entry"
            ));
        }
    }
    for entry in &out {
        if !check_ids.contains(&entry.id.as_str()) && !retired_ids.contains(&entry.id.as_str()) {
            return Err(format!(
                "web-audit remediation: orphan remediation \"{}\" matches no check",
                entry.id
            ));
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}
