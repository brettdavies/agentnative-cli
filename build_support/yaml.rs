//! YAML loading shared by the build-time parsers (`parser.rs`,
//! `web_registry.rs`).
//!
//! The options hold the vendored inputs to YAML 1.2 core-schema typing:
//! `yes`/`no`/`on`/`off` stay strings, `<<` is an ordinary key rather than a
//! merge, `.inf`/`.nan` load as JSON `null`, and an application tag such as
//! `!foo` is an error because it has no JSON form.

use std::fmt;

use serde::de::{self, Deserialize, DeserializeOwned, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_saphyr::options::{MergeKeyPolicy, NonFiniteFloatPolicy};

/// Deserialize one YAML document; the error is the parser's message.
pub fn from_str<T: DeserializeOwned>(yaml: &str) -> Result<T, String> {
    let options = serde_saphyr::options! {
        strict_booleans: true,
        merge_keys: MergeKeyPolicy::AsOrdinary,
        non_finite_float_policy: NonFiniteFloatPolicy::PassThrough,
        reject_unsupported_tags: true,
    };
    serde_saphyr::from_str_with_options(yaml, options).map_err(|e| e.to_string())
}

/// A YAML document as JSON that refuses a mapping key which is not a string
/// (`1:`, `true:`, `~:`, a sequence) instead of stringifying it, so such a
/// key fails the build.
pub struct StringKeyed(pub serde_json::Value);

struct StringKey(String);

impl<'de> Deserialize<'de> for StringKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeyVisitor;
        impl Visitor<'_> for KeyVisitor {
            type Value = StringKey;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string mapping key")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<StringKey, E> {
                Ok(StringKey(v.to_string()))
            }
        }
        deserializer.deserialize_any(KeyVisitor)
    }
}

impl<'de> Deserialize<'de> for StringKeyed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueVisitor;
        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = StringKeyed;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any YAML value")
            }
            fn visit_bool<E>(self, v: bool) -> Result<StringKeyed, E> {
                Ok(StringKeyed(v.into()))
            }
            fn visit_i64<E>(self, v: i64) -> Result<StringKeyed, E> {
                Ok(StringKeyed(v.into()))
            }
            fn visit_u64<E>(self, v: u64) -> Result<StringKeyed, E> {
                Ok(StringKeyed(v.into()))
            }
            fn visit_f64<E>(self, v: f64) -> Result<StringKeyed, E> {
                Ok(StringKeyed(v.into()))
            }
            fn visit_str<E>(self, v: &str) -> Result<StringKeyed, E> {
                Ok(StringKeyed(v.into()))
            }
            fn visit_unit<E>(self) -> Result<StringKeyed, E> {
                Ok(StringKeyed(serde_json::Value::Null))
            }
            fn visit_none<E>(self) -> Result<StringKeyed, E> {
                Ok(StringKeyed(serde_json::Value::Null))
            }
            fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<StringKeyed, D::Error> {
                StringKeyed::deserialize(d)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<StringKeyed, A::Error> {
                let mut out = Vec::new();
                while let Some(StringKeyed(v)) = seq.next_element()? {
                    out.push(v);
                }
                Ok(StringKeyed(out.into()))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<StringKeyed, A::Error> {
                let mut out = serde_json::Map::new();
                while let Some((StringKey(k), StringKeyed(v))) = map.next_entry()? {
                    out.insert(k, v);
                }
                Ok(StringKeyed(out.into()))
            }
        }
        deserializer.deserialize_any(ValueVisitor)
    }
}
