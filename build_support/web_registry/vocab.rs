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
    "protected-resource",
    "server-card",
];
/// Eval rules the Rust engine ports.
pub const EVAL_RULES: &[&str] = &[
    "api-description",
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
pub(super) const LANE_CATEGORY: &str = "mcp";
pub(super) const CORS_SURFACES: &[&str] = &["preflight", "actual"];
pub(super) const PROTECTED_RESOURCE_OPS: &[&str] = &["challenge", "metadata"];
/// Documents discovery keeps for a `retained-document` check.
pub(super) const RETAINED_DOCUMENT_KEYS: &[&str] = &["ai-catalog", "api-catalog", "server-card"];
/// Handlers that score a retained document instead of issuing a request.
pub(super) const RETAINED_DOCUMENT_HANDLERS: &[&str] = &["http", "server-card"];
/// The field rules a server-card check reads; with none it would pass every card.
pub(super) const CARD_FIELD_PARAMS: &[&str] = &["required", "remote_required"];
/// The JSON Schema type names `has_type` in the server-card handler knows. Any other
/// reads as a mismatch on every card.
pub(super) const JSON_TYPES: &[&str] = &[
    "string", "number", "integer", "boolean", "object", "array", "null",
];
/// Any other op runs the JSON-errors probe under the check's own id.
pub(super) const API_HYGIENE_OPS: &[&str] = &["json-errors", "rate-limit"];
/// Any other op runs the link crawl under the check's own id.
pub(super) const LLMS_TXT_OPS: &[&str] = &["format", "links", "when-to-use"];
/// Regex-bearing `with` values: path inside `with`, short field name, and the
/// JavaScript flags `assert.ts` evaluates the value under.
pub(super) const PATTERN_FIELDS: &[(&[&str], &str, &str)] = &[
    (&["expect", "content_type"], "content_type", "i"),
    (&["expect", "header_regex", "pattern"], "header_regex", "i"),
    (&["expect", "body_regex"], "body_regex", "im"),
    (&["expect", "body_not_regex"], "body_not_regex", "im"),
];
