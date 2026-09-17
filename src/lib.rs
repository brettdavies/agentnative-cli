//! The agent-native CLI linter, as a library.
//!
//! The public surface is [`web_audit`]: the engine behind `anc web`, which
//! audits an HTTP(S) target for agent readiness with the same check registry
//! and verdict semantics as anc.dev. The remaining modules back the `anc`
//! binary's CLI audit and are re-exported unlisted so the binary can reach
//! them; they carry no compatibility promise.

#[doc(hidden)]
pub mod anc_toml;
#[doc(hidden)]
pub mod argv;
#[doc(hidden)]
pub mod audit;
#[doc(hidden)]
pub mod audits;
#[doc(hidden)]
pub mod build_info;
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod color;
#[doc(hidden)]
pub mod error;
#[doc(hidden)]
pub mod json_error;
#[doc(hidden)]
pub mod output;
#[doc(hidden)]
pub mod principles;
#[doc(hidden)]
pub mod project;
#[doc(hidden)]
pub mod runner;
#[doc(hidden)]
pub mod scorecard;
#[doc(hidden)]
pub mod skill_install;
#[doc(hidden)]
pub mod source;
#[doc(hidden)]
pub mod types;

pub mod web_audit;
