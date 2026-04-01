pub mod python;
pub mod rust;

use crate::check::Check;
use crate::project::Language;

/// Returns all source checks for the given language.
pub fn all_source_checks(language: Language) -> Vec<Box<dyn Check>> {
    match language {
        Language::Rust => rust::all_rust_checks(),
        Language::Python => python::all_python_checks(),
        _ => vec![],
    }
}
