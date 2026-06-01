//! Principle registry + matrix generator.
//!
//! The registry is the single source of truth linking spec requirements
//! (MUSTs, SHOULDs, MAYs across P1–P7) to the audits that verify them.
//! `Audit::covers()` declares which requirement IDs an audit evidences; the
//! matrix generator inverts that mapping to produce coverage artifacts.

pub mod matrix;
pub mod registry;

#[allow(unused_imports)] // Re-exports used by downstream code + tests.
pub use registry::{Applicability, ExceptionCategory, Level, REQUIREMENTS, Requirement};
