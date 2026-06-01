pub mod agents_md;
pub mod bundle_exists;
pub mod completions;
pub mod dependencies;
pub mod dry_run;
pub mod error_module;
pub mod non_interactive;
pub mod schema_file;

use crate::audit::Audit;

pub fn all_project_audits() -> Vec<Box<dyn Audit>> {
    vec![
        Box::new(agents_md::AgentsMdAudit),
        Box::new(non_interactive::NonInteractiveSourceAudit),
        Box::new(completions::CompletionsAudit),
        Box::new(dependencies::DependenciesAudit),
        Box::new(error_module::ErrorModuleAudit),
        Box::new(dry_run::DryRunAudit),
        Box::new(schema_file::SchemaFileAudit),
        Box::new(bundle_exists::BundleExistsAudit),
    ]
}
