//! YAML loading shared by the build-time parsers.
//!
//! The options hold the vendored inputs to YAML 1.2 core-schema typing:
//! `yes`/`no`/`on`/`off` stay strings, `<<` is an ordinary key rather than a
//! merge, `.inf`/`.nan` load as JSON `null`, and an application tag such as
//! `!foo` is an error because it has no JSON form.

use serde::de::DeserializeOwned;
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
