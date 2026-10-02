//! Web-audit registry codegen used by `build.rs` to generate
//! `$OUT_DIR/generated_web_registry.rs` from the vendored site inputs under
//! `src/web_audit/vendored/`.
//!
//! Mirrors `agentnative-site:src/build/13-web-audit-registry.mjs`: the same
//! structural validation (every failure names the check id), `keyword`
//! derived from `tier`, `{ua:...}` expansion against the vendored token map,
//! and the remediation catalog held 1:1 against the check set. Two
//! deliberate differences: every regex-bearing value is translated and
//! compiled with the `regex` crate at build time (`js_regex`), and a
//! well-formed entry naming a handler kind or eval rule outside the ported
//! set binds to `Unsupported` instead of failing, so the engine reports it
//! as a skip at runtime rather than holding the vendor pin hostage.
//!
//! Emission order is semantic for `CHECKS` (scorecard rows follow registry
//! order) and sorted everywhere else (remediation, token map).

use serde_yaml::{Mapping, Value as Yaml};

use super::js_regex;
use super::ts_consts::SiteConstants;

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
];
/// Eval rules the Rust engine ports.
pub const EVAL_RULES: &[&str] = &["legacy-alias-redirects", "scoped-discovery"];
const SITE_TYPES: &[&str] = &["content", "api", "mcp", "all"];
const ANTECEDENTS: &[&str] = &[
    "none",
    "http-root",
    "html-root",
    "mcp-present",
    "mcp-auth",
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
const TIERS: &[(&str, &str)] = &[
    ("required", "must"),
    ("recommended", "should"),
    ("optional", "may"),
];
const CORS_SURFACES: &[&str] = &["preflight", "actual"];
const BREADCRUMB_MAX: usize = 40;
/// Regex-bearing `with` values: path inside `with`, short field name, and the
/// JavaScript flags `assert.ts` evaluates the value under.
const PATTERN_FIELDS: &[(&[&str], &str, &str)] = &[
    (&["expect", "content_type"], "content_type", "i"),
    (&["expect", "header_regex", "pattern"], "header_regex", "i"),
    (&["expect", "body_regex"], "body_regex", "im"),
    (&["expect", "body_not_regex"], "body_not_regex", "im"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpDiscovery {
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

// ---------------------------------------------------------------------------
// Registry normalization
// ---------------------------------------------------------------------------

/// Validate and normalize the registry YAML text. `ua_tokens` is the vendored
/// `{ua:...}` map from `ts_consts`.
pub fn normalize_registry(
    yaml: &str,
    ua_tokens: &[(String, String)],
) -> Result<NormalizedRegistry, String> {
    let doc: Yaml = serde_yaml::from_str(yaml)
        .map_err(|e| format!("web-audit registry: YAML parse error: {e}"))?;
    let doc = doc.as_mapping().ok_or_else(|| {
        "web-audit registry: expected a YAML mapping at the top level".to_string()
    })?;

    let mcp_discovery = normalize_discovery(doc)?;
    let categories_map = doc
        .get("categories")
        .and_then(Yaml::as_mapping)
        .ok_or_else(|| {
            "web-audit registry: expected a top-level \"categories\" mapping".to_string()
        })?;
    let category_order: Vec<String> = doc
        .get("category_order")
        .and_then(Yaml::as_sequence)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "web-audit registry: expected a top-level \"category_order\" array".to_string()
        })?
        .iter()
        .map(|v| {
            v.as_str().map(str::to_string).ok_or_else(|| {
                "web-audit registry: category_order entries must be strings".to_string()
            })
        })
        .collect::<Result<_, _>>()?;
    let category_keys: Vec<String> = categories_map
        .iter()
        .map(|(k, _)| {
            k.as_str()
                .map(str::to_string)
                .ok_or_else(|| "web-audit registry: categories keys must be strings".to_string())
        })
        .collect::<Result<_, _>>()?;
    if category_order.len() != category_keys.len()
        || !category_order
            .iter()
            .all(|slug| category_keys.contains(slug))
    {
        return Err(
            "web-audit registry: category_order must list every categories key exactly once"
                .to_string(),
        );
    }
    let categories: Vec<(String, String)> = category_order
        .iter()
        .map(|slug| {
            let name = categories_map
                .get(Yaml::String(slug.clone()))
                .and_then(Yaml::as_str)
                .ok_or_else(|| {
                    format!("web-audit registry: category {slug:?} needs a string display name")
                })?;
            Ok((slug.clone(), name.to_string()))
        })
        .collect::<Result<_, String>>()?;

    let checks = doc
        .get("checks")
        .and_then(Yaml::as_sequence)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "web-audit registry: expected a non-empty top-level \"checks\" array".to_string()
        })?;
    let mut seen: Vec<String> = Vec::new();
    let mut normalized = Vec::with_capacity(checks.len());
    for check in checks {
        let check = normalize_check(check, &category_keys, ua_tokens)?;
        if seen.contains(&check.id) {
            return Err(format!(
                "web-audit registry: duplicate check id \"{}\"",
                check.id
            ));
        }
        seen.push(check.id.clone());
        normalized.push(check);
    }

    let version = match doc.get("version") {
        None => 1,
        Some(v) => v.as_u64().ok_or_else(|| {
            "web-audit registry: version must be a non-negative integer".to_string()
        })?,
    };
    Ok(NormalizedRegistry {
        version,
        mcp_discovery,
        category_order,
        categories,
        checks: normalized,
    })
}

