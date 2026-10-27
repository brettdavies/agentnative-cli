# Web-audit conformance corpus

Golden fixtures that pin the web-audit engine for the CLI's Rust port. Each scenario pairs the exchanges a
stub fetch answers with the scorecard the engine produces over them; both engines must reproduce
`scorecard.json` byte for byte from `scenario.json`. The generator is `scripts/web-audit/gen-fixtures.ts`
and the scenarios are authored in `scripts/web-audit/conformance-scenarios.ts`; this directory is its output
and `tests/web-audit-conformance-corpus.test.ts` fails when the two disagree.

## Layout

```text
tests/fixtures/web-audit-conformance/
  README.md                        this file
  regex-parity.json                every registry pattern x a fixed probe table -> RegExp boolean
  scores.json                      every scenario's scores and row statuses, one line per row
  scenarios/<name>/scenario.json   input: target, site type, exchanges, unmatched policy
  scenarios/<name>/scorecard.json  output: the engine's scorecard, normalized as described below
```

## scenario.json

- `description`: what the scenario exercises.
- `covers`: the check ids the scenario is the subject of; the completeness gate requires every registry id
  to appear in at least one scenario's list.
- `target`: the URL handed to the engine.
- `site_type`: `"content"`, `"api"` or `null` (run everything).
- `spec_version`: the literal both engines are given for the run.
- `follow_declarations` (optional): `false` runs with following off, so no declared host is requested and the
  trail records each declaration not followed. Absent means `true`. Both engines run every scenario with a per-domain
  budget that admits every domain, so no budget state reaches a golden.
- `unmatched`: the response for a request no exchange matches, either a transport failure
  (`{"error": "Name: message"}`) or a full response.
- `allow_unmatched`: when false, generation fails if any request reaches the unmatched policy.
- `exchanges`: ordered rules; the first match wins and a rule may match repeatedly.

Matching: `method` compares case-insensitively; `url` compares after both sides pass through the URL
parser; `headers` is a subset match on lowercase names with exact values; `body_json_method` parses the
request body as JSON and compares its top-level `method`; `body_contains` is a substring match on the raw
body.

Responses carry lowercase header names with single string values and a UTF-8 text body exactly as the engine reads it.
No response carries `content-encoding`: decompression is pinned by transport tests, not by the corpus. A transport
failure is `{"error": "Name: message"}`, the string `ProbeResponse.error` carries at the seam; a `TimeoutError` is
always recorded as `TimeoutError: deadline exceeded`. Redirects are ordinary exchanges (a 3xx with a `location`
header) that the guarded fetch above the seam follows with a new request, except where the engine keeps a probe off
hosts nothing confirmed. The guarded fetch sends nothing over `http`: a request URL on `http` never reaches the seam
and reads as a failure with the error `not requested: not https`, and a redirect to `http` it would otherwise follow
is not taken, so the probe's answer is that redirect's status and headers with an empty body and the error `redirect
refused: <status> to <location>: not https`. A request to the MCP endpoint on the audited origin (discovery's
common-path POSTs and every probe of that endpoint) takes only hops that keep the scheme, host, and port: discovery
records a redirect to another origin with its target and declares the target, and any other probe reads it as a
refused redirect. A probe of an endpoint on a declared host takes no redirect at all, and neither does a probe of a
document on that endpoint's origin (a registry path written with `{mcp_origin}`, which the engine replaces with the
endpoint's scheme, host, and port). The GET an API anchor host off the audited origin receives takes only hops that
keep the scheme, host, and port, and a redirect to another origin is its answer.

## scorecard.json

The engine's terminal scorecard as `JSON.stringify(scorecard, null, 2)` plus a trailing newline, with one
normalization applied before writing and expected of any engine under comparison: `results` is sorted into
registry order, because wave-2 rows arrive in the order the concurrent probes resolve, which carries no
meaning and which a second engine cannot reproduce. Rows carry the summarized `evidence` line, never the raw
evidence, so no wall-clock value reaches the file. Key order is otherwise the engine's emission order and
every number is an integer. A scenario that ends in
the engine's `unreachable` event writes `{"unreachable": "<reason>"}` instead of a scorecard. The engine runs
under a fixed clock, so no per-audit deadline fires; per-probe timeouts appear only as declared transport
failures. Every scenario is a public-vantage audit holding no credential, so every scorecard records
`"vantage": {"network": "public", "credentialed": false}`, and an engine under comparison runs the scenarios at
that vantage.

