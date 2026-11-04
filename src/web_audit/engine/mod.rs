//! The run: discovery, a schedule that starts each check as soon as what it
//! reads has settled, antecedent gating, concurrency, deadlines and scorecard
//! assembly, everything except the handlers themselves. A mirror of the site's
//! `engine.ts`, with the site's async concurrency mapped to one worker pool per
//! audit whose deadline is enforced at request issue time.

pub mod antecedents;
pub mod antecedents_mcp;
pub mod api_catalog;
pub mod api_follow;
pub mod api_targets;
pub mod direct_source;
pub mod discovery;
pub mod discovery_documents;
pub mod discovery_posts;
pub mod discovery_walk;
pub mod endpoint_of_record;
pub mod follow;
pub mod follow_cards;
pub mod follow_hops;
pub mod follow_refusals;
pub mod follow_requests;
pub mod follow_trail;
pub mod handlers;
pub mod mcp_wire;
pub mod pool;
pub mod prerequisites;
pub mod probe;
pub mod provenance;
pub mod reciprocity;
pub mod reciprocity_metadata;
pub mod retained;
pub mod run;
mod schedule;
pub mod sign_in;
pub mod slots;
pub mod summary;
pub mod types;

pub use handlers::{Dispatch, HandlerFn, HandlerSet};
pub use probe::EXTERNAL_DNS_WITHHELD;
pub use run::{
    DEFAULT_CONCURRENCY, DEFAULT_PER_AUDIT_DEADLINE, DEFAULT_PER_CHECK_TIMEOUT,
    DEGRADED_PER_CHECK_TIMEOUT, ProgressSink, RunInput, RunOutcome, RunReport, Unported,
    run_web_audit,
};
pub use types::{
    FetchHandle, HandlerContext, McpAuthRequired, McpLaneEvidence, McpModernLane, ProbeOutcome,
    ProbeStatus, why_item,
};