fn normalize_discovery(doc: &Mapping) -> Result<McpDiscovery, String> {
    let err = || {
        "web-audit registry: mcp_discovery must carry well_known[], common_paths[], protocol_version".to_string()
    };
    let discovery = doc
        .get("mcp_discovery")
        .and_then(Yaml::as_mapping)
        .ok_or_else(err)?;
    let strings = |key: &str| -> Result<Vec<String>, String> {
        discovery
            .get(key)
            .and_then(Yaml::as_sequence)
            .ok_or_else(err)?
            .iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(err))
            .collect()
    };
    Ok(McpDiscovery {
        well_known: strings("well_known")?,
        common_paths: strings("common_paths")?,
        protocol_version: discovery
            .get("protocol_version")
            .and_then(Yaml::as_str)
            .ok_or_else(err)?
            .to_string(),
    })
}

fn check_err(id: &str, what: impl std::fmt::Display) -> String {
    format!("web-audit registry: check \"{id}\" {what}")
}

fn required_str(check: &Mapping, id: &str, key: &str) -> Result<String, String> {
    match check.get(key) {
        Some(Yaml::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(check_err(id, format!("missing {key}"))),
    }
}

fn normalize_check(
    raw: &Yaml,
    category_keys: &[String],
    ua_tokens: &[(String, String)],
) -> Result<NormalizedCheck, String> {
    let check = raw
        .as_mapping()
        .ok_or_else(|| "web-audit registry: every checks entry must be a mapping".to_string())?;
    let id = match check.get("id").and_then(Yaml::as_str) {
        Some(id) if is_check_id(id) => id.to_string(),
        other => {
            return Err(format!(
                "web-audit registry: check id {} must match /^[a-z0-9][a-z0-9-]*$/",
                other
                    .map(|s| format!("\"{s}\""))
                    .unwrap_or_else(|| "null".to_string())
            ));
        }
    };

    let category = check
        .get("category")
        .and_then(Yaml::as_str)
        .unwrap_or_default()
        .to_string();
    if !category_keys.contains(&category) {
        return Err(check_err(
            &id,
            format!("names unknown category \"{category}\""),
        ));
    }
    let tier = check
        .get("tier")
        .and_then(Yaml::as_str)
        .unwrap_or_default()
        .to_string();
    let keyword = TIERS
        .iter()
        .find(|(t, _)| *t == tier)
        .map(|(_, k)| k.to_string())
        .ok_or_else(|| {
            check_err(
                &id,
                format!("has invalid tier \"{tier}\" (want required|recommended|optional)"),
            )
        })?;
    if check.contains_key("keyword") {
        return Err(check_err(
            &id,
            "hand-authors a keyword field: keyword is derived from tier at build time",
        ));
    }
    let principle = check
        .get("principle")
        .and_then(Yaml::as_str)
        .filter(|p| is_principle(p))
        .ok_or_else(|| {
            check_err(
                &id,
                format!(
                    "needs a principle in P1..P8 (got {})",
                    yaml_display(check.get("principle"))
                ),
            )
        })?
        .to_string();
    if check.contains_key("applies_to") {
        return Err(check_err(
            &id,
            "carries the retired applies_to field: use site_types + antecedent",
        ));
    }
    let site_types: Vec<String> = check
        .get("site_types")
        .and_then(Yaml::as_sequence)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| check_err(&id, "needs a non-empty site_types array"))?
        .iter()
        .map(|v| {
            let st = v.as_str().unwrap_or_default();
            if SITE_TYPES.contains(&st) {
                Ok(st.to_string())
            } else {
                Err(check_err(
                    &id,
                    format!("has invalid site_types entry \"{st}\""),
                ))
            }
        })
        .collect::<Result<_, _>>()?;
    let antecedent = check
        .get("antecedent")
        .and_then(Yaml::as_str)
        .unwrap_or_default()
        .to_string();
    if !ANTECEDENTS.contains(&antecedent.as_str()) {
        return Err(check_err(
            &id,
            format!("has unknown antecedent \"{antecedent}\""),
        ));
    }
    let eval = match check.get("eval") {
        None => None,
        Some(Yaml::String(e)) => Some(e.clone()),
        Some(other) => {
            return Err(check_err(
                &id,
                format!("eval must be a string (got {})", yaml_display(Some(other))),
            ));
        }
    };
    let eval_supported = eval.as_deref().is_none_or(|e| EVAL_RULES.contains(&e));
    let weight = check
        .get("weight")
        .and_then(Yaml::as_u64)
        .filter(|w| *w > 0)
        .ok_or_else(|| {
            check_err(
                &id,
                format!(
                    "needs a positive integer weight (got {})",
                    yaml_display(check.get("weight"))
                ),
            )
        })?;
    let title = required_str(check, &id, "title")?;
    let hint = required_str(check, &id, "hint")?;
    let breadcrumb = required_str(check, &id, "breadcrumb")?;
    if breadcrumb.chars().count() > BREADCRUMB_MAX {
        return Err(check_err(
            &id,
            format!(
                "breadcrumb is {} chars, over the {BREADCRUMB_MAX} a trail fits",
                breadcrumb.chars().count()
            ),
        ));
    }
    let handler = check
        .get("handler")
        .and_then(Yaml::as_str)
        .ok_or_else(|| check_err(&id, "missing handler"))?
        .to_string();
    let handler_supported = HANDLER_KINDS.contains(&handler.as_str());
    let with = check
        .get("with")
        .and_then(Yaml::as_mapping)
        .ok_or_else(|| check_err(&id, "missing \"with\" handler parameters"))?;
    if handler == "cors-preflight" {
        let surface = with.get("surface").and_then(Yaml::as_str);
        if !surface.is_some_and(|s| CORS_SURFACES.contains(&s)) {
            return Err(check_err(
                &id,
                format!(
                    "needs with.surface \"preflight\" or \"actual\" (got {})",
                    yaml_display(with.get("surface"))
                ),
            ));
        }
    }
    let with = expand_probe_user_agent(&id, with, ua_tokens)?;
    let patterns = collect_patterns(&id, &with)?;
    let with_json = serde_json::to_string(&yaml_to_json(&id, &Yaml::Mapping(with))?)
        .map_err(|e| check_err(&id, format!("with cannot serialize as JSON: {e}")))?;

    Ok(NormalizedCheck {
        id,
        category,
        tier,
        keyword,
        principle,
        site_types,
        antecedent,
        eval,
        eval_supported,
        weight,
        title,
        breadcrumb,
        hint,
        handler,
        handler_supported,
        with_json,
        patterns,
    })
}

