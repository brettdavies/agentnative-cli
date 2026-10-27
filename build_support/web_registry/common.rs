//! Helpers every validator shares: the `Mapping` alias, key and id checks, and error formatting.

use serde_json::Value;

pub(super) type Mapping = serde_json::Map<String, Value>;

pub(super) fn unknown_key<'a>(map: &'a Mapping, allowed: &[&str]) -> Option<&'a str> {
    map.keys()
        .map(String::as_str)
        .find(|k| !allowed.contains(k))
}

pub(super) fn check_err(id: &str, what: impl std::fmt::Display) -> String {
    format!("web-audit registry: check \"{id}\" {what}")
}

pub(super) fn is_check_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub(super) fn yaml_display(v: Option<&Value>) -> String {
    match v {
        None => "null".to_string(),
        Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "null".to_string()),
    }
}
