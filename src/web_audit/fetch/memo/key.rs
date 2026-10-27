//! The memo's identity for a request: what the target sees of it.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

use crate::web_audit::transport::Request;

/// Method, URL, lowercased headers sorted by name, and body. An
/// `Authorization` value enters as a digest, so the key never holds a
/// credential, and the key prints header names only.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct Key {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

impl Key {
    /// The key for `request`, digesting a credential with the memo's own hasher.
    pub(super) fn of(request: &Request, digest: &RandomState) -> Self {
        let mut headers: Vec<(String, String)> = request
            .headers
            .iter()
            .map(|(name, value)| {
                let name = name.to_ascii_lowercase();
                let value = if name == "authorization" {
                    format!("digest:{:016x}", digest.hash_one(value))
                } else {
                    value.clone()
                };
                (name, value)
            })
            .collect();
        headers.sort_by(|a, b| a.0.cmp(&b.0));
        Key {
            method: request.method.to_ascii_uppercase(),
            url: request.url.clone(),
            headers,
            body: request.body.clone(),
        }
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(n, _)| n.as_str()).collect();
        f.debug_struct("Key")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &names)
            .finish_non_exhaustive()
    }
}