fn is_check_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_principle(p: &str) -> bool {
    let mut chars = p.chars();
    chars.next() == Some('P')
        && chars.next().is_some_and(|d| ('1'..='8').contains(&d))
        && chars.next().is_none()
}

fn yaml_display(v: Option<&Yaml>) -> String {
    match v {
        None => "null".to_string(),
        Some(v) => serde_json::to_string(&yaml_to_json("", v).unwrap_or(serde_json::Value::Null))
            .unwrap_or_else(|_| "null".to_string()),
    }
}

/// Expand a `{ua:...}` token in `with.headers` (any casing of `user-agent`)
/// against the vendored map; a literal User-Agent value is a build error.
fn expand_probe_user_agent(
    id: &str,
    with: &Mapping,
    ua_tokens: &[(String, String)],
) -> Result<Mapping, String> {
    let Some(headers) = with.get("headers").and_then(Yaml::as_mapping) else {
        return Ok(with.clone());
    };
    let Some((ua_key, ua_value)) = headers.iter().find(|(k, _)| {
        k.as_str()
            .is_some_and(|k| k.eq_ignore_ascii_case("user-agent"))
    }) else {
        return Ok(with.clone());
    };
    let value = ua_value.as_str().unwrap_or_default();
    let known: Vec<&str> = ua_tokens.iter().map(|(k, _)| k.as_str()).collect();
    let expanded = ua_tokens
        .iter()
        .find(|(k, _)| k == value)
        .map(|(_, v)| v.clone())
        .ok_or_else(|| {
            check_err(
                id,
                format!(
                    "User-Agent {value:?} must be a {{ua:...}} token from src/shared/user-agents.ts (known: {})",
                    known.join(", ")
                ),
            )
        })?;
    let mut headers = headers.clone();
    headers.insert(ua_key.clone(), Yaml::String(expanded));
    let mut out = with.clone();
    out.insert(Yaml::String("headers".to_string()), Yaml::Mapping(headers));
    Ok(out)
}

