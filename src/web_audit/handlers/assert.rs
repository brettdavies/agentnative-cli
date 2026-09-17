//! The `expect` block evaluation and the legacy-alias classification, a
//! mirror of the site's `assert.ts`. Every reason string is the site's
//! text, because the summarized evidence line on a scorecard row is built
//! from them.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use url::Url;

use crate::web_audit::fetch::ProbeResponse;
use crate::web_audit::registry::{CHECKS, WebCheck};

/// A header-regex expectation.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct HeaderRegex {
    /// Header name, matched case-insensitively.
    pub name: String,
    /// The JavaScript pattern, compiled case-insensitively.
    pub pattern: String,
}

/// A check's `with.expect` block.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ExpectBlock {
    /// The response status must be one of these.
    pub status: Option<Vec<u16>>,
    /// The response status must be below this bound.
    pub status_below: Option<u16>,
    /// The content-type must match this pattern.
    pub content_type: Option<String>,
    /// This header must be present.
    pub header_present: Option<String>,
    /// This header must match this pattern.
    pub header_regex: Option<HeaderRegex>,
    /// The body must match this pattern.
    pub body_regex: Option<String>,
    /// The body must not match this pattern.
    pub body_not_regex: Option<String>,
    /// At least one markdown href must resolve to a same-origin recovery surface.
    pub same_origin_recovery_link: Option<bool>,
}

impl ExpectBlock {
    /// Whether the block names a status expectation.
    pub fn has_status_expectation(&self) -> bool {
        self.status.is_some() || self.status_below.is_some()
    }
}

/// The evaluation of an `expect` block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertOutcome {
    /// Whether every present key held.
    pub ok: bool,
    /// One line per evaluated key, in evaluation order.
    pub reasons: Vec<String>,
}

/// The registry's patterns, compiled once from their Rust translations.
static COMPILED: LazyLock<HashMap<(&'static str, &'static str), Regex>> = LazyLock::new(|| {
    CHECKS
        .iter()
        .flat_map(|check| {
            check.patterns.iter().map(move |p| {
                let regex = Regex::new(p.rust)
                    .unwrap_or_else(|e| panic!("registry pattern {}.{}: {e}", check.id, p.field));
                ((check.id, p.field), regex)
            })
        })
        .collect()
});

/// The compiled pattern for one of a check's expectation fields.
pub fn compiled(check: &WebCheck, field: &str) -> Option<&'static Regex> {
    CHECKS
        .iter()
        .find(|c| c.id == check.id)
        .and_then(|c| c.patterns.iter().find(|p| p.field == field))
        .and_then(|p| COMPILED.get(&(check.id, p.field)))
}

fn status_text(status: Option<u16>) -> String {
    match status {
        Some(s) => s.to_string(),
        None => "null".to_string(),
    }
}

