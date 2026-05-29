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
