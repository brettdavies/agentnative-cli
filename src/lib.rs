#![doc = include_str!("../README.md")]
#![doc = ""]
#![doc = "# Library surface"]
#![doc = ""]
#![doc = "The modules that back the `anc` binary's CLI audit are re-exported"]
#![doc = "unlisted so the binary can reach them; they carry no compatibility promise."]
#![deny(missing_docs)]
#![warn(missing_debug_implementations)]

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
pub mod exit_codes;
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
