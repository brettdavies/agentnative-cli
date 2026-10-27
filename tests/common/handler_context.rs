//! A handler context over a mock transport, for calling one handler
//! directly instead of replaying a whole run.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentnative::web_audit::engine::{FetchHandle, HandlerContext, McpLaneEvidence};
use agentnative::web_audit::locality::Locality;
use agentnative::web_audit::registry::{WebCheck, check_by_id};
use agentnative::web_audit::transport::Transport;

use super::corpus::PublicResolver;

pub const BASE: &str = "https://example.com/";

pub fn check(id: &str) -> &'static WebCheck {
    check_by_id(id).unwrap_or_else(|| panic!("no registry check {id}"))
}

/// A public-target context at [`BASE`] with no root, endpoint, or retained
/// body or document.
pub fn context(transport: Arc<dyn Transport + Send + Sync>) -> HandlerContext {
    HandlerContext {
        base: BASE.to_string(),
        host: "example.com".to_string(),
        mcp_endpoint: None,
        protocol_version: "2025-06-18",
        default_timeout: Duration::from_secs(2),
        root: None,
        scoped_dirs: Arc::new(Vec::new()),
        retained_bodies: Arc::new(HashMap::new()),
        retained_documents: Arc::new(HashMap::new()),
        api_targets: None,
        api_description_bodies: Arc::new(HashMap::new()),
        api_host_probes: Arc::default(),
        fetch: FetchHandle::new(transport, Arc::new(PublicResolver), "test-ua"),
        mcp_session_id: None,
        mcp_lanes: McpLaneEvidence::default(),
        deadline: Instant::now() + Duration::from_secs(20),
        external_dns: false,
        target_locality: Some(Locality::Public),
    }
}
