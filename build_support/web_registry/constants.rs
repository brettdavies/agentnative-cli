//! The strict read of the contract's `constants.json`, and its emission. Every key is
//! bound here: an engine setting the site changes reaches the engine through these
//! constants, so a key this build does not read stops the build instead of changing
//! nothing.

use serde_json::{Map, Value};

/// How a key is emitted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    TokenMap,
    /// A count or a byte size.
    Size,
    /// Milliseconds, emitted as a `Duration` under the name without `_MS`.
    Millis,
}

const KEYS: &[(&str, Kind)] = &[
    ("REGISTRY_FINGERPRINT", Kind::Text),
    ("AUDIT_USER_AGENT", Kind::Text),
    ("PROBE_UA_TOKENS", Kind::TokenMap),
    ("DEFAULT_CONCURRENCY", Kind::Size),
    ("DEFAULT_PER_CHECK_TIMEOUT_MS", Kind::Millis),
    ("DEFAULT_PER_AUDIT_DEADLINE_MS", Kind::Millis),
    ("DEGRADED_PER_CHECK_TIMEOUT_MS", Kind::Millis),
    ("DISCOVERY_BUDGET_MS", Kind::Millis),
    ("DEFAULT_TIMEOUT_MS", Kind::Millis),
    ("DEFAULT_MAX_REDIRECTS", Kind::Size),
    ("STATUS_ONLY_BODY_BYTES", Kind::Size),
    ("AUDIT_PROBE_MAX_BODY_BYTES", Kind::Size),
    ("DOCUMENT_MAX_BODY_BYTES", Kind::Size),
    ("METADATA_MAX_BODY_BYTES", Kind::Size),
    ("OPENAPI_MAX_BODY_BYTES", Kind::Size),
    ("MEMO_BODY_MAX_BYTES", Kind::Size),
    ("MEMO_TOTAL_MAX_BYTES", Kind::Size),
    ("FOLLOW_SLICE_MS", Kind::Millis),
    ("ENDPOINT_GET_TIMEOUT_MS", Kind::Millis),
    ("CARD_DOCUMENT_TIMEOUT_MS", Kind::Millis),
    ("DESCRIPTION_TIMEOUT_MS", Kind::Millis),
    ("MAX_FOLLOWED_HOSTS", Kind::Size),
    ("MAX_FOLLOW_REQUESTS", Kind::Size),
    ("MAX_CATALOG_CARDS", Kind::Size),
    ("MAX_CATALOG_ANCHORS", Kind::Size),
    ("MIN_VISIBLE_CHARS", Kind::Size),
    ("MAX_TWIN_PROBES", Kind::Size),
    ("MIN_TWIN_CHARS", Kind::Size),
    ("DEFAULT_MAX_LINKS", Kind::Size),
    ("DEFAULT_MAX_CANDIDATES", Kind::Size),
    ("MAX_AUTHORIZATION_SERVERS", Kind::Size),
    ("MAX_URL_LENGTH", Kind::Size),
    ("MAX_SHOWN_LENGTH", Kind::Size),
];

/// A read `constants.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constants {
    /// The identifying User-Agent every probe sends unless a check sets its own.
    pub audit_user_agent: String,
    /// Registry `{ua:...}` token to literal User-Agent, sorted.
    pub probe_ua_tokens: Vec<(String, String)>,
    items: Vec<(&'static str, Value)>,
}

const REFRESH: &str =
    "run `scripts/sync-web-audit.sh`, or bind the key in build_support/web_registry/constants.rs";

/// Read `constants.json`. A missing key or a key no binding reads fails naming it.
pub fn read_constants(json: &str) -> Result<Constants, String> {
    let mut raw: Map<String, Value> =
        serde_json::from_str(json).map_err(|e| format!("constants.json: {e}"))?;
    let mut items = Vec::new();
    for (key, kind) in KEYS {
        let value = raw
            .remove(*key)
            .ok_or_else(|| format!("constants.json: missing \"{key}\"; {REFRESH}"))?;
        let fits = match kind {
            Kind::Text => value.is_string(),
            Kind::TokenMap => value
                .as_object()
                .is_some_and(|map| map.values().all(Value::is_string)),
            Kind::Size | Kind::Millis => value.is_u64(),
        };
        if !fits {
            return Err(format!(
                "constants.json: \"{key}\" has the wrong shape: {value}"
            ));
        }
        items.push((*key, value));
    }
    if let Some(key) = raw.keys().next() {
        return Err(format!(
            "constants.json: \"{key}\" is a key nothing in this build reads; {REFRESH}"
        ));
    }
    let text = |key: &str| -> String {
        items
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    let audit_user_agent = text("AUDIT_USER_AGENT");
    let mut probe_ua_tokens: Vec<(String, String)> = items
        .iter()
        .find(|(k, _)| *k == "PROBE_UA_TOKENS")
        .and_then(|(_, v)| v.as_object())
        .map(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default();
    probe_ua_tokens.sort();
    Ok(Constants {
        audit_user_agent,
        probe_ua_tokens,
        items,
    })
}

impl Constants {
    /// The generated constant for every key but the two `emit_rust` documents itself.
    pub(super) fn emit(&self, src: &mut String) {
        for (key, value) in &self.items {
            let kind = KEYS
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, kind)| *kind)
                .expect("a bound key");
            match kind {
                Kind::TokenMap => {}
                Kind::Text if *key == "AUDIT_USER_AGENT" => {}
                Kind::Text => {
                    let text = value.as_str().unwrap_or_default();
                    src.push_str(&format!(
                        "/// The site's `{key}`.\npub const {key}: &str = {text:?};\n"
                    ));
                }
                Kind::Size => {
                    src.push_str(&format!(
                        "/// The site's `{key}`.\npub const {key}: usize = {value};\n"
                    ));
                }
                Kind::Millis => {
                    let name = key.trim_end_matches("_MS");
                    src.push_str(&format!(
                        "/// The site's `{key}`.\npub const {name}: std::time::Duration = std::time::Duration::from_millis({value});\n"
                    ));
                }
            }
        }
    }
}
