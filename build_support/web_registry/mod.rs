//! Web-audit registry codegen used by `build.rs` to generate
//! `$OUT_DIR/generated_web_registry.rs` from the vendored site inputs under
//! `src/web_audit/vendored/`.
//!
//! Mirrors `agentnative-site:src/build/13-web-audit-registry.mjs`: the same
//! structural validation (every failure names the check id), `keyword`
//! derived from `tier`, `{ua:...}` expansion against the vendored token map,
//! and the remediation catalog held 1:1 against the check set. Two
//! deliberate differences: every regex-bearing value is translated and
//! compiled with the `regex` crate at build time (`js_regex`), and a
//! well-formed entry naming a handler kind or eval rule outside the ported
//! set binds to `Unsupported` instead of failing, so the engine reports it
//! as a skip at runtime rather than holding the vendor pin hostage.
//!
//! Emission order is semantic for `CHECKS` (scorecard rows follow registry
//! order) and sorted everywhere else (remediation, token map).

mod alternatives;
mod card_schema;
mod check;
mod common;
mod emit;
mod model;
mod patterns;
mod registry;
mod remediation;
mod retired;
mod user_agent;
mod vocab;

pub use emit::emit_rust;
pub use model::{
    AlternativeGroup, AlternativeVariant, McpDiscovery, McpLane, NormalizedCheck,
    NormalizedRegistry, PatternSpec, RemediationEntry, RemediationResource, RetiredCheck,
};
pub use registry::normalize_registry;
pub use remediation::normalize_remediation;
pub use vocab::{EVAL_RULES, HANDLER_KINDS};
