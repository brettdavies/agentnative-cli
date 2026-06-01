pub mod behavioral;
pub mod project;
pub mod source;

use crate::audit::Audit;
use crate::project::Language;

/// Every audit the linter can run, across every language dispatch. Used by
/// the matrix generator so the coverage artifact reflects the full catalog
/// regardless of what project `anc` currently has in hand.
pub fn all_audits_catalog() -> Vec<Box<dyn Audit>> {
    let mut all: Vec<Box<dyn Audit>> = Vec::new();
    all.extend(behavioral::all_behavioral_audits());
    all.extend(project::all_project_audits());
    all.extend(source::all_source_audits(Language::Rust));
    all.extend(source::all_source_audits(Language::Python));
    all
}
