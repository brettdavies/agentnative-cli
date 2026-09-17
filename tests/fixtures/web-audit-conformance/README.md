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
- `unmatched`: the response for a request no exchange matches, either a transport failure
  (`{"error": "Name: message"}`) or a full response.
- `allow_unmatched`: when false, generation fails if any request reaches the unmatched policy.
- `exchanges`: ordered rules; the first match wins and a rule may match repeatedly.

Matching: `method` compares case-insensitively; `url` compares after both sides pass through the URL
parser; `headers` is a subset match on lowercase names with exact values; `body_json_method` parses the
request body as JSON and compares its top-level `method`; `body_contains` is a substring match on the raw
body.

Responses carry lowercase header names with single string values and a UTF-8 text body exactly as the engine
reads it. No response carries `content-encoding`: decompression is pinned by transport tests, not by the
corpus. A transport failure is `{"error": "Name: message"}`, the string `ProbeResponse.error` carries at the
seam; a `TimeoutError` is always recorded as `TimeoutError: deadline exceeded`. Redirects are ordinary
exchanges (a 3xx with a `location` header) that the guarded fetch above the seam follows with a new request.

## scorecard.json

The engine's terminal scorecard as `JSON.stringify(scorecard, null, 2)` plus a trailing newline, with one
normalization applied before writing and expected of any engine under comparison: `results` is sorted into
registry order, because wave-2 rows arrive in the order the concurrent probes resolve, which carries no
meaning and which a second engine cannot reproduce. Rows carry the summarized `evidence` line, never the raw
evidence, so no wall-clock value reaches the file. Key order is otherwise the engine's emission order and
every number is an integer. A scenario that ends in
the engine's `unreachable` event writes `{"unreachable": "<reason>"}` instead of a scorecard. The engine runs
under a fixed clock, so no per-audit deadline fires; per-probe timeouts appear only as declared transport
failures.

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
| `alias-redirect-pass` | one legacy card path 301s to the canonical card, which is enough to pass | `mcp-card-legacy-aliases`, `well-known-mcp-card`, `mcp-usage-doc` |
| `alias-unpublished-optional-absent` | no legacy path published at all is optional-absent, never a penalty | `mcp-card-legacy-aliases` |
| `api-hygiene-fallback-path` | with no usable OpenAPI body the probe falls back to the well-known nonsense path | `json-errors`, `rate-limit-headers` |
| `api-hygiene-html-error` | an HTML error body on the API probe is broken and carries no rate-limit header | `json-errors`, `rate-limit-headers` |
| `api-hygiene-json-200` | a 200 JSON body where a client error was expected is absent for json-errors | `json-errors` |
| `api-hygiene-pass` | a documented 4xx GET answered with a JSON error body and rate-limit headers passes both rows | `json-errors`, `rate-limit-headers` |
| `auth-md-absent` | no auth document at either path is absent, which a MAY finalizes as optional-absent | `auth-md` |
| `auth-md-bare-heading` | a document without a content type but opening with a markdown heading still passes | `auth-md` |
| `auth-md-html-broken` | an HTML page at the auth.md path is present but malformed | `auth-md` |
| `auth-md-pass` | a markdown auth document passes once the auth antecedent holds | `auth-md`, `oauth-discovery` |
| `content-non-2xx-broken` | a rich root served with a 5xx is broken, not a pass | `content-without-js` |
| `content-rich-pass` | rich HTML with an H1 and visible text passes without probing the twin | `content-without-js` |
| `content-thin-absent` | thin HTML with no llms.txt is absent | `content-without-js` |
| `content-thin-dead-links-absent` | thin HTML whose llms.txt links do not resolve is absent | `content-without-js`, `llms-txt-links` |
| `content-thin-twin-na` | thin HTML softened by a live llms.txt content link is `n_a` | `content-without-js` |
| `cors-full-pass` | Allow-Origin on both surfaces passes both rows | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-post-only` | the POST carries Allow-Origin but the preflight does not: preflight broken, actual pass | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-post-transport-failure-no-cors` | a bare preflight beside a failed POST is an operational unknown, not a declared posture | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-posture-consistent` | no Allow-Origin on the preflight or the POST is a consistent no-CORS posture: both rows `n_a` | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-preflight-500-with-acao` | Allow-Origin on a failing preflight is misconfigured: preflight broken, actual classifies from its own POST | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-preflight-only` | the preflight declares CORS but the POST omits Allow-Origin: preflight pass, actual broken | `mcp-cors-preflight`, `mcp-cors-actual` |
| `cors-preflight-transport-failure` | a transport failure on the preflight suppresses only the preflight row; the actual row classifies from its POST | `mcp-cors-preflight`, `mcp-cors-actual` |
| `dns-aid-nxdomain` | every name resolves NXDOMAIN: absent, which a MAY finalizes as optional-absent | `dns-aid` |
| `dns-aid-pass` | the first resolver answers Status 0 with a record for the index name | `dns-aid` |
| `dns-aid-resolver-fallback` | the first resolver fails at the resolver level and the second answers | `dns-aid` |
| `dns-aid-resolvers-unreachable` | every resolver fails at the transport level: an operational error, not an absence | `dns-aid` |
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
| `llms-quality-broken-link` | a link that answers 5xx makes the links row broken | `llms-txt-links` |
| `llms-quality-dead-link` | a link that answers 404 makes the links row absent | `llms-txt-links` |
| `llms-quality-h1-only` | an llms.txt with only an H1 misses format, has no links to follow, and has no when-to-use heading | `llms-txt-format`, `llms-txt-links`, `llms-txt-when-to-use` |
| `llms-quality-pass` | an llms.txt with an H1, a summary, resolving links and a when-to-use heading passes the trio | `llms-txt-format`, `llms-txt-links`, `llms-txt-when-to-use` |
| `mcp-capabilities-empty` | an initialize result with empty capabilities passes initialize but is broken on the capabilities row | `mcp-initialize`, `mcp-capabilities` |
| `mcp-card-no-endpoint-field` | a card without an endpoint field passes the card check while discovery falls through to initialize | `well-known-mcp-card`, `mcp-initialize` |
| `mcp-card-off-origin` | a card declaring an off-origin endpoint is recorded and never probed; discovery falls through to initialize | `well-known-mcp-card`, `mcp-initialize` |
| `mcp-dual-stack` | a dual-stack server passes every legacy, modern, conformance and negotiation row | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-garbage-responses` | a discovered endpoint that answers every POST with an HTML 500 is broken on every probed row | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-legacy-lane` | a legacy-only server discovered by initialize: legacy rows pass, modern rows read the lane as absent | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-modern-lane` | a modern-only server discovered by the header-routed fallback: modern rows pass, legacy rows read the lane as absent | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-negotiation-defects` | a stream answered to a JSON-only client is broken and a 200 with an unasked-for type is noncompliant | `mcp-accept-json`, `mcp-accept-unsatisfiable` |
| `mcp-open-stream` | a server that answers initialize on a stream it never closes: the seam records the per-check timeout, exactly as a live run does | `mcp-initialize`, `mcp-capabilities` |
| `mcp-rate-limited` | a -32099 rate-limit refusal is an operational error on every row, never a penalty | `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `mcp-resources-empty-broken` | resources advertised but resources/list returns an empty array is broken | `mcp-resources-list` |
| `mcp-resources-unadvertised` | capabilities that omit resources gate both resources rows to `n_a` | `mcp-resources-list`, `mcp-modern-resources-miss` |
| `mcp-sse-framing` | a legacy tools/list answered as text/event-stream parses the first data line | `mcp-tools-list` |
| `mcp-stateful-session` | a stateful server issues a session on initialize, refuses sessionless conformance probes with -32000, and is scored on the re-ask | `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-tools-list`, `mcp-resources-list` |
| `mcp-typed-http-refusals` | bare HTTP 400/415 refusals with no envelope conform where the row allows them; a bare 404 never does | `mcp-malformed-body`, `mcp-accept-unsatisfiable`, `mcp-batch-reject`, `mcp-unknown-tool` |
| `mcp-wrong-error-codes` | conformance probes refused under the wrong code are noncompliant; a result where a refusal was required is broken | `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-unknown-method`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss` |
| `mcp-www-authenticate` | an endpoint that challenges initialize with 401 and WWW-Authenticate is broken and satisfies the mcp-auth antecedent | `mcp-initialize`, `oauth-protected-resource`, `auth-md` |
| `run-all-pass` | every surface answers as the registry wants it to | `openapi`, `json-schemas`, `api-catalog`, `json-errors`, `rate-limit-headers`, `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail`, `mcp-cors-preflight`, `mcp-cors-actual`, `well-known-mcp-card`, `mcp-card-legacy-aliases`, `mcp-usage-doc`, `webmcp`, `llms-txt`, `llms-txt-format`, `llms-txt-links`, `llms-txt-when-to-use`, `llms-full-txt`, `llms-txt-scoped`, `llms-full-txt-scoped`, `accept-markdown`, `markdown-cli-ua`, `markdown-agent-ua`, `agent-ua-reachable`, `markdown-accept-plain`, `markdown-vary`, `markdown-frontmatter`, `root-meta-description`, `schema-org-jsonld`, `content-without-js`, `semantic-html`, `noscript-fallback`, `robots`, `sitemap`, `agent-friendly-404`, `agent-friendly-404-md`, `link-headers`, `root-link-rel`, `dns-aid`, `robots-ai-rules`, `content-signals`, `web-bot-auth`, `security-txt`, `a2a-agent-card`, `ai-catalog`, `agent-skills`, `oauth-discovery`, `oauth-protected-resource`, `auth-md` |
| `run-antecedent-unmet` | a bare HTML site: no llms.txt, no API surface, no MCP endpoint, so every gated check is `n_a` | `llms-txt-format`, `llms-txt-links`, `llms-txt-when-to-use`, `llms-txt-scoped`, `llms-full-txt-scoped`, `json-schemas`, `api-catalog`, `json-errors`, `rate-limit-headers`, `mcp-cors-preflight`, `mcp-cors-actual`, `well-known-mcp-card`, `mcp-card-legacy-aliases`, `mcp-usage-doc`, `oauth-protected-resource`, `robots-ai-rules`, `content-signals`, `auth-md`, `mcp-initialize`, `mcp-capabilities`, `mcp-tools-list`, `mcp-resources-list`, `mcp-modern-tools-list`, `mcp-server-discover`, `mcp-unknown-method`, `mcp-malformed-body`, `mcp-batch-reject`, `mcp-unknown-tool`, `mcp-modern-unknown-method`, `mcp-modern-clientcaps`, `mcp-modern-header-mismatch`, `mcp-modern-version-reject`, `mcp-modern-resources-miss`, `mcp-accept-json`, `mcp-accept-unsatisfiable`, `mcp-get-fast-fail` |
| `run-body-over-cap` | bodies past the 64 KiB probe cap are truncated: a huge tools/list no longer parses and a huge JSON error body reads as non-JSON | `mcp-tools-list`, `json-errors` |
| `run-edge-530` | every probe comes back as a Cloudflare edge error, which is not the target answering | `robots`, `llms-txt` |
| `run-healthy` | a healthy site with a handful of misses across tiers | `content-signals`, `security-txt`, `web-bot-auth`, `noscript-fallback`, `mcp-cors-preflight`, `mcp-cors-actual`, `rate-limit-headers` |
| `run-redirects` | redirect chains: a two-hop public chain is followed, a hop into the metadata range is refused, a cross-origin hop lands on a 404, and a five-hop chain exceeds the cap | `llms-txt`, `robots`, `sitemap`, `security-txt` |
| `run-root-401` | a root that challenges for auth still audits, and the challenge satisfies the auth antecedent | `auth-md`, `oauth-discovery`, `agent-ua-reachable` |
| `run-root-not-html` | the root is JSON, so every HTML-root check and the markdown twin family are `n_a` | `webmcp`, `accept-markdown`, `markdown-vary`, `markdown-frontmatter`, `root-meta-description`, `schema-org-jsonld`, `content-without-js`, `semantic-html`, `noscript-fallback`, `root-link-rel` |
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
