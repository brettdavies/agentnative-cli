pub mod behavioral;
pub mod project;
pub mod source;

use crate::check::Check;
use crate::project::Language;

/// Every check the linter can run, across every language dispatch. Used by
/// the matrix generator so the coverage artifact reflects the full catalog
/// regardless of what project `anc` currently has in hand.
pub fn all_checks_catalog() -> Vec<Box<dyn Check>> {
    let mut all: Vec<Box<dyn Check>> = Vec::new();
    all.extend(behavioral::all_behavioral_checks());
    all.extend(project::all_project_checks());
    all.extend(source::all_source_checks(Language::Rust));
    all.extend(source::all_source_checks(Language::Python));
    all
}