Each row's `hosts` lists the distinct hosts its raw evidence items were requested from, in evidence order, as
`{"host": ...}` objects. Only an item with no `blocked` marker counts: a URL the SSRF guard refused, or one never
requested because it is not `https`, reached no host. An item with a string `url` counts that URL's host; one with no
`url` and a non-empty string `host` counts that value, which is how a row a declared host kept from being evaluated
names that host. The host is the WHATWG URL `host`, which keeps a non-default port (`example.com:8443`), so an engine
whose URL library drops the port must add it back. An item whose `url` does not parse contributes nothing, and a row
with no counting item has `hosts: []`. `host` is present, holding the same value, exactly when `hosts` has one entry.
A row that evaluates several targets (the API rows when the api-catalog lists API anchors: one per declared
description, one per anchor host) marks each target's items with its outcome, and when those items name more than one
host each `hosts` entry also carries `status`: the worst outcome among that host's targets (`broken`, then
`noncompliant`, `absent`, `error`, `pass`), or `n_a` with the first such target's `na_reason` when none on that host
was evaluated. The row's own status is the same rule over all its targets.

`declared_hosts` holds one entry per URL the target's discovery documents declare off its origin, in declaration
order (the AI catalog's card entries, the card under the discovered endpoint, then the well-known cards; the endpoints a
followed card document names come right after that document's entry), then the api-catalog's anchors in linkset order
(`api-anchor`; one with no `service-desc` other than an MCP surface reads `not-followed` with reason
`no-service-desc`), then the description each API anchor declares (`api-description`) in the same order, then the
targets the common-path POSTs were redirected to off the origin, in probe order and with the redirecting path as
their `surface`, never in the order requests complete. A URL declared twice keeps its first entry. Endpoints are tried
one at a time in that order and the first that its own host confirms becomes the endpoint, so every later endpoint
reads `not-followed`. A declared URL or redirect hop on `http` that the guard admits is never requested and
reads `not-followed` with reason `insecure-scheme`, a refused hop recorded as its `final_url`.

## scores.json

One entry per scenario, keyed by scenario name in sorted order: `score_pct`, `score` (`relative` and
`global`), and `results`, one line per row in registry order carrying the row's `id`, `status`, and
`na_reason` when it has one. A scenario that ends unreachable reads `{"unreachable": true}`. Every value is
copied from the scenario's `scorecard.json`, so the file adds no contract of its own; it turns a regeneration that
moves a score or a row status into a short diff of one file instead of a change buried in a full golden.

## regex-parity.json

`probes` is one fixed string table; `patterns` lists every `content_type`, `header_regex`, `body_regex`
and `body_not_regex` value in the normalized registry with the flags `assert.ts` compiles it under (`i` for
the first two, `im` for the body patterns) and `results[i] = new RegExp(pattern, flags).test(probes[i])`.

## Scenarios

| Scenario | Exercises | Subject checks |
| -------- | --------- | -------------- |
| `alias-inline-copy-noncompliant` | a legacy path serving its own copy of the card is noncompliant | `mcp-card-legacy-aliases` |
| `alias-non-permanent-broken` | a 302 to the canonical card signals no canonical intent and is broken | `mcp-card-legacy-aliases` |
| `alias-redirect-away-broken` | a legacy path that 301s away from the canonical card is broken | `mcp-card-legacy-aliases` |
| `alias-redirect-pass` | one legacy card path 301s to the canonical card, which is enough to pass | `mcp-card-legacy-aliases`, `mcp-server-card`, `mcp-usage-doc` |
| `alias-unpublished-optional-absent` | no legacy path published at all is optional-absent, never a penalty | `mcp-card-legacy-aliases` |
| `api-anchor-hosts` | a Stripe-shaped api-catalog anchors two API hosts and an MCP endpoint with no service-desc: the OpenAPI row scores each declared description where it is hosted (JSON on one host, YAML on the other), the hygiene rows probe each anchor host once (the documented 4xx path, then the nonsense path) and list both hosts in anchor order, JSON errors pass and rate-limit headers are missing at both, the third anchor is recorded not followed and never requested, and no hygiene probe reaches the audited site | `openapi`, `api-catalog`, `json-errors`, `rate-limit-headers` |
| `api-description-over-cap` | an API anchor declares a JSON description larger than the 512 KiB read cap whose `openapi` key sits after its components, past the bytes read: the OpenAPI row counts it present from the truncated read, and the hygiene probes, with no parsed description to take a path from, fall back to the nonsense path on the anchor host | `openapi` |
| `api-hygiene-fallback-path` | with no usable OpenAPI body the probe falls back to the well-known nonsense path | `json-errors`, `rate-limit-headers` |
| `api-hygiene-html-error` | an HTML error body on the API probe is broken and carries no rate-limit header | `json-errors`, `rate-limit-headers` |
| `api-hygiene-json-200` | a 200 JSON body where a client error was expected is absent for json-errors | `json-errors` |
| `api-hygiene-pass` | a documented 4xx GET answered with a JSON error body and rate-limit headers passes both rows | `json-errors`, `rate-limit-headers` |
| `auth-bare-401` | the audited site's /mcp answers 401 with no challenge and no metadata names it: a bare 401 is refusal evidence, so no endpoint is found | `mcp-initialize` |
| `auth-declared-endpoint` | the card names a root endpoint on another host that answers 401 and publishes metadata naming it without the trailing slash: the metadata admits it, and the MCP rows are scored there with sign-in required | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-server-discover`, `mcp-malformed-body`, `mcp-modern-version-reject`, `mcp-get-fast-fail`, `mcp-cors-preflight`, `mcp-cors-actual`, `mcp-auth-challenge`, `mcp-auth-servers`, `mcp-auth-enforced`, `oauth-protected-resource` |
| `auth-echo-unanswered` | the audited site's card declares its /mcp, which answers every POST with a 401 naming same-host root RFC 9728 metadata that names it, and the metadata read at a nonsense path draws a 503: the card already made the endpoint of record, so an echo read that got no answer leaves sign-in settled, the rows the 401s answer read auth-required, and none reads broken | `mcp-initialize`, `mcp-tools-list`, `mcp-auth-challenge`, `mcp-auth-servers`, `mcp-auth-enforced` |
| `auth-echoing-gateway` | the audited site's /mcp answers 401 naming path-suffixed metadata, but the host answers a nonsense path's metadata with that path as its resource too: the metadata confirms nothing, so no endpoint is found | `mcp-initialize` |
| `auth-enforcement-defects` | an endpoint that requires sign-in lists an http authorization server and serves a legacy tools/list without a token: the challenge row passes, the metadata row is broken, and the refusal row is noncompliant | `mcp-auth-challenge`, `mcp-auth-servers`, `mcp-auth-enforced` |
| `auth-later-401` | the audited site's /mcp serves initialize without a token and refuses server/discover with a method-not-found, while every other request it reads a token for draws a 401 naming same-host RFC 9728 metadata that names it: no handshake asked for sign-in, so the endpoint presents the open design and the sign-in rows are n_a, and each later row whose 401 that metadata backs reads auth-required rather than broken | `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-unknown-tool`, `mcp-accept-json`, `mcp-auth-enforced` |
| `auth-md-absent` | no auth document at either path is absent, which a MAY finalizes as optional-absent | `auth-md` |
| `auth-md-bare-heading` | a document without a content type but opening with a markdown heading still passes | `auth-md` |
| `auth-md-html-broken` | an HTML page at the auth.md path is present but malformed | `auth-md` |
| `auth-md-pass` | a markdown auth document passes once the auth antecedent holds | `auth-md`, `oauth-discovery` |
| `auth-modern-only` | a modern-only server behind OAuth at the audited site's /mcp refuses every legacy POST at HTTP 200 with a JSON-RPC error before reading a token, while every modern POST draws a 401 naming same-host RFC 9728 metadata that names it: the endpoint is found with sign-in required, the modern session rows and the resources rows read auth-required, the legacy session rows read the legacy refusal as they do on an open modern-only server, and the refusal row is asked on the modern lane, where it passes | `mcp-server-discover`, `mcp-modern-tools-list`, `mcp-tools-list`, `mcp-auth-enforced` |
| `auth-open-endpoint` | an open server whose card documents that no sign-in is required: no wire probe draws a 401, so the sign-in rows are `n_a` | `mcp-auth-challenge`, `mcp-auth-servers`, `mcp-auth-enforced` |
| `auth-own-endpoint` | the audited site's /mcp answers every POST with a 401 whose challenge names same-host RFC 9728 metadata naming it: the endpoint is found with sign-in required, rows that need no session are scored, and the rest read auth-required | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-server-discover`, `mcp-malformed-body`, `mcp-modern-version-reject`, `mcp-get-fast-fail`, `mcp-cors-preflight`, `mcp-cors-actual`, `mcp-auth-challenge`, `mcp-auth-servers`, `mcp-auth-enforced`, `oauth-protected-resource` |
| `auth-servers-mixed` | an endpoint that requires sign-in lists a public https authorization server between an http one and one on a private address: an agent can still sign in through the usable server, so the metadata row is noncompliant and names both unusable entries, and none of the three is requested | `mcp-auth-servers` |
| `content-non-2xx-broken` | a rich root served with a 5xx is broken, not a pass | `content-without-js` |
| `content-rich-pass` | rich HTML with an H1 and visible text passes without probing the twin | `content-without-js` |
| `content-thin-absent` | thin HTML with no llms.txt is absent | `content-without-js` |
| `content-thin-dead-links-absent` | thin HTML whose llms.txt links do not resolve is absent | `content-without-js`, `llms-txt-links` |
| `content-thin-twin-na` | thin HTML softened by a live llms.txt content link is `n_a` | `content-without-js` |
| `cors-full-pass` | Allow-Origin on both surfaces passes both rows | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-post-only` | the POST carries Allow-Origin but the preflight does not: preflight broken, actual pass | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-post-rate-limited-no-cors` | a bare preflight beside a POST answered HTTP 408 is an operational unknown on both rows, not a declared posture | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-post-transport-failure-no-cors` | a bare preflight beside a failed POST is an operational unknown, not a declared posture | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-posture-consistent` | no Allow-Origin on the preflight or the POST is a consistent no-CORS posture: both rows `n_a` | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-preflight-500-with-acao` | Allow-Origin on a failing preflight is misconfigured: preflight broken, actual classifies from its own POST | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-preflight-only` | the preflight declares CORS but the POST omits Allow-Origin: preflight pass, actual broken | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-preflight-transport-failure` | a transport failure on the preflight suppresses only the preflight row; the actual row classifies from its POST | `mcp-cors-preflight`, `mcp-cors-actual` |
| `discovery-card-generations-auth` | an inline SEP-2127 catalog card and a SEP-1649 card declaring authentication name one endpoint that never answers 401; the catalog card wins the endpoint and the SEP-1649 declaration still satisfies the auth antecedents, so the OAuth rows are scored | `oauth-protected-resource`, `auth-md` |
| `discovery-catalog-card` | an AI catalog entry names a SEP-2127 card on the audited origin; its streamable-http remote is the endpoint, so no common path is POSTed, and the card passes the card check | `ai-catalog`, `mcp-initialize`, `mcp-server-card` |
| `discovery-catalog-declarations` | catalog cards off the audited origin or behind a URL template are declared, never requested; discovery falls through to initialize | `mcp-initialize` |
| `discovery-catalog-inline-card` | an AI catalog entry carries its SEP-2127 card inline; the card is read in place and its remote is the endpoint | `ai-catalog`, `mcp-initialize` |
| `discovery-legacy-card` | a SEP-1649 card at the well-known path names the endpoint in transport.url; no common path is POSTed and the card suffix answers 404 | `mcp-server-card`, `mcp-initialize` |
| `discovery-suffix-card` | no catalog and no well-known card: initialize finds the endpoint, and the SEP-2127 card under the endpoint at /mcp/server-card is the card of record | `mcp-initialize` |
| `discovery-suffix-card-no-remotes` | no catalog and no well-known card: initialize finds the endpoint, and the card at /mcp/server-card carries the SEP-2127 $schema and every required field but no remotes, which the extension schema allows: it is the card of record and passes the card check | `mcp-server-card` |
| `dns-aid-nxdomain` | every name resolves NXDOMAIN: absent, which a MAY finalizes as optional-absent | `dns-aid` |
| `dns-aid-pass` | the first resolver answers Status 0 with a record for the index name | `dns-aid` |
| `dns-aid-resolver-fallback` | the first resolver fails at the resolver level and the second answers | `dns-aid` |
| `dns-aid-resolvers-unreachable` | every resolver fails at the transport level: an operational error, not an absence | `dns-aid` |
| `follow-card-admit` | the card names an endpoint on another host, whose own card at `<endpoint>/server-card` names it: the MCP rows are scored there | `mcp-initialize`, `mcp-tools-list`, `mcp-cors-preflight`, `mcp-cors-actual`, `mcp-get-fast-fail` |
| `follow-catalog-card-document` | the AI catalog names, by URL, the SEP-2127 card at `<endpoint>/server-card` on another host, and no well-known card exists: the follow slice reads that card, its remote is the endpoint of record, and the card passes the card check there | `mcp-server-card`, `mcp-initialize` |
| `follow-disabled` | the same declared endpoint as follow-card-admit with following off: nothing off the audited origin is requested and the MCP rows read follow-disabled | `mcp-initialize`, `mcp-tools-list`, `mcp-cors-preflight`, `mcp-cors-actual`, `mcp-get-fast-fail` |
| `follow-host-cap` | four declared hosts each redirect into a private range and are blocked; the fifth exceeds the per-audit host cap and is never requested | `mcp-initialize` |
| `follow-host-catalog-card` | the well-known SEP-1649 card names an endpoint on another host that serves no card at `<endpoint>/server-card`, but its host's own AI catalog lists, by URL, a SEP-2127 card naming it: that card admits the endpoint and, outranking the SEP-1649 card, passes the card check with no advisory | `mcp-server-card` |
| `follow-http-declarations` | the AI catalog names an https endpoint that redirects to http, the card names an http endpoint, and the api-catalog anchors an http API host whose description is http, each host answering as one the audit would follow: nothing is requested over http, every entry reads not-followed with reason insecure-scheme (the redirected one with its hop as the final URL), and no MCP or API row is evaluated at any of them: the API rows read absent, as nothing declared over http earns more than a missing API | `mcp-initialize`, `openapi`, `json-errors` |
| `follow-own-redirect-admit` | the audited site's /mcp answers the discovery POSTs with a 307 to another host whose card at `<endpoint>/server-card` names it: no POST follows the redirect, the target is confirmed like a declared endpoint, and the MCP rows are scored there | `mcp-initialize`, `mcp-tools-list`, `mcp-cors-preflight`, `mcp-cors-actual`, `mcp-get-fast-fail` |
| `follow-own-redirect-refused` | the audited site's /mcp answers the discovery POSTs with a 307 to another host that publishes nothing naming that URL: no POST or OPTIONS reaches the host, and the MCP rows name the host that did not confirm it | `mcp-initialize`, `mcp-tools-list`, `mcp-cors-preflight`, `mcp-cors-actual`, `mcp-get-fast-fail` |
| `follow-reciprocity-refused` | the declared endpoint answers GET with 405 and Allow: POST but publishes no card, catalog entry, or metadata naming it: no wire probe, and the MCP rows name the host that did not confirm it | `mcp-initialize`, `mcp-tools-list`, `mcp-cors-preflight`, `mcp-cors-actual`, `mcp-get-fast-fail` |
| `follow-redirect-hop` | the declared endpoint redirects once to another public host whose card names the final URL: the final URL is the endpoint and the trail records both | `mcp-initialize`, `mcp-tools-list`, `mcp-cors-preflight`, `mcp-cors-actual`, `mcp-get-fast-fail` |
| `frontmatter-absent` | a twin opening with prose is absent, which a MAY finalizes as optional-absent | `markdown-frontmatter` |
| `frontmatter-crlf-pass` | CRLF line endings and a leading BOM are tolerated | `markdown-frontmatter` |
| `frontmatter-no-key-line-broken` | a fence pair enclosing no key line is broken | `markdown-frontmatter` |
| `frontmatter-pass` | a markdown twin opening with a terminated frontmatter block passes | `markdown-frontmatter` |
| `frontmatter-unterminated-broken` | a leading fence with a key line but no terminator is broken | `markdown-frontmatter` |
| `http-404-markdown-no-recovery` | a markdown 404 body without a recovery link misses the same-origin-recovery assertion | `agent-friendly-404-md` |
| `http-404-markdown-recovery` | a markdown 404 body with a same-origin recovery link passes, and a real 404 status passes the plain check | `agent-friendly-404`, `agent-friendly-404-md` |
| `http-affordance-absent` | a failed body assertion without a status expectation is absent, not broken | `root-meta-description`, `noscript-fallback`, `semantic-html`, `schema-org-jsonld`, `root-link-rel` |
| `http-challenge-interstitial` | an AI user-fetcher that receives a bot challenge page fails the reachability check | `agent-ua-reachable`, `markdown-agent-ua` |
| `http-content-type-mismatch` | a 200 with the wrong content type is broken (present but invalid) | `json-schemas` |
| `http-discovery-cards` | the well-known discovery and auth documents answer with the expected JSON shapes | `a2a-agent-card`, `ai-catalog`, `agent-skills`, `security-txt`, `web-bot-auth`, `oauth-discovery`, `api-catalog`, `sitemap`, `llms-full-txt` |
| `http-discovery-cards-broken` | discovery documents that answer 200 with the wrong shape are broken | `a2a-agent-card`, `ai-catalog`, `agent-skills`, `security-txt`, `web-bot-auth` |
| `http-get-fast-fail-html-broken` | a GET with Accept text/event-stream that returns 200 HTML is broken | `mcp-get-fast-fail` |
| `http-get-fast-fail-markdown-broken` | a GET with Accept text/event-stream that returns 200 markdown is broken | `mcp-get-fast-fail` |
| `http-get-fast-fail-sse-pass` | a GET that returns 200 text/event-stream passes | `mcp-get-fast-fail` |
| `http-get-fast-fail-timeout-broken` | a held-open GET on the MCP endpoint times out against its explicit budget and is broken | `mcp-get-fast-fail` |
| `http-header-regex-absent` | Link and Vary header assertions miss when the headers are absent | `link-headers`, `markdown-vary` |
| `http-header-regex-pass` | Link and Vary header assertions pass on the root headers | `link-headers`, `markdown-vary` |
| `http-llms-txt-absent` | a 404 on the only candidate is absent | `llms-txt` |
| `http-llms-txt-pass` | a 200 llms.txt with a link index passes and retains its body | `llms-txt` |
| `http-mixed-candidates-broken` | across `path_any` candidates a broken candidate outranks absent ones | `agent-skills` |
| `http-path-any-second-candidate` | `openapi` passes on its second `path_any` candidate | `openapi` |
| `http-robots-ai-rules` | robots.txt with AI-crawler rules and content signals passes both gated checks | `robots`, `robots-ai-rules`, `content-signals` |
| `http-robots-broken` | a 5xx where a document is expected is broken | `robots` |
| `http-robots-no-ai-rules` | robots.txt without a User-agent line or Content-Signal misses both gated checks | `robots-ai-rules`, `content-signals` |
| `http-soft-404` | a 200 shell on an unknown path is a soft 404 and broken | `agent-friendly-404`, `agent-friendly-404-md` |
| `http-transport-timeout-error` | a timeout on a check without an explicit hang budget is an operational error | `llms-txt` |
| `http-ua-negotiation` | the CLI and AI user-agent probes receive the markdown twin while the default probe receives HTML | `markdown-cli-ua`, `markdown-agent-ua`, `accept-markdown`, `markdown-accept-plain`, `agent-ua-reachable` |
| `http-ua-negotiation-absent` | every markdown-shaped request receives HTML, so the twin family is absent | `markdown-cli-ua`, `markdown-agent-ua`, `accept-markdown`, `markdown-accept-plain` |
| `llms-quality-angle-bracket-pass` | an llms.txt link whose destination sits in angle brackets and holds a space and parentheses is fetched without the brackets, and the links row passes | `llms-txt-links` |
| `llms-quality-broken-link` | a link that answers 5xx makes the links row broken | `llms-txt-links` |
| `llms-quality-dead-and-http-link` | an llms.txt that lists a dead link and an http link: the http link is never requested, though it would answer with a server error that would read broken, and the dead link decides the links row, which reads absent | `llms-txt-links` |
| `llms-quality-dead-link` | a link that answers 404 makes the links row absent | `llms-txt-links` |
| `llms-quality-h1-only` | an llms.txt with only an H1 misses format, has no links to follow, and has no when-to-use heading | `llms-txt-format`, `llms-txt-links`, `llms-txt-when-to-use` |
| `llms-quality-http-link` | an llms.txt that lists an http link beside resolving https links: the http link is never requested, though it would answer, and the links row reads noncompliant naming it | `llms-txt-links` |
| `llms-quality-pass` | an llms.txt with an H1, a summary, resolving links and a when-to-use heading passes the trio | `llms-txt-format`, `llms-txt-links`, `llms-txt-when-to-use` |
| `mcp-capabilities-empty` | an initialize result with empty capabilities passes initialize but is broken on the capabilities row | `mcp-initialize`, `mcp-capabilities` |
| `mcp-card-no-endpoint-field` | a card without an endpoint field that names its server in serverInfo is SEP-1649-shaped and passes the card check with the superseded advisory, while discovery falls through to initialize | `mcp-server-card`, `mcp-initialize` |
| `mcp-card-off-origin` | a card declaring an off-origin endpoint is recorded and never probed; discovery falls through to initialize | `mcp-server-card`, `mcp-initialize` |
| `mcp-discover-rate-limited` | a 429 on server/discover alone leaves the modern lane unknown, so the modern rows probe on their own answers instead of reading absent | `mcp-server-discover`, `mcp-modern-tools-list`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss` |
| `mcp-dual-stack` | a dual-stack server passes every legacy, modern, conformance and negotiation row | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-edges-rate-limited` | the GET and the preflight answer HTTP 429, the preflight with Allow-Origin, while the Origin-bearing POST carries Allow-Origin: the GET fast-fail and preflight rows are operational errors, and the actual row, which its own Allow-Origin settles, passes | `mcp-get-fast-fail`, `mcp-cors-preflight`, `mcp-cors-actual` |
| `mcp-garbage-responses` | a discovered endpoint that answers every POST with an HTML 500 is broken on every probed row | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-http-rate-limited` | an HTTP 429 or 408 answer is an operational error on every row whatever body rides it, like a -32099 refusal | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-legacy-lane` | a legacy-only server discovered by initialize: legacy rows pass, modern rows read the lane as absent | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-modern-lane` | a modern-only server discovered by the header-routed fallback: modern rows pass, legacy rows read the lane as absent | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-negotiation-defects` | a stream answered to a JSON-only client is broken and a 200 with an unasked-for type is noncompliant | `mcp-accept-json`, `mcp-accept-unsatisfiable` |
| `mcp-open-stream` | a server that answers initialize on a stream it never closes: the seam records the per-check timeout, exactly as a live run does | `mcp-initialize`, `mcp-capabilities` |
| `mcp-rate-limited` | a -32099 rate-limit refusal is an operational error on every row, never a penalty | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-resources-empty-broken` | resources advertised but resources/list returns an empty array is broken | `mcp-resources-list` |
| `mcp-resources-unadvertised` | capabilities that omit resources gate both resources rows to `n_a` | `mcp-resources-list`, `mcp-modern-resources-miss` |
| `mcp-server-card-html-200` | the well-known card path answers 200 with the HTML app shell, a soft 404: no card is published, so the card check reads absent | `mcp-server-card` |
| `mcp-server-card-missing-field` | an AI catalog names a SEP-2127 card that omits the required name: its remote is still the endpoint, and the card check reads broken naming the missing field | `mcp-server-card` |
| `mcp-server-card-unparseable` | the well-known card answers 200 as application/json with a body that does not parse: the card is published, so the card check reads broken rather than absent | `mcp-server-card` |
| `mcp-sse-framing` | a legacy tools/list answered as text/event-stream parses the first data line | `mcp-tools-list` |
| `mcp-stateful-session` | a stateful server issues a session on initialize, refuses sessionless conformance probes with -32000, and is scored on the re-ask | `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-tools-list`, `mcp-resources-list` |
| `mcp-typed-http-refusals` | bare HTTP 400/415 refusals with no envelope conform where the row allows them; a bare 404 never does | `mcp-malformed-body`, `mcp-accept-unsatisfiable`, `mcp-batch-reject`, `mcp-unknown-tool` |
| `mcp-wrong-error-codes` | conformance probes refused under the wrong code are noncompliant; a result where a refusal was required is broken | `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-unknown-method`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss` |
| `mcp-www-authenticate` | an endpoint that answers legacy POSTs with a 401 whose challenge names no metadata, while root RFC 9728 metadata names it: it requires sign-in, so the legacy session rows, the resources rows, and every row a 401 answers read auth-required, and the challenge satisfies the mcp-auth antecedent; its modern lane refuses server/discover with a method-not-found and no 401, an answer a token would not change, so the modern session rows read absent as they do on an open server | `mcp-initialize`, `mcp-server-discover`, `mcp-auth-challenge`, `oauth-protected-resource`, `auth-md` |
| `run-all-pass` | every surface answers as the registry wants it to | `openapi`, `json-schemas`, `api-catalog`, `json-errors`, `rate-limit-headers`, `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail`, `mcp-cors-preflight`, `mcp-cors-actual`, `mcp-server-card`, `mcp-card-legacy-aliases`, `mcp-usage-doc`, `webmcp`, `llms-txt`, `llms-txt-format`, `llms-txt-links`, `llms-txt-when-to-use`, `llms-full-txt`, `llms-txt-scoped`, `llms-full-txt-scoped`, `accept-markdown`, `markdown-cli-ua`, `markdown-agent-ua`, `agent-ua-reachable`, `markdown-accept-plain`, `markdown-vary`, `markdown-frontmatter`, `root-meta-description`, `schema-org-jsonld`, `content-without-js`, `semantic-html`, `noscript-fallback`, `robots`, `sitemap`, `agent-friendly-404`, `agent-friendly-404-md`, `link-headers`, `root-link-rel`, `dns-aid`, `robots-ai-rules`, `content-signals`, `web-bot-auth`, `security-txt`, `a2a-agent-card`, `ai-catalog`, `agent-skills`, `oauth-discovery`, `oauth-protected-resource`, `auth-md` |
| `run-antecedent-unmet` | a bare HTML site: no llms.txt, no API surface, no MCP endpoint, so every gated check is `n_a` | `llms-txt-format`, `llms-txt-links`, `llms-txt-when-to-use`, `llms-txt-scoped`, `llms-full-txt-scoped`, `json-schemas`, `api-catalog`, `json-errors`, `rate-limit-headers`, `mcp-cors-preflight`, `mcp-cors-actual`, `mcp-server-card`, `mcp-card-legacy-aliases`, `mcp-usage-doc`, `oauth-protected-resource`, `robots-ai-rules`, `content-signals`, `auth-md`, `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `run-body-over-cap` | bodies past the 64 KiB probe cap are truncated: a huge tools/list no longer parses and a huge JSON error body reads as non-JSON | `mcp-tools-list`, `json-errors` |
| `run-document-redirects-to-http` | the audited site answers /llms.txt and its API catalog with a redirect to the same path over http, where each would answer: neither hop is taken, and each document reads as missing (llms.txt absent, the optional API catalog n_a), its evidence naming the redirect to http | `llms-txt`, `api-catalog` |
| `run-edge-530` | every probe comes back as a Cloudflare edge error, which is not the target answering | `robots`, `llms-txt` |
| `run-healthy` | a healthy site with a handful of misses across tiers | `content-signals`, `security-txt`, `web-bot-auth`, `noscript-fallback`, `mcp-cors-preflight`, `mcp-cors-actual`, `rate-limit-headers` |
| `run-redirects` | redirect chains: a two-hop public chain is followed, a hop into the metadata range is refused, a cross-origin hop lands on a 404, and a five-hop chain exceeds the cap | `llms-txt`, `robots`, `sitemap`, `security-txt` |
| `run-root-401` | a root that challenges for auth still audits, and the challenge satisfies the auth antecedent | `auth-md`, `oauth-discovery`, `agent-ua-reachable` |
| `run-root-not-html` | the root is JSON, so every HTML-root check and the markdown twin family are `n_a` | `webmcp`, `accept-markdown`, `markdown-vary`, `markdown-frontmatter`, `root-meta-description`, `schema-org-jsonld`, `content-without-js`, `semantic-html`, `noscript-fallback`, `root-link-rel` |
| `run-root-redirects-to-http` | every https request, the root included, redirects to the http root, which would answer: the run ends unreachable after the root request alone, and nothing is requested over http | `agent-ua-reachable`, `content-without-js` |
| `run-site-type-api` | the full site declared as an API: content-only checks are `n_a` at the type filter | `llms-full-txt`, `llms-txt-scoped`, `llms-full-txt-scoped`, `openapi`, `json-errors` |
| `run-site-type-content` | the full site declared as content: API-only checks are `n_a` at the type filter, MCP still applies on discovery | `openapi`, `json-schemas`, `api-catalog`, `json-errors`, `rate-limit-headers`, `llms-full-txt`, `mcp-initialize` |
| `run-unreachable` | nothing answers at the network level | `robots`, `llms-txt` |
| `scoped-llms-absent` | every scoped candidate 404s: absent, which a MAY finalizes as optional-absent | `llms-txt-scoped`, `llms-full-txt-scoped` |
| `scoped-llms-broken` | a present but malformed scoped file is broken | `llms-txt-scoped` |
| `scoped-llms-pass` | a valid scoped llms.txt under a section linked from the root index passes | `llms-txt-scoped`, `llms-full-txt-scoped` |
| `scoped-llms-sitemap-dirs` | section directories come from the sitemap too, and a private-IP href is never enumerated | `llms-txt-scoped` |
| `webmcp-mention-only` | prose naming the Model Context Protocol is not WebMCP exposure | `webmcp` |
| `webmcp-model-context` | an inline navigator.modelContext registration passes | `webmcp` |
| `webmcp-script-asset` | a `webmcp` script asset in the root HTML passes and names the marker | `webmcp` |
