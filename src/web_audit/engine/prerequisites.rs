//! What must settle before a check can gate or probe, a mirror of the site's
//! `row-prerequisites.ts`: what its antecedent token reads, plus what its
//! handler reads from the context. A new check takes its edges from its token
//! and handler, so it never names them itself.

use super::antecedents;
use crate::web_audit::registry::{HandlerBinding, WebCheck};

/// One step a row can wait on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Prerequisite {
    /// A source check's probe.
    Source(&'static str),
    /// Both wire probes answered, sign-in settled, their rows read again if it
    /// is required, and the session and lane facts taken.
    McpSettled,
    /// The session notified, after [`Prerequisite::McpSettled`]. The protocol
    /// has a client send nothing else on a session until the server has that
    /// notification, so every later request to the endpoint comes after it.
    McpNotified,
}

use Prerequisite::{McpNotified, McpSettled, Source};

fn handler_prerequisites(handler: HandlerBinding) -> &'static [Prerequisite] {
    use HandlerBinding as H;
    match handler {
        H::Http
        | H::DnsDoh
        | H::AuthMd
        | H::Webmcp
        | H::MarkdownFrontmatter
        | H::ServerCard
        | H::Unsupported(_) => &[],
        H::Mcp | H::CorsPreflight => &[McpNotified],
        H::ProtectedResource => &[McpSettled],
        H::ContentWithoutJs | H::LlmsTxtQuality => &[Source("llms-txt")],
        H::ScopedLlms => &[Source("llms-txt"), Source("sitemap")],
        H::ApiHygiene => &[Source("openapi")],
    }
}

/// Everything `check` waits on before it gates, each once.
pub fn row_prerequisites(check: &WebCheck) -> Vec<Prerequisite> {
    let mut edges: Vec<Prerequisite> = Vec::new();
    for edge in antecedents::prerequisites(check.antecedent)
        .iter()
        .chain(handler_prerequisites(check.handler))
    {
        if !edges.contains(edge) {
            edges.push(*edge);
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::engine::antecedents::{SOURCE_CHECK_IDS, is_source_check};
    use crate::web_audit::registry::{CHECKS, check_by_id};

    fn edges(id: &str) -> Vec<Prerequisite> {
        row_prerequisites(check_by_id(id).expect("a registry check"))
    }

    #[test]
    fn every_source_an_edge_names_is_a_source_check_in_the_registry() {
        for id in SOURCE_CHECK_IDS {
            assert!(check_by_id(id).is_some(), "{id}: not in the registry");
        }
        for check in CHECKS {
            for edge in row_prerequisites(check) {
                if let Source(id) = edge {
                    assert!(is_source_check(id), "{}: waits on {id}", check.id);
                }
            }
        }
    }

    #[test]
    fn every_row_that_can_send_on_the_session_waits_for_the_notification() {
        for check in CHECKS {
            let carries_session = matches!(
                check.handler,
                HandlerBinding::Mcp | HandlerBinding::CorsPreflight
            );
            if carries_session {
                assert!(edges(check.id).contains(&McpNotified), "{}", check.id);
            }
        }
    }

    #[test]
    fn every_mcp_token_waits_for_the_notification_whatever_it_reads() {
        use crate::web_audit::registry::AntecedentToken as T;
        for token in [
            T::McpPresent,
            T::McpAuth,
            T::McpSession,
            T::McpAuthRequired,
            T::McpResources,
        ] {
            assert!(
                antecedents::prerequisites(token).contains(&McpNotified),
                "{token:?}"
            );
        }
    }

    // Each row today also has these through its token, so only this names them
    // as the handler's own.
    #[test]
    fn a_handler_declares_what_it_reads_whatever_its_token_says() {
        for handler in [HandlerBinding::Mcp, HandlerBinding::CorsPreflight] {
            assert_eq!(
                handler_prerequisites(handler),
                &[McpNotified],
                "{handler:?}"
            );
        }
        assert_eq!(
            handler_prerequisites(HandlerBinding::ApiHygiene),
            &[Source("openapi")]
        );
    }

    #[test]
    fn a_row_reading_nothing_starts_with_the_sources() {
        assert_eq!(edges("security-txt"), Vec::new());
    }

    #[test]
    fn a_handlers_edge_counts_even_when_the_antecedent_reads_nothing() {
        let row = check_by_id("content-without-js").expect("a registry check");
        assert!(antecedents::prerequisites(row.antecedent).is_empty());
        assert_eq!(edges("content-without-js"), vec![Source("llms-txt")]);
    }

    #[test]
    fn edges_shared_by_token_and_handler_appear_once() {
        let row = edges("mcp-tools-list");
        assert_eq!(row.iter().filter(|e| **e == McpNotified).count(), 1);
    }
}
