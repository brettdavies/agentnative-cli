//! The probe handlers, one module per registry handler kind, each a port
//! of the site's `handlers/<kind>.ts`. A handler is a function from a
//! check and the run's context to an outcome; the engine dispatches by
//! the check's handler kind through a [`HandlerSet`].

pub mod api_hygiene;
pub mod assert;
pub mod auth_md;
pub mod content_without_js;
pub mod cors_preflight;
pub mod dns_doh;
pub mod http;
pub mod llms_txt_quality;
pub mod markdown_frontmatter;
pub mod scoped_llms;
pub mod shared;
pub mod webmcp;

use crate::web_audit::engine::HandlerSet;

/// Every stateless handler kind and the `legacy-alias-redirects` rule,
/// registered under their registry spellings.
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
        .register_eval("legacy-alias-redirects", http::run_legacy_alias_redirects);
    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::registry::{EVAL_RULES, HANDLER_KINDS};

    #[test]
    fn every_registry_kind_but_mcp_is_registered() {
        let set = stateless_handlers();
        for kind in HANDLER_KINDS {
            assert_eq!(set.supports_kind(kind), *kind != "mcp", "{kind}");
        }
        assert_eq!(EVAL_RULES, &["legacy-alias-redirects", "scoped-discovery"]);
    }
}
