// The single definition of this site's production host.
//
// Imported by three module graphs with incompatible type environments —
// the Worker (tsconfig.worker.json: Workers types, no DOM), the browser
// client (tsconfig.client.json: DOM, `types: []`), and the Bun build
// (tsconfig.node.json, which reaches this `.ts` from `.mjs` callers).
// A bare string export is the only shape all three accept.
//
// Two distinct uses, and conflating them is the bug this constant
// exists to prevent:
//
//   1. Canonical/SEO identity — where the real thing lives. Always this
//      value, even in a staging build, or crawlers learn that staging is
//      the authority.
//   2. Fallback for a per-surface origin resolver, when no request,
//      `location`, or `PUBLIC_BASE_URL` is in scope.
//
// It is NOT the source for navigational links. Those resolve from the
// surface's own origin (the in-flight Request, `location.origin`, or
// `resolveBaseUrl()`) so a deployment links back to itself.
export const CANONICAL_SITE_URL = 'https://anc.dev';
