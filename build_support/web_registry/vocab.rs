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
/// Regex-bearing `with` values: path inside `with`, short field name, and the
/// JavaScript flags `assert.ts` evaluates the value under.
pub(super) const PATTERN_FIELDS: &[(&[&str], &str, &str)] = &[
    (&["expect", "content_type"], "content_type", "i"),
    (&["expect", "header_regex", "pattern"], "header_regex", "i"),
    (&["expect", "body_regex"], "body_regex", "im"),
    (&["expect", "body_not_regex"], "body_not_regex", "im"),
];
