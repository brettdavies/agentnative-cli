//! `{ua:...}` token expansion in a check's `with.headers`.

use serde_json::Value;

use super::common::{Mapping, check_err};

/// Expand a `{ua:...}` token in `with.headers` (any casing of `user-agent`)
/// against the vendored map; a literal User-Agent value is a build error.
pub(super) fn expand_probe_user_agent(
    id: &str,
    with: &Mapping,
    ua_tokens: &[(String, String)],
) -> Result<Mapping, String> {
    let Some(headers) = with.get("headers").and_then(Value::as_object) else {
        return Ok(with.clone());
    };
    let Some((ua_key, ua_value)) = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
    else {
        return Ok(with.clone());
    };
    let value = ua_value.as_str().unwrap_or_default();
    let known: Vec<&str> = ua_tokens.iter().map(|(k, _)| k.as_str()).collect();
    let expanded = ua_tokens
        .iter()
        .find(|(k, _)| k == value)
        .map(|(_, v)| v.clone())
        .ok_or_else(|| {
            check_err(
                id,
                format!(
                    "User-Agent {value:?} must be a {{ua:...}} token from src/shared/user-agents.ts (known: {})",
                    known.join(", ")
                ),
            )
        })?;
    let mut headers = headers.clone();
    headers.insert(ua_key.clone(), Value::String(expanded));
    let mut out = with.clone();
    out.insert("headers".to_string(), Value::Object(headers));
    Ok(out)
}
