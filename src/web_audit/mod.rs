//! The local web audit engine behind `anc web`.
//!
//! The check definitions come from [`registry`], compiled at build time from
//! the vendored anc.dev registry; [`score`] and [`scorecard`] mirror the
//! site's scoring model and scorecard shape. Those three build in every
//! configuration. The networking engine builds with the `web` feature.
#![cfg_attr(
    feature = "web",
    doc = "",
    doc = "Probes flow through one blocking, single-hop [`transport::Transport`]",
    doc = "(ureq over rustls, trusting the set [`fetch::trust`] picks per locality class),",
    doc = "a guarded fetch layer in [`fetch`]",
    doc = "that follows redirects itself so every hop is checked, and the",
    doc = "[`locality`] classifier that decides which hosts count as local, public,",
    doc = "or cloud-metadata before any connection is made."
)]

#[cfg(feature = "web")]
pub mod emit;
#[cfg(feature = "web")]
pub mod engine;
#[cfg(feature = "web")]
pub mod fetch;
#[cfg(feature = "web")]
pub mod handlers;
#[cfg(feature = "web")]
pub mod headers;
#[cfg(feature = "web")]
pub mod locality;
#[cfg(feature = "web")]
pub mod mock;
#[cfg(feature = "web")]
mod pattern;
pub mod registry;
#[cfg(feature = "web")]
pub mod render;
pub mod score;
pub mod scorecard;
#[cfg(feature = "web")]
pub mod target;
#[cfg(feature = "web")]
mod tls;
#[cfg(feature = "web")]
pub mod transport;
pub mod universe;
