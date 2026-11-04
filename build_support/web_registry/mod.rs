//! Web-audit registry codegen used by `build.rs` to generate
//! `$OUT_DIR/generated_web_registry.rs` from the site's vendoring contract under
//! `src/web_audit/vendored/contract/`.
//!
//! The site normalizes its registry and writes the result into the contract, so
//! nothing here re-derives a value. The read is strict: a key this build does not
//! know, an enumerated value outside its vocabulary, or a `with` value a ported handler
//! could only misread fails naming the entry, so a contract the build cannot read stops
//! it. Cross-field rules the site's normalizer owns are tested over the contract in
//! `tests/web_audit_contract.rs`. Two things stay here because only Rust
//! can do them: every regex-bearing value is translated and compiled with the `regex`
//! crate (`js_regex`), and an entry naming a handler kind or eval rule outside the
//! ported set binds to `Unsupported` instead of failing, so the engine reports it as a
//! skip at runtime rather than holding the vendor pin hostage.
//!
//! Emission order is semantic for `CHECKS` (scorecard rows follow registry
//! order) and sorted everywhere else (remediation, token map).

mod constants;
mod emit;
mod model;
mod patterns;
mod read;
mod vocab;

pub use constants::{Constants, read_constants};
pub use emit::emit_rust;
pub use model::{
    AlternativeGroup, AlternativeVariant, McpDiscovery, McpLane, NormalizedCheck,
    NormalizedRegistry, PatternSpec, RemediationEntry, RetiredCheck,
};
pub use read::{read_registry, read_remediation};
pub use vocab::{EVAL_RULES, HANDLER_KINDS};
