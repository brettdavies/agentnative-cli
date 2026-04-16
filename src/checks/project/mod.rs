pub mod agents_md;
pub mod completions;
pub mod dependencies;
pub mod dry_run;
pub mod error_module;
pub mod non_interactive;

use crate::check::Check;

pub fn all_project_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(agents_md::AgentsMdCheck),
        Box::new(non_interactive::NonInteractiveSourceCheck),
        Box::new(completions::CompletionsCheck),
        Box::new(dependencies::DependenciesCheck),
        Box::new(error_module::ErrorModuleCheck),
        Box::new(dry_run::DryRunCheck),
    ]
}
