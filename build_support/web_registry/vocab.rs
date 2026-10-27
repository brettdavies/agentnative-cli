//! The closed vocabularies and key sets the registry is validated against.

/// Handler kinds the Rust engine ports, in the site's declaration order.
pub const HANDLER_KINDS: &[&str] = &[
    "http",
    "cors-preflight",
    "mcp",
    "dns-doh",
    "auth-md",
    "webmcp",
    "scoped-llms",
    "markdown-frontmatter",
    "content-without-js",
    "llms-txt-quality",
    "api-hygiene",
    "server-card",
];
/// Eval rules the Rust engine ports.
pub const EVAL_RULES: &[&str] = &[
    "legacy-alias-redirects",
    "retained-document",
    "scoped-discovery",
];
pub(super) const SITE_TYPES: &[&str] = &["content", "api", "mcp", "all"];
pub(super) const ANTECEDENTS: &[&str] = &[
    "none",
    "http-root",
    "html-root",
    "mcp-present",
    "mcp-session",
    "mcp-auth",
    "mcp-auth-required",
    "mcp-resources",
    "api-surface",
    "schemas-ref",
    "docs-site",
    "root-llms-txt",
    "root-llms-full-txt",
    "markdown-twin",
    "robots-present",
    "auth-present",
];
pub(super) const TIERS: &[(&str, &str)] = &[
    ("required", "must"),
    ("recommended", "should"),
    ("optional", "may"),
];
pub(super) const CORS_SURFACES: &[&str] = &["preflight", "actual"];
pub(super) const PROTECTED_RESOURCE_OPS: &[&str] = &["challenge", "metadata"];
/// Documents discovery keeps for a `retained-document` check, mirroring the
/// site's `RETAINED_DOCUMENT_KEYS` (`src/shared/web-audit-documents.ts`).
pub(super) const RETAINED_DOCUMENT_KEYS: &[&str] = &["ai-catalog", "api-catalog", "server-card"];
/// Handlers that score a retained document instead of issuing a request.
pub(super) const RETAINED_DOCUMENT_HANDLERS: &[&str] = &["http", "server-card"];
/// `with` parameters the build splices into a server-card check from the
/// vendored schema; hand-authoring one is a build error.
pub(super) const CARD_FIELD_PARAMS: &[&str] = &["required", "remote_required"];
pub(super) const TOP_LEVEL_KEYS: &[&str] = &[
    "version",
    "mcp_discovery",
    "category_order",
    "categories",
    "mcp_lanes",
    "retired",
    "alternatives",
    "checks",
];
pub(super) const DISCOVERY_KEYS: &[&str] = &[
    "ai_catalog",
    "card_suffix",
    "well_known",
    "common_paths",
    "protocol_version",
];
pub(super) const CHECK_KEYS: &[&str] = &[
    "id",
    "category",
    "tier",
    "principle",
    "site_types",
    "antecedent",
    "eval",
    "lane",
    "weight",
    "title",
    "breadcrumb",
    "hint",
    "handler",
    "with",
];
pub(super) const LANE_FIELDS: &[&str] = &["label", "note"];
pub(super) const RETIRED_FIELDS: &[&str] = &["successor", "reason"];
pub(super) const GROUP_FIELDS: &[&str] = &["group", "variants"];
pub(super) const VARIANT_FIELDS: &[&str] = &["antecedents", "presented_by"];
/// The category whose checks group under a protocol lane.
pub(super) const LANE_CATEGORY: &str = "mcp";
pub(super) const BREADCRUMB_MAX: usize = 40;
/// Regex-bearing `with` values: path inside `with`, short field name, and the
/// JavaScript flags `assert.ts` evaluates the value under.
pub(super) const PATTERN_FIELDS: &[(&[&str], &str, &str)] = &[
    (&["expect", "content_type"], "content_type", "i"),
    (&["expect", "header_regex", "pattern"], "header_regex", "i"),
    (&["expect", "body_regex"], "body_regex", "im"),
    (&["expect", "body_not_regex"], "body_not_regex", "im"),
];