/// Translate and compile every regex-bearing `with` value.
fn collect_patterns(id: &str, with: &Mapping) -> Result<Vec<PatternSpec>, String> {
    let mut out = Vec::new();
    for (path, field, flags) in PATTERN_FIELDS {
        let mut cursor: Option<&Yaml> = None;
        for (i, key) in path.iter().enumerate() {
            let map = if i == 0 {
                Some(with)
            } else {
                cursor.and_then(Yaml::as_mapping)
            };
            cursor = map.and_then(|m| m.get(*key));
            if cursor.is_none() {
                break;
            }
        }
        let Some(value) = cursor else { continue };
        let source = value
            .as_str()
            .ok_or_else(|| check_err(id, format!("{field} must be a string pattern")))?;
        let rust = js_regex::translate(source, flags)
            .map_err(|e| check_err(id, format!("{field} {source:?}: {e}")))?;
        regex::Regex::new(&rust).map_err(|e| {
            check_err(
                id,
                format!("{field} {source:?} does not compile as a Rust regex: {e}"),
            )
        })?;
        out.push(PatternSpec {
            field: field.to_string(),
            source: source.to_string(),
            flags: flags.to_string(),
            rust,
        });
    }
    Ok(out)
}

fn yaml_to_json(id: &str, v: &Yaml) -> Result<serde_json::Value, String> {
    use serde_json::Value as Json;
    Ok(match v {
        Yaml::Null => Json::Null,
        Yaml::Bool(b) => Json::Bool(*b),
        Yaml::Number(n) => {
            if let Some(i) = n.as_i64() {
                Json::from(i)
            } else if let Some(u) = n.as_u64() {
                Json::from(u)
            } else {
                Json::from(n.as_f64().unwrap_or_default())
            }
        }
        Yaml::String(s) => Json::String(s.clone()),
        Yaml::Sequence(seq) => Json::Array(
            seq.iter()
                .map(|v| yaml_to_json(id, v))
                .collect::<Result<_, _>>()?,
        ),
        Yaml::Mapping(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                let key = k
                    .as_str()
                    .ok_or_else(|| check_err(id, "with has a non-string key"))?;
                out.insert(key.to_string(), yaml_to_json(id, v)?);
            }
            Json::Object(out)
        }
        Yaml::Tagged(_) => {
            return Err(check_err(
                id,
                "with carries a YAML tag, which has no JSON form",
            ));
        }
    })
}

