//! The local web audit engine behind `anc web`.
//!
//! Probes flow through one blocking, single-hop [`transport::Transport`]
//! (ureq over rustls with bundled roots), a guarded fetch layer in [`fetch`]
//! that follows redirects itself so every hop is checked, and the
//! [`locality`] classifier that decides which hosts count as local, public,
//! or cloud-metadata before any connection is made. The check definitions
//! come from [`registry`], compiled at build time from the vendored anc.dev
//! registry.

pub mod engine;
pub mod fetch;
pub mod handlers;
pub mod headers;
pub mod locality;
pub mod mock;
pub mod registry;
pub mod score;
pub mod scorecard;
pub mod transport;
