//! The `server-card` handler, a mirror of the site's
//! `handlers/server-card.ts`: it scores the card of record discovery kept,
//! with no request of its own. A card that answered but does not parse
//! reads broken. A SEP-1649-shaped card passes with the superseded
//! advisory, and so does one that names its server in `serverInfo`, a
//! SEP-1649 field SEP-2127 does not have, with no endpoint field for
//! discovery to read; any other card is held to SEP-2127 and passes when it
//! carries every field the extension schema requires, at its top level and
//! in each `remotes[]` item, with the JSON type the schema gives. The field
//! lists arrive in `with`, spliced from the vendored schema at build.

use serde_json::{Value, json};

use super::shared::item;
use crate::web_audit::engine::discovery_documents::{
    CardShape, JsonObject, card_shape, parse_json_object,
};
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::fetch::body::DOCUMENT_MAX_BODY_BYTES;
use crate::web_audit::registry::WebCheck;
use crate::web_audit::scorecard::RowAdvisory;

const REMOTES: &str = "remotes";

/// Whether `value` holds the JSON type the schema names. The build admits
/// only the seven JSON Schema type names.
fn has_type(value: &Value, json_type: &str) -> bool {
    match json_type {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_number().is_some_and(|n| {
            n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.fract() == 0.0)
        }),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => false,
    }
}

/// One required field and the JSON types the schema allows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardFieldRule {
    /// The field name.
    pub field: String,
    /// The JSON Schema type names it may hold.
    pub types: Vec<String>,
}

/// The `with` block of the `server-card` check.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServerCardWith {
    /// The retained-document key the card was kept under.
    pub retained: String,
    /// The fields a card's top level must carry.
    pub required: Vec<CardFieldRule>,
    /// The fields each `remotes[]` item must carry.
    pub remote_required: Vec<CardFieldRule>,
}

fn field_rules(with: &Value, key: &str) -> Vec<CardFieldRule> {
    let rule = |rule: &Value| -> Option<CardFieldRule> {
        let types = rule.get("type")?.as_array()?;
        Some(CardFieldRule {
            field: rule.get("field")?.as_str()?.to_string(),
            types: types
                .iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect(),
        })
    };
    let rules = with.get(key).and_then(Value::as_array);
    rules.into_iter().flatten().filter_map(rule).collect()
}

impl ServerCardWith {
    /// Read a check's `with` JSON, whose shape the build validated.
    pub fn parse(with_json: &str) -> ServerCardWith {
        let with: Value = serde_json::from_str(with_json).unwrap_or_default();
        ServerCardWith {
            retained: with
                .get("retained")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            required: field_rules(&with, "required"),
            remote_required: field_rules(&with, "remote_required"),
        }
    }
}

/// What `value` lacks against `rules`, each problem prefixed with where it sits.
fn field_problems(value: &JsonObject, rules: &[CardFieldRule], at: &str) -> Vec<String> {
    let missing: Vec<&str> = rules
        .iter()
        .filter(|rule| !value.contains_key(&rule.field))
        .map(|rule| rule.field.as_str())
        .collect();
    let mut problems = Vec::new();
    if !missing.is_empty() {
        let plural = if missing.len() == 1 { "" } else { "s" };
        problems.push(format!(
            "{at}missing required field{plural} {}",
            missing.join(", ")
        ));
    }
    for rule in rules {
        if let Some(field) = value.get(&rule.field)
            && !rule.types.iter().any(|t| has_type(field, t))
        {
            problems.push(format!(
                "{at}{} is not {}",
                rule.field,
                rule.types.join(" or ")
            ));
        }
    }
    problems
}

