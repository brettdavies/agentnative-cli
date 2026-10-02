// Single definition point (STAR) for every User-Agent the web-audit engine
// sends. These strings are behavioral test inputs, not plumbing: sites key
// content negotiation and bot policy on the literal value, so a drifted
// duplicate silently changes what a check measures. The registry references
// them as `{ua:...}` tokens that the build expands
// (src/build/13-web-audit-registry.mjs), and the build rejects literal
// User-Agent values in registry.yaml so a new duplicate cannot land.
// These are outbound identities only. Classifying the User-Agents that
// visitors send is src/worker/telemetry/user-agent.ts, a separate table.

import { AUDIT_PATH } from './audit-routes';
import { CANONICAL_SITE_URL } from './site-url';

/** The auditor's own identity, sent by default on every probe (see guardedFetch). */
export const AUDIT_USER_AGENT = `anc-web-audit/1.0 (+${CANONICAL_SITE_URL}${AUDIT_PATH})`;

/**
 * Representative AI on-demand user-fetcher, the probe input for the checks
 * that measure how a site treats that client class (markdown-agent-ua,
 * agent-ua-reachable). Sites match this literal string.
 */
export const AI_USER_FETCHER_PROBE_UA = 'ChatGPT-User/1.0 (+https://openai.com/bot)';

/** Representative shell HTTP client, the probe input for markdown-cli-ua. */
export const CLI_PROBE_UA = 'curl/8.7.1';

/** Registry `{ua:...}` token map, expanded at build time. */
export const PROBE_UA_TOKENS = Object.freeze({
  '{ua:ai-user-fetcher}': AI_USER_FETCHER_PROBE_UA,
  '{ua:cli}': CLI_PROBE_UA,
});
