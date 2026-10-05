//! Reads the string constants the web-audit engine consumes from the
//! vendored site modules (`user-agents.ts`, `site-url.ts`, `audit-routes.ts`).
//! The probe User-Agent strings are behavioral test inputs with one
//! definition point on the site, so they are parsed out of the vendored
//! source rather than restated as Rust literals.
//!
//! The parser is deliberately narrow: `export const NAME = '<literal>';`,
//! `export const NAME = \`<template with ${IDENT} parts>\`;` and one
//! `export const NAME = Object.freeze({ '<key>': IDENT, ... });` map. Anything
//! it cannot read fails the build naming the constant.

use std::collections::BTreeMap;

/// Constants resolved from the vendored modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteConstants {
    /// The identifying User-Agent every probe sends unless a check sets its own.
    pub audit_user_agent: String,
    /// Registry `{ua:...}` token to literal User-Agent, in source order.
    pub probe_ua_tokens: Vec<(String, String)>,
}

/// Resolve `AUDIT_USER_AGENT` and the `PROBE_UA_TOKENS` map from the three
/// vendored modules.
pub fn resolve_site_constants(
    user_agents: &str,
    site_url: &str,
    audit_routes: &str,
) -> Result<SiteConstants, String> {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    for (module, src) in [
        ("site-url.ts", site_url),
        ("audit-routes.ts", audit_routes),
        ("user-agents.ts", user_agents),
    ] {
        string_consts(module, src, &mut env)?;
    }
    let audit_user_agent = env
        .get("AUDIT_USER_AGENT")
        .cloned()
        .ok_or_else(|| "user-agents.ts: no `export const AUDIT_USER_AGENT`".to_string())?;
    let mut probe_ua_tokens = Vec::new();
    for (key, ident) in freeze_map("user-agents.ts", user_agents, "PROBE_UA_TOKENS")? {
        let value = env.get(&ident).cloned().ok_or_else(|| {
            format!("user-agents.ts: PROBE_UA_TOKENS entry {key:?} names unknown constant {ident}")
        })?;
        probe_ua_tokens.push((key, value));
    }
    if probe_ua_tokens.is_empty() {
        return Err("user-agents.ts: PROBE_UA_TOKENS is empty".to_string());
    }
    Ok(SiteConstants {
        audit_user_agent,
        probe_ua_tokens,
    })
}

/// Add every `export const NAME = <string or template>;` in `src` to `env`,
/// templates resolved against the constants already in `env` (earlier
/// modules and earlier lines of this one).
fn string_consts(
    module: &str,
    src: &str,
    env: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    for line in src.lines() {
        let Some(rest) = line.trim_start().strip_prefix("export const ") else {
            continue;
        };
        let Some((name, value)) = rest.split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim().trim_end_matches(';').trim();
        let literal =
            if let Some(body) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
                body.replace("\\'", "'")
            } else if let Some(body) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
                body.replace("\\\"", "\"")
            } else if let Some(body) = value.strip_prefix('`').and_then(|v| v.strip_suffix('`')) {
                expand_template(module, name, body, env)?
            } else {
                continue;
            };
        env.insert(name.to_string(), literal);
    }
    Ok(())
}

/// Replace every `${IDENT}` in a template literal with an already-resolved
/// constant; any other placeholder fails naming the constant.
fn expand_template(
    module: &str,
    name: &str,
    body: &str,
    resolved: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| format!("{module}: {name} has an unterminated ${{ placeholder"))?;
        let ident = &after[..end];
        if !ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!(
                "{module}: {name} placeholder ${{{ident}}} is not a bare identifier"
            ));
        }
        let value = resolved.get(ident).cloned().ok_or_else(|| {
            format!("{module}: {name} references {ident}, which is not a resolved constant")
        })?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The `'<key>': IDENT` entries of `export const NAME = Object.freeze({ ... })`.
fn freeze_map(module: &str, src: &str, name: &str) -> Result<Vec<(String, String)>, String> {
    let decl = format!("export const {name} = Object.freeze({{");
    let start = src
        .find(&decl)
        .ok_or_else(|| format!("{module}: no `{decl} ...` declaration"))?;
    let body = &src[start + decl.len()..];
    let end = body
        .find("})")
        .ok_or_else(|| format!("{module}: {name} map is unterminated"))?;
    let mut entries = Vec::new();
    for raw in body[..end].split(',') {
        let entry = raw.trim();
        if entry.is_empty() {
            continue;
        }
        let quote = entry
            .chars()
            .next()
            .filter(|q| *q == '\'' || *q == '"')
            .ok_or_else(|| format!("{module}: {name} entry {entry:?} is not `'key': IDENT`"))?;
        let close = entry[1..]
            .find(quote)
            .map(|i| i + 1)
            .ok_or_else(|| format!("{module}: {name} entry {entry:?} has an unterminated key"))?;
        let key = entry[1..close].to_string();
        let ident = entry[close + 1..]
            .trim()
            .strip_prefix(':')
            .ok_or_else(|| format!("{module}: {name} entry {entry:?} is not `'key': IDENT`"))?
            .trim()
            .to_string();
        if !ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!(
                "{module}: {name} entry {key:?} value {ident:?} is not a bare identifier"
            ));
        }
        entries.push((key, ident));
    }
    Ok(entries)
}
