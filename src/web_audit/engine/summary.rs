//! The compact evidence line each result row carries, a mirror of the
//! site's `summarizeEvidence`. The strings are part of the byte-parity
//! contract, so every branch reproduces the site's formatting, including
//! how JavaScript prints a missing or null field inside a template.

use serde_json::Value;

use super::types::{ProbeOutcome, ProbeStatus};
use crate::web_audit::registry::{EvalBinding, HandlerBinding, WebCheck};
use crate::web_audit::scorecard::EvidenceItem;

/// How a template literal prints a value: `undefined` for a missing key,
/// `null` for JSON null, the bare text for a string, JavaScript's number
/// form, `true`/`false`, elements joined by commas for an array, and
/// `[object Object]` for an object.
pub fn js_str(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::Null => String::new(),
                other => js_str(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_string(),
    }
}

/// JavaScript truthiness for the `if (x)` checks the site makes.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `a ?? b`: the value unless it is missing or null.
fn coalesce<'a>(value: Option<&'a Value>, fallback: &'a str) -> String {
    match value {
        None | Some(Value::Null) => fallback.to_string(),
        Some(v) => js_str(Some(v)),
    }
}

fn why_lines(item: &EvidenceItem) -> Option<Vec<String>> {
    item.get("why")?
        .as_array()
        .map(|items| items.iter().map(|v| js_str(Some(v))).collect())
}

fn joined_why(item: &EvidenceItem, separator: &str) -> Option<String> {
    why_lines(item).map(|lines| lines.join(separator))
}

/// The one-line evidence summary for a finalized outcome.
pub fn summarize_evidence(check: &WebCheck, outcome: &ProbeOutcome) -> String {
    let empty = EvidenceItem::new();
    let first = outcome.evidence.first().unwrap_or(&empty);
    if outcome.status == ProbeStatus::Na {
        return joined_why(first, "; ").unwrap_or_else(|| "not applicable".to_string());
    }

    if check.handler == HandlerBinding::Mcp {
        if truthy(first.get("error")) {
            return format!(
                "{}: {}",
                js_str(first.get("url")),
                js_str(first.get("error"))
            );
        }
        if matches!(
            outcome.status,
            ProbeStatus::Absent | ProbeStatus::Noncompliant
        ) && let Some(why) = joined_why(first, "; ")
        {
            return why;
        }
        let op = with_op(check);
        if op.as_deref() == Some("initialize") {
            return match server_info_name(first) {
                Some(name) => format!(
                    "serverInfo {name}, protocol {}",
                    js_str(first.get("protocolVersion"))
                ),
                None => "no serverInfo in initialize result".to_string(),
            };
        }
        if op.as_deref() == Some("server-discover") && first.contains_key("supported_versions") {
            return match first.get("supported_versions").and_then(Value::as_array) {
                Some(versions) => format!(
                    "supports {}, serverInfo {}",
                    versions
                        .iter()
                        .map(|v| js_str(Some(v)))
                        .collect::<Vec<_>>()
                        .join(", "),
                    server_info_name(first).unwrap_or_else(|| "missing".to_string())
                ),
                None => "no supportedVersions in the server/discover result".to_string(),
            };
        }
        if first.contains_key("tools") {
            return match first.get("tools").and_then(Value::as_array) {
                Some(tools) => format!(
                    "{} tools, {} with input schema",
                    tools.len(),
                    js_str(first.get("with_input_schema"))
                ),
                None => "no tools array".to_string(),
            };
        }
        if first.contains_key("error_code") {
            return format!("error code {}", js_str(first.get("error_code")));
        }
    }

    if check.handler == HandlerBinding::CorsPreflight {
        let side = |label: &str, row: Option<&EvidenceItem>| -> String {
            match row {
                Some(row) => {
                    let outcome = match row.get("error") {
                        Some(v) if !v.is_null() => js_str(Some(v)),
                        _ => coalesce(row.get("status"), "error"),
                    };
                    format!(
                        "{label} {outcome} allow-origin {}",
                        coalesce(row.get("allow_origin"), "absent")
                    )
                }
                None => label.to_string(),
            }
        };
        let probe = |name: &str| {
            outcome
                .evidence
                .iter()
                .find(|e| e.get("probe").and_then(Value::as_str) == Some(name))
        };
        return format!(
            "{}; {}",
            side("preflight", probe("preflight")),
            side("post", probe("post"))
        );
    }

    if check.handler == HandlerBinding::DnsDoh {
        let hit = outcome.evidence.iter().find(|e| {
            e.get("answers")
                .and_then(Value::as_f64)
                .is_some_and(|n| n > 0.0)
        });
        return match hit {
            Some(hit) => format!(
                "{}: {} record(s) via {}",
                js_str(hit.get("name")),
                js_str(hit.get("answers")),
                js_str(hit.get("resolver"))
            ),
            None => "no DNS-AID records".to_string(),
        };
    }

    if check.handler == HandlerBinding::Webmcp
        && let Some(hit) = outcome
            .evidence
            .iter()
            .find(|e| e.get("marker").is_some_and(Value::is_string))
    {
        return format!(
            "{} -> {} ({})",
            js_str(hit.get("url")),
            js_str(hit.get("status")),
            js_str(hit.get("marker"))
        );
    }

    if check.eval == Some(EvalBinding::LegacyAliasRedirects) {
        let wanted = if outcome.status == ProbeStatus::Pass {
            "pass"
        } else {
            "broken"
        };
        let decisive = outcome
            .evidence
            .iter()
            .find(|e| e.get("alias_verdict").and_then(Value::as_str) == Some(wanted))
            .unwrap_or(first);
        let note = why_lines(decisive).and_then(|lines| lines.into_iter().next());
        return format!(
            "{} -> {}{}",
            coalesce(decisive.get("url"), check.id),
            coalesce(decisive.get("status"), "error"),
            note.filter(|n| !n.is_empty())
                .map(|n| format!(" ({n})"))
                .unwrap_or_default()
        );
    }

    let item = if outcome.status == ProbeStatus::Pass {
        outcome
            .evidence
            .iter()
            .find(|e| truthy(e.get("ok")))
            .unwrap_or(first)
    } else {
        first
    };
    if truthy(item.get("error")) {
        return format!("{}: {}", js_str(item.get("url")), js_str(item.get("error")));
    }
    let why = why_lines(item).and_then(|lines| lines.last().cloned());
    let is_miss = matches!(
        outcome.status,
        ProbeStatus::Broken | ProbeStatus::Absent | ProbeStatus::Error
    );
    format!(
        "{} -> {}{}",
        coalesce(item.get("url"), check.id),
        coalesce(item.get("status"), "error"),
        why.filter(|w| is_miss && !w.is_empty())
            .map(|w| format!(" ({w})"))
            .unwrap_or_default()
    )
}