// ---------------------------------------------------------------------------
// Remediation normalization
// ---------------------------------------------------------------------------

/// Validate the remediation catalog against the check ids and return the
/// entries sorted by id. Mirrors `normalizeWebRemediation`.
pub fn normalize_remediation(
    yaml: &str,
    check_ids: &[&str],
) -> Result<Vec<RemediationEntry>, String> {
    let doc: Yaml = serde_yaml::from_str(yaml)
        .map_err(|e| format!("web-audit remediation.yaml: YAML parse error: {e}"))?;
    let remediation = doc
        .as_mapping()
        .and_then(|d| d.get("remediation"))
        .and_then(Yaml::as_mapping)
        .ok_or_else(|| {
            "web-audit remediation.yaml: expected a top-level \"remediation\" mapping".to_string()
        })?;
    let mut out = Vec::with_capacity(remediation.len());
    for (key, entry) in remediation {
        let id = key
            .as_str()
            .ok_or_else(|| "web-audit remediation: entry keys must be strings".to_string())?;
        let entry = entry.as_mapping().ok_or_else(|| {
            format!(
                "web-audit remediation: entry \"{id}\" needs string title, goal, and fix fields"
            )
        })?;
        let field = |name: &str| -> Result<String, String> {
            entry
                .get(name)
                .and_then(Yaml::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    format!("web-audit remediation: entry \"{id}\" needs string title, goal, and fix fields")
                })
        };
        let title = field("title")?;
        let goal = field("goal")?;
        let fix = field("fix")?;
        if entry.contains_key("body") || entry.contains_key("evidence_template") {
            return Err(format!(
                "web-audit remediation: entry \"{id}\" carries a retired body/evidence_template field: use goal/fix/resources"
            ));
        }
        if fix.contains("{{evidence}}") {
            return Err(format!(
                "web-audit remediation: entry \"{id}\" carries an evidence slot: evidence is assembled at audit time"
            ));
        }
        let resources = match entry.get("resources") {
            None | Some(Yaml::Null) => Vec::new(),
            Some(Yaml::Sequence(seq)) => seq
                .iter()
                .map(|r| {
                    let r = r.as_mapping();
                    let label = r.and_then(|r| r.get("label")).and_then(Yaml::as_str);
                    let url = r.and_then(|r| r.get("url")).and_then(Yaml::as_str);
                    match (label, url) {
                        (Some(label), Some(url)) if url.starts_with("http://") || url.starts_with("https://") => {
                            Ok(RemediationResource { label: label.to_string(), url: url.to_string() })
                        }
                        _ => Err(format!(
                            "web-audit remediation: entry \"{id}\" resource needs a label and an absolute url"
                        )),
                    }
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(format!("web-audit remediation: entry \"{id}\" resources must be an array")),
        };
        out.push(RemediationEntry {
            id: id.to_string(),
            title,
            goal,
            fix,
            resources,
        });
    }
    for id in check_ids {
        if !out.iter().any(|r| r.id == *id) {
            return Err(format!(
                "web-audit remediation: check \"{id}\" has no remediation entry"
            ));
        }
    }
    for entry in &out {
        if !check_ids.contains(&entry.id.as_str()) {
            return Err(format!(
                "web-audit remediation: orphan remediation \"{}\" matches no check",
                entry.id
            ));
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

// ---------------------------------------------------------------------------
// Emission
// ---------------------------------------------------------------------------

fn pascal(kebab: &str) -> String {
    let mut out = String::with_capacity(kebab.len());
    for word in kebab.split('-') {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

// The plain enums carry serde derives so the scorecard serializes them under
// their registry spelling; the binding enums carry a payload and never reach
// the wire.
fn emit_enum(src: &mut String, doc: &str, name: &str, variants: &[&str], unsupported: bool) {
    src.push_str(&format!("/// {doc}\n"));
    if unsupported {
        src.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n");
    } else {
        src.push_str(
            "#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]\n",
        );
        src.push_str("#[serde(rename_all = \"kebab-case\")]\n");
    }
    src.push_str(&format!("pub enum {name} {{\n"));
    for v in variants {
        src.push_str(&format!("    /// `{v}`\n    {},\n", pascal(v)));
    }
    if unsupported {
        src.push_str(
            "    /// A kind the vendored registry names that this engine has not ported.\n",
        );
        src.push_str("    Unsupported(&'static str),\n");
    }
    src.push_str("}\n\n");
    src.push_str(&format!("impl {name} {{\n"));
    src.push_str("    /// The registry spelling.\n");
    src.push_str("    pub fn as_str(self) -> &'static str {\n        match self {\n");
    for v in variants {
        src.push_str(&format!("            {name}::{} => {v:?},\n", pascal(v)));
    }
    if unsupported {
        src.push_str(&format!("            {name}::Unsupported(name) => name,\n"));
    }
    src.push_str("        }\n    }\n}\n\n");
}

fn str_slice(items: &[String]) -> String {
    let parts: Vec<String> = items.iter().map(|s| format!("{s:?}")).collect();
    format!("&[{}]", parts.join(", "))
}

/// Render the generated module. Every table is a `const`; `CHECKS` keeps
/// registry order and `REMEDIATION` is sorted by id.
pub fn emit_rust(
    reg: &NormalizedRegistry,
    remediation: &[RemediationEntry],
    consts: &SiteConstants,
    site_sha: &str,
) -> String {
    let tiers: Vec<&str> = TIERS.iter().map(|(t, _)| *t).collect();
    let keywords: Vec<&str> = TIERS.iter().map(|(_, k)| *k).collect();
    let mut src = String::new();
    src.push_str("// @generated by build.rs from src/web_audit/vendored/. Do not edit by hand.\n");
    src.push_str("// Run `scripts/sync-web-audit.sh` to refresh the inputs; `cargo build` regenerates this file.\n\n");
    src.push_str("/// The registry's `version` field.\n");
    src.push_str(&format!(
        "pub const REGISTRY_VERSION: u64 = {};\n",
        reg.version
    ));
    src.push_str("/// The agentnative-site commit the vendored inputs were taken from.\n");
    src.push_str(&format!("pub const SITE_SHA: &str = {site_sha:?};\n"));
    src.push_str(
        "/// The identifying User-Agent sent on every probe that sets no User-Agent of its own.\n",
    );
    src.push_str(&format!(
        "pub const AUDIT_USER_AGENT: &str = {:?};\n",
        consts.audit_user_agent
    ));
    src.push_str("/// Registry `{ua:...}` token to the literal User-Agent it expands to.\n");
    src.push_str("pub const PROBE_UA_TOKENS: &[(&str, &str)] = &[\n");
    let mut tokens = consts.probe_ua_tokens.clone();
    tokens.sort();
    for (k, v) in &tokens {
        src.push_str(&format!("    ({k:?}, {v:?}),\n"));
    }
    src.push_str("];\n");
    src.push_str("/// MCP endpoint discovery configuration.\n");
    src.push_str(&format!(
        "pub const MCP_DISCOVERY: McpDiscovery = McpDiscovery {{ well_known: {}, common_paths: {}, protocol_version: {:?} }};\n",
        str_slice(&reg.mcp_discovery.well_known),
        str_slice(&reg.mcp_discovery.common_paths),
        reg.mcp_discovery.protocol_version
    ));
    src.push_str("/// Display order of the categories.\n");
    src.push_str(&format!(
        "pub const CATEGORY_ORDER: &[&str] = {};\n",
        str_slice(&reg.category_order)
    ));
    src.push_str("/// `(slug, display name)` in `CATEGORY_ORDER` order.\n");
    src.push_str("pub const CATEGORIES: &[(&str, &str)] = &[\n");
    for (slug, name) in &reg.categories {
        src.push_str(&format!("    ({slug:?}, {name:?}),\n"));
    }
    src.push_str("];\n");
    src.push_str("/// Handler kinds this engine ports.\n");
    src.push_str(&format!(
        "pub const HANDLER_KINDS: &[&str] = {};\n",
        str_slice(
            &HANDLER_KINDS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        )
    ));
    src.push_str("/// Eval rules this engine ports.\n");
    src.push_str(&format!(
        "pub const EVAL_RULES: &[&str] = {};\n\n",
        str_slice(&EVAL_RULES.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    ));

    emit_enum(&mut src, "Registry `tier`.", "WebCheckTier", &tiers, false);
    emit_enum(
        &mut src,
        "Requirement keyword derived from `tier`.",
        "WebCheckKeyword",
        &keywords,
        false,
    );
    emit_enum(
        &mut src,
        "Per-check declared-site-type filter value.",
        "WebCheckSiteType",
        SITE_TYPES,
        false,
    );
    emit_enum(
        &mut src,
        "Runtime gate that flips a check to n_a when unmet.",
        "AntecedentToken",
        ANTECEDENTS,
        false,
    );
    emit_enum(
        &mut src,
        "Probe handler a check dispatches to.",
        "HandlerBinding",
        HANDLER_KINDS,
        true,
    );
    emit_enum(
        &mut src,
        "Non-standard evaluation rule a check carries.",
        "EvalBinding",
        EVAL_RULES,
        true,
    );

    src.push_str("/// Every check, in registry order.\n");
    src.push_str("pub const CHECKS: &[WebCheck] = &[\n");
    for c in &reg.checks {
        let site_types: Vec<String> = c
            .site_types
            .iter()
            .map(|s| format!("WebCheckSiteType::{}", pascal(s)))
            .collect();
        let eval = match &c.eval {
            None => "None".to_string(),
            Some(e) if c.eval_supported => format!("Some(EvalBinding::{})", pascal(e)),
            Some(e) => format!("Some(EvalBinding::Unsupported({e:?}))"),
        };
        let handler = if c.handler_supported {
            format!("HandlerBinding::{}", pascal(&c.handler))
        } else {
            format!("HandlerBinding::Unsupported({:?})", c.handler)
        };
        src.push_str("    WebCheck {\n");
        src.push_str(&format!("        id: {:?},\n", c.id));
        src.push_str(&format!("        category: {:?},\n", c.category));
        src.push_str(&format!(
            "        tier: WebCheckTier::{},\n",
            pascal(&c.tier)
        ));
        src.push_str(&format!(
            "        keyword: WebCheckKeyword::{},\n",
            pascal(&c.keyword)
        ));
        src.push_str(&format!("        principle: {:?},\n", c.principle));
        src.push_str(&format!(
            "        site_types: &[{}],\n",
            site_types.join(", ")
        ));
        src.push_str(&format!(
            "        antecedent: AntecedentToken::{},\n",
            pascal(&c.antecedent)
        ));
        src.push_str(&format!("        eval: {eval},\n"));
        src.push_str(&format!("        weight: {},\n", c.weight));
        src.push_str(&format!("        title: {:?},\n", c.title));
        src.push_str(&format!("        breadcrumb: {:?},\n", c.breadcrumb));
        src.push_str(&format!("        hint: {:?},\n", c.hint));
        src.push_str(&format!("        handler: {handler},\n"));
        src.push_str(&format!("        with_json: {:?},\n", c.with_json));
        src.push_str("        patterns: &[\n");
        for p in &c.patterns {
            src.push_str(&format!(
                "            RegexPattern {{ field: {:?}, source: {:?}, flags: {:?}, rust: {:?} }},\n",
                p.field, p.source, p.flags, p.rust
            ));
        }
        src.push_str("        ],\n    },\n");
    }
    src.push_str("];\n\n");

    src.push_str("/// Fix catalog, one entry per check, sorted by id.\n");
    src.push_str("pub const REMEDIATION: &[WebRemediation] = &[\n");
    for r in remediation {
        src.push_str("    WebRemediation {\n");
        src.push_str(&format!("        id: {:?},\n", r.id));
        src.push_str(&format!("        title: {:?},\n", r.title));
        src.push_str(&format!("        goal: {:?},\n", r.goal));
        src.push_str(&format!("        fix: {:?},\n", r.fix));
        src.push_str("        resources: &[\n");
        for res in &r.resources {
            src.push_str(&format!(
                "            RemediationResource {{ label: {:?}, url: {:?} }},\n",
                res.label, res.url
            ));
        }
        src.push_str("        ],\n    },\n");
    }
    src.push_str("];\n");
    src
}