/// Evaluate an `expect` block against a response. All present keys AND
/// together; the first failing key short-circuits with the reasons
/// accumulated so far.
pub fn assert_http(check: &WebCheck, expect: &ExpectBlock, resp: &ProbeResponse) -> AssertOutcome {
    let mut reasons: Vec<String> = Vec::new();
    if let Some(error) = resp.error.as_deref().filter(|e| !e.is_empty()) {
        return AssertOutcome {
            ok: false,
            reasons: vec![format!("request failed: {error}")],
        };
    }
    let status = resp.status;
    let body = resp.body.as_str();
    let matches =
        |field: &str, text: &str| compiled(check, field).is_some_and(|re| re.is_match(text));

    if let Some(wanted) = &expect.status {
        let ok = status.is_some_and(|s| wanted.contains(&s));
        let list = wanted
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        reasons.push(format!(
            "status {} {} [{list}]",
            status_text(status),
            if ok { "in" } else { "not in" }
        ));
        if !ok {
            return AssertOutcome { ok: false, reasons };
        }
    }
    if let Some(bound) = expect.status_below {
        let ok = status.is_some_and(|s| s < bound);
        reasons.push(format!(
            "status {} {} {bound}",
            status_text(status),
            if ok { "below" } else { "not below" }
        ));
        if !ok {
            return AssertOutcome { ok: false, reasons };
        }
    }
    if let Some(pattern) = &expect.content_type {
        let ct = resp.headers.get("content-type").unwrap_or("");
        let ok = matches("content_type", ct);
        reasons.push(format!(
            "content-type {} {} /{pattern}/",
            serde_json::to_string(ct).unwrap_or_default(),
            if ok { "~" } else { "!~" }
        ));
        if !ok {
            return AssertOutcome { ok: false, reasons };
        }
    }
    if let Some(name) = &expect.header_present {
        let name = name.to_lowercase();
        let ok = resp.headers.contains(&name);
        reasons.push(format!(
            "header {name} {}",
            if ok { "present" } else { "absent" }
        ));
        if !ok {
            return AssertOutcome { ok: false, reasons };
        }
    }
    if let Some(spec) = &expect.header_regex {
        let value = resp.headers.get(&spec.name.to_lowercase()).unwrap_or("");
        let ok = matches("header_regex", value);
        reasons.push(format!(
            "header {} {} /{}/",
            spec.name,
            if ok { "matches" } else { "no match" },
            spec.pattern
        ));
        if !ok {
            return AssertOutcome { ok: false, reasons };
        }
    }
    if let Some(pattern) = &expect.body_regex {
        let ok = matches("body_regex", body);
        reasons.push(format!(
            "body {} /{pattern}/",
            if ok { "matches" } else { "no match" }
        ));
        if !ok {
            return AssertOutcome { ok: false, reasons };
        }
    }
    if let Some(pattern) = &expect.body_not_regex {
        let hit = matches("body_not_regex", body);
        reasons.push(format!(
            "body {} /{pattern}/",
            if hit { "matches forbidden" } else { "avoids" }
        ));
        if hit {
            return AssertOutcome { ok: false, reasons };
        }
    }
    AssertOutcome { ok: true, reasons }
}

/// How a legacy alias probe classifies against the canonical URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AliasVerdict {
    /// A permanent redirect to the canonical.
    Pass,
    /// Content served inline, or a redirect that misses the canonical.
    Broken,
    /// Nothing published at the alias.
    NotApplicable,
}

impl AliasVerdict {
    /// The registry spelling on the evidence row.
    pub fn as_str(self) -> &'static str {
        match self {
            AliasVerdict::Pass => "pass",
            AliasVerdict::Broken => "broken",
            AliasVerdict::NotApplicable => "n_a",
        }
    }
}