fn with_op(check: &WebCheck) -> Option<String> {
    serde_json::from_str::<Value>(check.with_json)
        .ok()?
        .get("op")?
        .as_str()
        .map(str::to_string)
}

fn server_info_name(item: &EvidenceItem) -> Option<String> {
    let name = item.get("serverInfo")?.get("name")?;
    truthy(Some(name)).then(|| js_str(Some(name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::registry::check_by_id;
    use serde_json::json;

    fn outcome(status: ProbeStatus, items: Vec<Value>) -> ProbeOutcome {
        ProbeOutcome::new(
            status,
            items
                .into_iter()
                .map(|v| v.as_object().cloned().unwrap())
                .collect(),
        )
    }

    #[test]
    fn js_string_coercion_matches_a_template_literal() {
        assert_eq!(js_str(None), "undefined");
        assert_eq!(js_str(Some(&Value::Null)), "null");
        assert_eq!(js_str(Some(&json!(404))), "404");
        assert_eq!(js_str(Some(&json!(["a", null, 2]))), "a,,2");
        assert_eq!(js_str(Some(&json!({"k": 1}))), "[object Object]");
    }

    #[test]
    fn http_rows_summarize_the_decisive_item() {
        let check = check_by_id("llms-txt").unwrap();
        let pass = outcome(
            ProbeStatus::Pass,
            vec![
                json!({"url": "https://x/a", "status": 404, "ok": false, "why": ["status 404 not in [200]"]}),
                json!({"url": "https://x/llms.txt", "status": 200, "ok": true, "why": ["status 200 in [200]"]}),
            ],
        );
        assert_eq!(
            summarize_evidence(check, &pass),
            "https://x/llms.txt -> 200"
        );
        let absent = outcome(
            ProbeStatus::Absent,
            vec![
                json!({"url": "https://x/llms.txt", "status": 404, "ok": false, "why": ["status 404 not in [200]"]}),
            ],
        );
        assert_eq!(
            summarize_evidence(check, &absent),
            "https://x/llms.txt -> 404 (status 404 not in [200])"
        );
        let failed = outcome(
            ProbeStatus::Error,
            vec![
                json!({"url": "https://x/llms.txt", "status": null, "ok": false, "why": ["request failed: TimeoutError: deadline exceeded"], "error": "TimeoutError: deadline exceeded"}),
            ],
        );
        assert_eq!(
            summarize_evidence(check, &failed),
            "https://x/llms.txt: TimeoutError: deadline exceeded"
        );
        let na = outcome(
            ProbeStatus::Na,
            vec![json!({"why": ["no resolvable probe URL"]})],
        );
        assert_eq!(summarize_evidence(check, &na), "no resolvable probe URL");
        let bare_na = outcome(ProbeStatus::Na, vec![]);
        assert_eq!(summarize_evidence(check, &bare_na), "not applicable");
    }

    #[test]
    fn mcp_rows_summarize_by_op_and_shape() {
        let init = check_by_id("mcp-initialize").unwrap();
        let good = outcome(
            ProbeStatus::Pass,
            vec![
                json!({"url": "https://x/mcp", "status": 200, "error": null, "serverInfo": {"name": "demo"}, "protocolVersion": "2025-06-18"}),
            ],
        );
        assert_eq!(
            summarize_evidence(init, &good),
            "serverInfo demo, protocol 2025-06-18"
        );
        let no_info = outcome(
            ProbeStatus::Broken,
            vec![
                json!({"url": "https://x/mcp", "status": 200, "error": null, "serverInfo": null, "protocolVersion": null}),
            ],
        );
        assert_eq!(
            summarize_evidence(init, &no_info),
            "no serverInfo in initialize result"
        );
        let refused = outcome(
            ProbeStatus::Absent,
            vec![
                json!({"url": "https://x/mcp", "status": 200, "error": null, "error_code": -32601, "why": ["no modern lane: server/discover refused with code -32601"]}),
            ],
        );
        let discover = check_by_id("mcp-server-discover").unwrap();
        assert_eq!(
            summarize_evidence(discover, &refused),
            "no modern lane: server/discover refused with code -32601"
        );
        let tools = check_by_id("mcp-tools-list").unwrap();
        let listed = outcome(
            ProbeStatus::Pass,
            vec![
                json!({"url": "https://x/mcp", "status": 200, "error": null, "tools": ["a", "b"], "with_input_schema": 1}),
            ],
        );
        assert_eq!(
            summarize_evidence(tools, &listed),
            "2 tools, 1 with input schema"
        );
        let unknown = check_by_id("mcp-unknown-method").unwrap();
        let coded = outcome(
            ProbeStatus::Pass,
            vec![
                json!({"url": "https://x/mcp", "status": 200, "error": null, "error_code": -32601}),
            ],
        );
        assert_eq!(summarize_evidence(unknown, &coded), "error code -32601");
    }

    #[test]
    fn cors_dns_webmcp_and_alias_rows_have_their_own_lines() {
        let cors = check_by_id("mcp-cors-preflight").unwrap();
        let pair = outcome(
            ProbeStatus::Na,
            vec![
                json!({"probe": "preflight", "url": "u", "status": 204, "allow_origin": null, "error": null, "why": ["no Allow-Origin on the preflight or the POST: consistent no-CORS posture"]}),
                json!({"probe": "post", "url": "u", "status": 200, "allow_origin": null, "error": null}),
            ],
        );
        assert_eq!(
            summarize_evidence(cors, &pair),
            "no Allow-Origin on the preflight or the POST: consistent no-CORS posture"
        );
        let mut broken = pair.clone();
        broken.status = ProbeStatus::Broken;
        assert_eq!(
            summarize_evidence(cors, &broken),
            "preflight 204 allow-origin absent; post 200 allow-origin absent"
        );
        let dns = check_by_id("dns-aid").unwrap();
        let hit = outcome(
            ProbeStatus::Pass,
            vec![
                json!({"name": "_agents.x", "resolver": "https://dns.google/resolve", "dns_status": 0, "answers": 2}),
            ],
        );
        assert_eq!(
            summarize_evidence(dns, &hit),
            "_agents.x: 2 record(s) via https://dns.google/resolve"
        );
        let miss = outcome(
            ProbeStatus::Absent,
            vec![json!({"name": "_agents.x", "resolver": "r", "dns_status": 3, "answers": 0})],
        );
        assert_eq!(summarize_evidence(dns, &miss), "no DNS-AID records");
        let webmcp = check_by_id("webmcp").unwrap();
        let marked = outcome(
            ProbeStatus::Pass,
            vec![
                json!({"url": "https://x/", "status": 200, "ok": true, "marker": "webmcp script asset"}),
            ],
        );
        assert_eq!(
            summarize_evidence(webmcp, &marked),
            "https://x/ -> 200 (webmcp script asset)"
        );
        let alias = check_by_id("mcp-card-legacy-aliases").unwrap();
        let redirected = outcome(
            ProbeStatus::Pass,
            vec![
                json!({"url": "https://x/.well-known/mcp/server-cards.json", "role": "alias", "status": 404, "alias_verdict": "n_a", "why": ["404 alias not published"]}),
                json!({"url": "https://x/.well-known/mcp.json", "role": "alias", "status": 301, "alias_verdict": "pass", "why": ["301 -> /.well-known/mcp/server-card.json"]}),
            ],
        );
        assert_eq!(
            summarize_evidence(alias, &redirected),
            "https://x/.well-known/mcp.json -> 301 (301 -> /.well-known/mcp/server-card.json)"
        );
    }
}