/// Every way `card` falls short of the SEP-2127 required fields.
pub fn sep2127_problems(card: &JsonObject, w: &ServerCardWith) -> Vec<String> {
    let mut problems = field_problems(card, &w.required, "");
    let Some(remotes) = card.get(REMOTES) else {
        return problems;
    };
    let Some(remotes) = remotes.as_array() else {
        problems.push(format!("{REMOTES} is not an array"));
        return problems;
    };
    for (i, remote) in remotes.iter().enumerate() {
        let at = format!("{REMOTES}[{i}] ");
        match remote.as_object() {
            Some(remote) => problems.extend(field_problems(remote, &w.remote_required, &at)),
            None => problems.push(format!("{at}is not an object")),
        }
    }
    problems
}

/// A card that names its server in `serverInfo` and carries no other
/// recognized shape is a SEP-1649 card.
fn scored_shape(card: Option<&JsonObject>, discovered: CardShape) -> CardShape {
    let names_server = card
        .and_then(|c| c.get("serverInfo"))
        .is_some_and(Value::is_object);
    if discovered == CardShape::Unrecognized && names_server {
        CardShape::Sep1649
    } else {
        discovered
    }
}

/// Score the card of record discovery kept.
pub fn run_server_card(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let w = ServerCardWith::parse(check.with_json);
    let Some(doc) = ctx.retained_documents.get(w.retained.as_str()) else {
        return ProbeOutcome::new(
            ProbeStatus::Absent,
            vec![item([
                ("retained", json!(w.retained)),
                ("why", json!(["discovery found no server card"])),
            ])],
        );
    };
    let card = parse_json_object(&doc.response);
    let discovered = doc.shape.unwrap_or_else(|| card_shape(card.as_ref()));
    let shape = scored_shape(card.as_ref(), discovered);
    let mut row = item([
        ("url", json!(doc.url)),
        ("status", json!(doc.response.status)),
        ("retained", json!(w.retained)),
        ("shape", json!(shape.as_str())),
    ]);
    let Some(card) = card else {
        let why = if doc.response.truncated {
            row.insert("truncated".to_string(), json!(true));
            format!(
                "the card was cut at the {} KiB read cap",
                DOCUMENT_MAX_BODY_BYTES / 1024
            )
        } else {
            "the card is not a JSON object".to_string()
        };
        row.insert("ok".to_string(), json!(false));
        row.insert("why".to_string(), json!([why]));
        return ProbeOutcome::new(ProbeStatus::Broken, vec![row]);
    };
    if shape == CardShape::Sep1649 {
        row.insert("ok".to_string(), json!(true));
        row.insert("why".to_string(), json!([]));
        return ProbeOutcome {
            advisory: Some(RowAdvisory::Superseded),
            ..ProbeOutcome::new(ProbeStatus::Pass, vec![row])
        };
    }
    let problems = sep2127_problems(&card, &w);
    let passed = problems.is_empty();
    row.insert("ok".to_string(), json!(passed));
    if passed {
        row.insert("why".to_string(), json!([]));
        return ProbeOutcome::new(ProbeStatus::Pass, vec![row]);
    }
    row.insert("why".to_string(), json!([problems.join("; ")]));
    ProbeOutcome::new(ProbeStatus::Broken, vec![row])
}

