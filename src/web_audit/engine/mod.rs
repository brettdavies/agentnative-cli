//! The wave scheduler: discovery, antecedent gating, concurrency, deadlines
//! and scorecard assembly, everything except the handlers themselves. A
//! mirror of the site's `engine.ts`, with the site's async concurrency
//! mapped to a bounded thread pool whose deadline is enforced at request
//! issue time.

pub mod antecedents;
pub mod discovery;
pub mod handlers;
pub mod mcp_wire;
pub mod pool;
pub mod retained;
pub mod summary;
pub mod types;
pub mod waves;

pub use handlers::{Dispatch, HandlerFn, HandlerSet};
pub use types::{
    FetchHandle, HandlerContext, McpLaneEvidence, McpModernLane, ProbeOutcome, ProbeStatus,
    why_item,
};
pub use waves::{
    DEFAULT_CONCURRENCY, DEFAULT_PER_AUDIT_DEADLINE, DEFAULT_PER_CHECK_TIMEOUT,
    DEGRADED_PER_CHECK_TIMEOUT, EXTERNAL_DNS_WITHHELD, ProgressSink, RunInput, RunOutcome,
    RunReport, Unported, run_web_audit,
};
