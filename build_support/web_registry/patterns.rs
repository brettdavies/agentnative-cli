//! Regex translation: each regex-bearing `with` value, translated and compiled at build time.

use serde_json::Value;

use super::PatternSpec;
use super::common::{Mapping, check_err};
use super::vocab::PATTERN_FIELDS;
use crate::js_regex;

/// Translate and compile every regex-bearing `with` value.
pub(super) fn collect_patterns(id: &str, with: &Mapping) -> Result<Vec<PatternSpec>, String> {
    let mut out = Vec::new();
    for (path, field, flags) in PATTERN_FIELDS {
        let mut cursor: Option<&Value> = None;
        for (i, key) in path.iter().enumerate() {
            let map = if i == 0 {
                Some(with)
            } else {
                cursor.and_then(Value::as_object)
            };
            cursor = map.and_then(|m| m.get(*key));
            if cursor.is_none() {
                break;
            }
        }
        let Some(value) = cursor else { continue };
        let source = value
            .as_str()
            .ok_or_else(|| check_err(id, format!("{field} must be a string pattern")))?;
        let rust = js_regex::translate(source, flags)
            .map_err(|e| check_err(id, format!("{field} {source:?}: {e}")))?;
        regex::Regex::new(&rust).map_err(|e| {
            check_err(
                id,
                format!("{field} {source:?} does not compile as a Rust regex: {e}"),
            )
        })?;
        out.push(PatternSpec {
            field: field.to_string(),
            source: source.to_string(),
            flags: flags.to_string(),
            rust,
        });
    }
    Ok(out)
}