/// The row's evidence line: where the card was read and, when it falls
/// short, how.
pub fn server_card_evidence(outcome: &ProbeOutcome) -> String {
    let first = outcome.evidence.first();
    let why: Vec<&str> = first
        .and_then(|f| f.get("why"))
        .and_then(Value::as_array)
        .map(|lines| lines.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let Some(url) = first.and_then(|f| f.get("url")).and_then(Value::as_str) else {
        return why.join("; ");
    };
    let status = match first.and_then(|f| f.get("status")) {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => "error".to_string(),
    };
    let read = format!("{url} -> {status}");
    if why.is_empty() {
        read
    } else {
        format!("{read} ({})", why.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(pairs: &[(&str, &[&str])]) -> Vec<CardFieldRule> {
        pairs
            .iter()
            .map(|(field, types)| CardFieldRule {
                field: field.to_string(),
                types: types.iter().map(|t| t.to_string()).collect(),
            })
            .collect()
    }

    fn with() -> ServerCardWith {
        ServerCardWith {
            retained: "server-card".to_string(),
            required: rules(&[("name", &["string"]), ("version", &["string", "null"])]),
            remote_required: rules(&[("type", &["string"]), ("url", &["string"])]),
        }
    }

    fn object(text: &str) -> JsonObject {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn the_vendored_check_reads_the_schema_lists_the_build_spliced() {
        let check = crate::web_audit::registry::check_by_id("mcp-server-card").unwrap();
        let w = ServerCardWith::parse(check.with_json);
        assert_eq!(w.retained, "server-card");
        let fields = |rules: &[CardFieldRule]| -> Vec<String> {
            rules.iter().map(|r| r.field.clone()).collect()
        };
        assert_eq!(
            fields(&w.required),
            ["$schema", "description", "name", "version"]
        );
        assert_eq!(fields(&w.remote_required), ["type", "url"]);
        assert!(
            w.required
                .iter()
                .chain(&w.remote_required)
                .all(|r| r.types == ["string"])
        );
    }

    #[test]
    fn problems_name_missing_fields_then_mistyped_ones_then_each_remote() {
        assert!(sep2127_problems(&object(r#"{"name":"x","version":null}"#), &with()).is_empty());
        assert_eq!(
            sep2127_problems(&object(r#"{"version":1}"#), &with()),
            [
                "missing required field name",
                "version is not string or null"
            ]
        );
        assert_eq!(
            sep2127_problems(&object(r#"{"remotes":{}}"#), &with()),
            [
                "missing required fields name, version",
                "remotes is not an array"
            ]
        );
        assert_eq!(
            sep2127_problems(
                &object(
                    r#"{"name":"x","version":"1","remotes":[7,{"type":"sse"},{"type":"x","url":3}]}"#
                ),
                &with()
            ),
            [
                "remotes[0] is not an object",
                "remotes[1] missing required field url",
                "remotes[2] url is not string"
            ]
        );
    }

    #[test]
    fn json_types_admit_what_javascript_admits() {
        assert!(has_type(&json!(2.0), "integer"));
        assert!(!has_type(&json!(2.5), "integer"));
        assert!(has_type(&json!(2.5), "number"));
        assert!(!has_type(&json!([]), "object"));
        assert!(has_type(&Value::Null, "null"));
        assert!(!has_type(&json!("x"), "text"));
    }

    #[test]
    fn a_server_info_card_with_no_endpoint_field_scores_as_sep_1649() {
        let card = object(r#"{"serverInfo":{"name":"x"}}"#);
        assert_eq!(
            scored_shape(Some(&card), CardShape::Unrecognized),
            CardShape::Sep1649
        );
        assert_eq!(
            scored_shape(Some(&card), CardShape::Sep2127),
            CardShape::Sep2127
        );
        assert_eq!(
            scored_shape(
                Some(&object(r#"{"serverInfo":"x"}"#)),
                CardShape::Unrecognized
            ),
            CardShape::Unrecognized
        );
    }

    #[test]
    fn the_evidence_line_reads_url_status_and_why() {
        let line = |items: Vec<Value>| {
            let evidence = items
                .into_iter()
                .map(|v| v.as_object().unwrap().clone())
                .collect();
            server_card_evidence(&ProbeOutcome::new(ProbeStatus::Broken, evidence))
        };
        assert_eq!(
            line(vec![
                json!({"retained": "server-card", "why": ["discovery found no server card"]})
            ]),
            "discovery found no server card"
        );
        assert_eq!(
            line(vec![
                json!({"url": "https://x/c", "status": 200, "why": []})
            ]),
            "https://x/c -> 200"
        );
        assert_eq!(
            line(vec![
                json!({"url": "https://x/c", "status": null, "why": ["a", "b"]})
            ]),
            "https://x/c -> error (a; b)"
        );
        assert_eq!(line(vec![]), "");
    }
}
