//! The probe handlers, one module per registry handler kind, each a port
//! of the site's `handlers/<kind>.ts`. A handler is a function from a
//! check and the run's context to an outcome; the engine dispatches by
//! the check's handler kind through a [`HandlerSet`].

pub mod api_description;
pub mod api_hygiene;
pub mod api_probe_url;
pub mod assert;
pub mod auth_md;
pub mod content_without_js;
pub mod cors_preflight;
pub mod dns_doh;
pub mod http;
pub mod llms_txt_quality;
pub mod markdown_frontmatter;
pub mod mcp;
pub mod protected_resource;
pub mod scoped_llms;
pub mod server_card;
pub mod shared;
pub mod targets;
pub mod webmcp;

use crate::web_audit::engine::HandlerSet;

/// Every stateless handler kind and the `api-description`,
/// `legacy-alias-redirects` and `retained-document` rules, registered under
/// their registry spellings.
pub fn stateless_handlers() -> HandlerSet {
    let mut set = HandlerSet::new();
    set.register("http", http::run_http)
        .register("cors-preflight", cors_preflight::run_cors_preflight)
        .register("dns-doh", dns_doh::run_dns_doh)
        .register("auth-md", auth_md::run_auth_md)
        .register("webmcp", webmcp::run_webmcp)
        .register("scoped-llms", scoped_llms::run_scoped_llms)
        .register(
            "markdown-frontmatter",
            markdown_frontmatter::run_markdown_frontmatter,
        )
        .register(
            "content-without-js",
            content_without_js::run_content_without_js,
        )
        .register("llms-txt-quality", llms_txt_quality::run_llms_txt_quality)
        .register("api-hygiene", api_hygiene::run_api_hygiene)
        .register(
            "protected-resource",
            protected_resource::run_protected_resource,
        )
        .register("server-card", server_card::run_server_card)
        .register_eval("api-description", api_description::run_api_description)
        .register_eval("legacy-alias-redirects", http::run_legacy_alias_redirects)
        .register_eval("retained-document", http::run_retained_document);
    set
}

/// Every registry handler kind, the full set a run dispatches to.
pub fn default_handlers() -> HandlerSet {
    let mut set = stateless_handlers();
    set.register("mcp", mcp::run_mcp);
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::registry::{EVAL_RULES, HANDLER_KINDS};

    #[test]
    fn the_stateless_set_omits_mcp_and_the_default_set_covers_every_kind() {
        let stateless = stateless_handlers();
        let default = default_handlers();
        for kind in HANDLER_KINDS {
            assert_eq!(stateless.supports_kind(kind), *kind != "mcp", "{kind}");
            assert!(default.supports_kind(kind), "{kind}");
        }
        assert_eq!(
            EVAL_RULES,
            &[
                "api-description",
                "legacy-alias-redirects",
                "retained-document",
                "scoped-discovery"
            ]
        );
    }
}
