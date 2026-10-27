//! The normalized registry and remediation types the validators produce and the emitter reads.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpDiscovery {
    pub ai_catalog: String,
    pub card_suffix: String,
    pub well_known: Vec<String>,
    pub common_paths: Vec<String>,
    pub protocol_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternSpec {
    pub field: String,
    pub source: String,
    pub flags: String,
    pub rust: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedCheck {
    pub id: String,
    pub category: String,
    pub tier: String,
    pub keyword: String,
    pub principle: String,
    pub site_types: Vec<String>,
    pub antecedent: String,
    pub eval: Option<String>,
    pub eval_supported: bool,
    pub weight: u64,
    pub title: String,
    pub breadcrumb: String,
    pub hint: String,
    /// The `mcp_lanes` key an `mcp`-category check groups under (display only).
    pub lane: Option<String>,
    pub handler: String,
    pub handler_supported: bool,
    pub with_json: String,
    pub patterns: Vec<PatternSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRegistry {
    pub version: u64,
    pub mcp_discovery: McpDiscovery,
    pub category_order: Vec<String>,
    /// `(slug, display name)` in `category_order` order.
    pub categories: Vec<(String, String)>,
    pub checks: Vec<NormalizedCheck>,
    /// `mcp_lanes` in declaration order (display only).
    pub mcp_lanes: Vec<McpLane>,
    /// Retired check ids, outside `checks`; remediation keeps an entry for each.
    pub retired: Vec<RetiredCheck>,
    /// Alternative site designs the global score counts one variant of.
    pub alternatives: Vec<AlternativeGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpLane {
    pub id: String,
    pub label: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetiredCheck {
    pub id: String,
    pub successor: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlternativeVariant {
    pub name: String,
    pub antecedents: Vec<String>,
    /// The tokens whose applied check presents this variant; defaults to
    /// `antecedents`.
    pub presented_by: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlternativeGroup {
    pub group: String,
    pub variants: Vec<AlternativeVariant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemediationResource {
    pub label: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemediationEntry {
    pub id: String,
    pub title: String,
    pub goal: String,
    pub fix: String,
    pub resources: Vec<RemediationResource>,
}
