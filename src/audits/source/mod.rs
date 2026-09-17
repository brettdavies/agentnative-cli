// Every audit under this module reads arbitrary UTF-8 from the audited tree.
// A byte-indexed slice is sound only when its bounds come from a scan of that
// same text, and each such site carries an `expect` naming the scan.
#![deny(clippy::string_slice)]

pub mod python;
pub mod rust;

use crate::audit::Audit;
use crate::project::Language;

/// Returns all source audits for the given language.
pub fn all_source_audits(language: Language) -> Vec<Box<dyn Audit>> {
    match language {
        Language::Rust => rust::all_rust_audits(),
        Language::Python => python::all_python_audits(),
        _ => vec![],
    }
}
