//! The strict read of the contract's `constants.json`, and its emission. Every key is
//! bound here: an engine setting the site changes reaches the engine through these
//! constants, so a key this build does not read stops the build instead of changing
//! nothing.

use serde_json::{Map, Value};

/// How a key is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    /// A count or a byte size.
    Size,
    /// Milliseconds, emitted as a `Duration` under the name without `_MS`.
    Millis,
}

const USER_AGENT: &str = "AUDIT_USER_AGENT";
const UA_TOKENS: &str = "PROBE_UA_TOKENS";

/// Every key but `USER_AGENT` and `UA_TOKENS`, which `emit_rust` documents itself.
const KEYS: &[(&str, Kind)] = &[
    ("REGISTRY_FINGERPRINT", Kind::Text),
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
    items: Vec<(&'static str, Kind, Value)>,
}

const REFRESH: &str =
    "run `scripts/sync-web-audit.sh`, or bind the key in build_support/web_registry/constants.rs";

fn take(raw: &mut Map<String, Value>, key: &str) -> Result<Value, String> {
    raw.remove(key)
        .ok_or_else(|| format!("constants.json: missing \"{key}\"; {REFRESH}"))
}

fn wrong_shape(key: &str, value: &Value) -> String {
    format!("constants.json: \"{key}\" has the wrong shape: {value}")
}

/// Read `constants.json`. A missing key or a key no binding reads fails naming it.
pub fn read_constants(json: &str) -> Result<Constants, String> {
    let mut raw: Map<String, Value> =
        serde_json::from_str(json).map_err(|e| format!("constants.json: {e}"))?;
    let agent = take(&mut raw, USER_AGENT)?;
    let audit_user_agent = agent
        .as_str()
        .ok_or_else(|| wrong_shape(USER_AGENT, &agent))?
        .to_string();
    let tokens = take(&mut raw, UA_TOKENS)?;
    let mut probe_ua_tokens = tokens
        .as_object()
        .and_then(|map| {
            map.iter()
                .map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect::<Option<Vec<_>>>()
        })
        .ok_or_else(|| wrong_shape(UA_TOKENS, &tokens))?;
    probe_ua_tokens.sort();
    let mut items = Vec::new();
    for (key, kind) in KEYS {
        let value = take(&mut raw, key)?;
        let fits = match kind {
            Kind::Text => value.is_string(),
            Kind::Size | Kind::Millis => value.is_u64(),
        };
        if !fits {
            return Err(wrong_shape(key, &value));
        }
        items.push((*key, *kind, value));
    }
    if let Some(key) = raw.keys().next() {
        return Err(format!(
            "constants.json: \"{key}\" is a key nothing in this build reads; {REFRESH}"
        ));
    }
    Ok(Constants {
        audit_user_agent,
        probe_ua_tokens,
        items,
    })
}

impl Constants {
    /// The generated constant for every key in `KEYS`.
    pub(super) fn emit(&self, src: &mut String) {
        for (key, kind, value) in &self.items {
            match kind {
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