/// Classify a non-followed alias probe against the canonical URL. An
/// absent alias is n_a; a 301/308 whose Location resolves to the canonical
/// passes; a 2xx serving content inline is broken; a non-permanent
/// redirect is broken because it signals no canonical intent.
pub fn classify_alias_probe(
    resp: &ProbeResponse,
    alias_url: &str,
    canonical_url: &str,
) -> (AliasVerdict, String) {
    if let Some(error) = &resp.error {
        return (
            AliasVerdict::NotApplicable,
            format!("request failed: {error}"),
        );
    }
    let status = resp.status.unwrap_or(0);
    if status == 301 || status == 308 {
        let Some(location) = resp.headers.get("location").filter(|l| !l.is_empty()) else {
            return (
                AliasVerdict::Broken,
                format!("{status} without a Location header"),
            );
        };
        let Some(resolved) = Url::parse(alias_url)
            .ok()
            .and_then(|alias| alias.join(location).ok())
        else {
            return (
                AliasVerdict::Broken,
                format!("{status} to unparseable target {location}"),
            );
        };
        let Ok(canonical) = Url::parse(canonical_url) else {
            return (
                AliasVerdict::Broken,
                format!("{status} to unparseable target {location}"),
            );
        };
        if resolved.origin() == canonical.origin() && resolved.path() == canonical.path() {
            return (
                AliasVerdict::Pass,
                format!("{status} -> {}", canonical.path()),
            );
        }
        return (
            AliasVerdict::Broken,
            format!("{status} away from the canonical ({})", resolved.path()),
        );
    }
    if matches!(status, 302 | 303 | 307) {
        return (
            AliasVerdict::Broken,
            format!("{status} non-permanent redirect (301/308 expected)"),
        );
    }
    if (200..300).contains(&status) {
        return (
            AliasVerdict::Broken,
            format!("{status} serves content inline (ambiguous duplicate)"),
        );
    }
    (
        AliasVerdict::NotApplicable,
        format!("{status} alias not published"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::headers::Headers;
    use crate::web_audit::registry::check_by_id;

    fn resp(status: Option<u16>, headers: &[(&str, &str)], body: &str) -> ProbeResponse {
        ProbeResponse {
            status,
            headers: Headers::from_pairs(headers.iter().copied()),
            body: body.to_string(),
            error: None,
            elapsed_ms: 0,
            truncated: false,
        }
    }

    fn expect_of(id: &str) -> (&'static WebCheck, ExpectBlock) {
        let check = check_by_id(id).unwrap();
        let with: serde_json::Value = serde_json::from_str(check.with_json).unwrap();
        let expect = serde_json::from_value(with["expect"].clone()).unwrap();
        (check, expect)
    }

    #[test]
    fn reasons_accumulate_in_order_and_short_circuit_on_the_first_miss() {
        let (check, expect) = expect_of("llms-txt");
        let ok = assert_http(
            check,
            &expect,
            &resp(Some(200), &[], "# Site\n- [x](https://e/x)"),
        );
        assert!(ok.ok);
        assert_eq!(ok.reasons[0], "status 200 in [200]");
        assert!(ok.reasons[1].starts_with("body matches /"));
        let miss = assert_http(check, &expect, &resp(Some(404), &[], ""));
        assert_eq!(
            miss,
            AssertOutcome {
                ok: false,
                reasons: vec!["status 404 not in [200]".to_string()]
            }
        );
        let dead = assert_http(
            check,
            &expect,
            &ProbeResponse::failed("TypeError: x".into(), 0),
        );
        assert_eq!(dead.reasons, vec!["request failed: TypeError: x"]);
        assert_eq!(
            assert_http(check, &expect, &resp(None, &[], "")).reasons,
            vec!["status null not in [200]"]
        );
    }

    #[test]
    fn content_type_and_header_lines_quote_the_site_way() {
        let (check, expect) = expect_of("accept-markdown");
        let out = assert_http(
            check,
            &expect,
            &resp(Some(200), &[("content-type", "text/\"plain\"")], ""),
        );
        assert_eq!(
            out.reasons,
            vec![r#"content-type "text/\"plain\"" !~ /markdown|text/plain/"#]
        );
        let (check, expect) = expect_of("markdown-vary");
        let out = assert_http(
            check,
            &expect,
            &resp(Some(200), &[("vary", "Accept-Encoding, User-Agent")], ""),
        );
        assert!(!out.ok);
        assert!(
            out.reasons
                .last()
                .unwrap()
                .starts_with("header vary no match /"),
            "{:?}",
            out.reasons
        );
    }

    #[test]
    fn alias_probes_classify_by_status_and_location() {
        let alias = "https://example.com/.well-known/mcp/server-card.json";
        let canonical = "https://example.com/.well-known/mcp.json";
        let hop = |status: u16, location: &str| {
            classify_alias_probe(
                &resp(Some(status), &[("location", location)], ""),
                alias,
                canonical,
            )
        };
        assert_eq!(
            hop(301, "/.well-known/mcp.json"),
            (
                AliasVerdict::Pass,
                "301 -> /.well-known/mcp.json".to_string()
            )
        );
        assert_eq!(
            hop(308, "https://other.test/.well-known/mcp.json").0,
            AliasVerdict::Broken
        );
        assert_eq!(
            hop(301, "/elsewhere").1,
            "301 away from the canonical (/elsewhere)"
        );
        assert_eq!(
            classify_alias_probe(&resp(Some(301), &[], ""), alias, canonical).1,
            "301 without a Location header"
        );
        assert_eq!(
            hop(302, "/.well-known/mcp.json").1,
            "302 non-permanent redirect (301/308 expected)"
        );
        assert_eq!(
            classify_alias_probe(&resp(Some(200), &[], "{}"), alias, canonical).1,
            "200 serves content inline (ambiguous duplicate)"
        );
        assert_eq!(
            classify_alias_probe(&resp(Some(404), &[], ""), alias, canonical),
            (
                AliasVerdict::NotApplicable,
                "404 alias not published".to_string()
            )
        );
        assert_eq!(
            classify_alias_probe(
                &ProbeResponse::failed("TypeError: x".into(), 0),
                alias,
                canonical
            )
            .1,
            "request failed: TypeError: x"
        );
    }
}
