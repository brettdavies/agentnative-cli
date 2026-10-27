//! Holds the hand-written `schema/scorecard.schema.json` to the shape serde
//! emits. `schemars` derives the shape from the serialized scorecard types,
//! and the test compares property names, `required` lists, and the JSON types
//! each property admits, at every depth. The hand-written file stays the
//! published contract: the derive carries none of its descriptions, closed
//! value sets, formats, or `additionalProperties: false`.

use std::collections::BTreeSet;

use schemars::generate::SchemaSettings;
use serde_json::Value;

use super::Scorecard;

const HAND_WRITTEN: &str = include_str!("../../schema/scorecard.schema.json");

fn hand_written() -> Value {
    serde_json::from_str(HAND_WRITTEN).expect("schema/scorecard.schema.json is valid JSON")
}

/// `for_serialize` puts an `Option` field without `skip_serializing_if` in
/// `required`, which is the always-present-null contract.
fn derived() -> Value {
    let schema = SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<Scorecard>();
    serde_json::to_value(schema).expect("a derived schema serializes to JSON")
}

fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    match node.get("$ref").and_then(Value::as_str) {
        Some(r) => resolve(root, &root["$defs"][r.trim_start_matches("#/$defs/")]),
        None => node,
    }
}

fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_f64() => "number",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// The JSON types a node admits, read from `type`, `enum`, `const`, and the
/// branches of `anyOf` / `oneOf`.
fn types(root: &Value, node: &Value) -> BTreeSet<String> {
    let node = resolve(root, node);
    let mut out = BTreeSet::new();
    match node.get("type") {
        Some(Value::String(t)) => {
            out.insert(t.clone());
        }
        Some(Value::Array(ts)) => {
            out.extend(ts.iter().filter_map(|t| t.as_str().map(String::from)));
        }
        _ => {}
    }
    if let Some(Value::Array(values)) = node.get("enum") {
        out.extend(values.iter().map(|v| json_type(v).to_string()));
    }
    if let Some(c) = node.get("const") {
        out.insert(json_type(c).to_string());
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(Value::Array(branches)) = node.get(key) {
            for branch in branches {
                out.extend(types(root, branch));
            }
        }
    }
    out
}

fn names(node: &Value, field: &str) -> BTreeSet<String> {
    match &node[field] {
        Value::Object(m) => m.keys().cloned().collect(),
        Value::Array(a) => a
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => BTreeSet::new(),
    }
}

/// The non-null branch of `anyOf: [X, {"type": "null"}]`, else the node.
fn non_null<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    let node = resolve(root, node);
    let Some(branches) = node.get("anyOf").and_then(Value::as_array) else {
        return node;
    };
    let mut rest = branches.iter().filter(|b| b["type"] != "null");
    match (branches.len(), rest.next(), rest.next()) {
        (2, Some(only), None) => resolve(root, only),
        _ => node,
    }
}

/// Every place the two schemas disagree. For a key outside `required`, both
/// sides compare without `null`: serde omits the key rather than writing
/// `null`, the hand-written schema says so, and the derive admits `null`
/// there regardless.
fn shape_diff(hr: &Value, h: &Value, dr: &Value, d: &Value, path: &str, out: &mut Vec<String>) {
    let (h, d) = (non_null(hr, h), non_null(dr, d));
    if h.get("properties").is_some() || d.get("properties").is_some() {
        let (hp, dp) = (names(h, "properties"), names(d, "properties"));
        for k in hp.symmetric_difference(&dp) {
            let side = if hp.contains(k) {
                "the schema but not the Rust types"
            } else {
                "the Rust types but not the schema"
            };
            out.push(format!("{path}.{k}: declared by {side}"));
        }
        let (h_req, d_req) = (names(h, "required"), names(d, "required"));
        if h_req != d_req {
            out.push(format!(
                "{path}: required lists differ: schema {h_req:?}, serde {d_req:?}"
            ));
        }
        for k in hp.intersection(&dp) {
            let (hc, dc) = (&h["properties"][k], &d["properties"][k]);
            let (mut ht, mut dt) = (types(hr, hc), types(dr, dc));
            if !d_req.contains(k) {
                ht.remove("null");
                dt.remove("null");
            }
            if ht != dt {
                out.push(format!(
                    "{path}.{k}: types differ: schema {ht:?}, serde {dt:?}"
                ));
            }
            shape_diff(hr, hc, dr, dc, &format!("{path}.{k}"), out);
        }
    }
    if let (Some(hi), Some(di)) = (h.get("items"), d.get("items")) {
        shape_diff(hr, hi, dr, di, &format!("{path}[]"), out);
    }
}

#[test]
fn hand_written_schema_matches_the_serialized_shape() {
    let (hand, derived) = (hand_written(), derived());
    let mut out = Vec::new();
    shape_diff(&hand, &hand, &derived, &derived, "$", &mut out);
    assert!(
        out.is_empty(),
        "schema/scorecard.schema.json disagrees with the serialized scorecard types; \
         update the schema (and SCHEMA_VERSION for a shape change):\n  {}",
        out.join("\n  ")
    );
}
