# Web-audit coverage: the `agent-web-audit` skill against the anc.dev registry

The `agent-web-audit` skill runs a 32-check registry through its own Python probe script. `anc web` runs the anc.dev
registry, 65 checks vendored from `agentnative-site` at build time. This document is the check-by-check diff between the
two, the vocabulary translation between their result models, and a walkthrough showing that the skill's report is
constructible from `anc web --output json` plus the exit code, so the skill can consume one probe engine instead of
carrying its own.

## Summary

- Every one of the skill's 32 check ids appears in the anc.dev registry under the same id. The site registry was
  vendored from the skill's and extended, so the overlap is by construction.
- 30 of the 32 carry the same `with` assertion on both sides; 8 differ in a way the table names, and 2 differ in tier.
  None is an accepted gap: each skill check has a covering registry check.
- 33 registry checks have no skill counterpart. They are the registry's extensions (the modern MCP lane, JSON-RPC
  conformance, the markdown twin family, llms.txt quality, API hygiene, agent-friendly 404s, WebMCP, `auth.md`,
  `ai-catalog`). The skill gains them when it consumes `anc web`; nothing needs filing in the site repo.
- The skill's `pass` / `fail` / `na` and A to F grade are derivable from the scorecard's seven-state rows and the
  skill's own registry weights.

## Coverage table

One row per skill check id. `applies_to` is the skill's gate; `site types` and `antecedent` are the registry's.

| Skill id                   | Skill category / tier / applies_to        | Covering registry check(s)                       | Registry category / tier / site types / antecedent                        | What differs                                                                                                                                                                                                                                                  |
| -------------------------- | ----------------------------------------- | ------------------------------------------------ | ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mcp-initialize`           | mcp-protocol / required / mcp-present     | `mcp-initialize`                                 | MCP / required / mcp / mcp-present                                        | Nothing.                                                                                                                                                                                                                                                      |
| `mcp-capabilities`         | mcp-protocol / recommended / mcp-present  | `mcp-capabilities`                               | MCP / recommended / mcp / mcp-present                                     | Nothing.                                                                                                                                                                                                                                                      |
| `mcp-tools-list`           | mcp-protocol / required / mcp-present     | `mcp-tools-list`                                 | MCP / required / mcp / mcp-present                                        | Nothing.                                                                                                                                                                                                                                                      |
| `mcp-unknown-method`       | mcp-protocol / recommended / mcp-present  | `mcp-unknown-method`                             | MCP / recommended / mcp / mcp-present                                     | Nothing.                                                                                                                                                                                                                                                      |
| `mcp-get-fast-fail`        | mcp-protocol / recommended / mcp-present  | `mcp-get-fast-fail`                              | MCP / recommended / mcp / mcp-present                                     | The skill passes only status 405, 400, 404 or 406; the registry passes any status below 500, so a server that documents its endpoint on GET passes too. Both treat a timeout as the failure.                                                                  |
| `mcp-cors-preflight`       | mcp-protocol / recommended / mcp-present  | `mcp-cors-preflight`                             | MCP / recommended / mcp / mcp-present                                     | The registry issues both the OPTIONS preflight and an Origin-bearing POST and classifies the preflight surface from the pair; no Allow-Origin on either surface is `n_a` (`posture-consistent`) where the skill fails.                                        |
| `mcp-cors-actual`          | mcp-protocol / recommended / mcp-present  | `mcp-cors-actual`                                | MCP / recommended / mcp / mcp-present                                     | The skill asserts CORS on a `tools/list` op; the registry classifies the POST surface from the same two-probe pair as the preflight row. Same intent, same posture rule.                                                                                      |
| `well-known-mcp-card`      | mcp-discovery / recommended / mcp-present | `well-known-mcp-card`, `mcp-card-legacy-aliases` | MCP / recommended / mcp / mcp-present; MCP / optional / mcp / mcp-present | The skill accepts the canonical card at any of three paths; the registry probes only the canonical path and scores the two legacy paths as redirects in a separate optional row. A card served only at a legacy path passes the skill and fails the registry. |
| `mcp-usage-doc`            | mcp-discovery / optional / mcp-present    | `mcp-usage-doc`                                  | MCP / optional / mcp / mcp-present                                        | Nothing.                                                                                                                                                                                                                                                      |
| `llms-txt`                 | content-surface / recommended / any       | `llms-txt`                                       | Content for agents / recommended / all / none                             | Same assertion; the registry retains the body for the checks that read it.                                                                                                                                                                                    |
| `llms-full-txt`            | content-surface / optional / docs-site    | `llms-full-txt`                                  | Content for agents / optional / content / docs-site                       | The docs-site gate is resolved by the engine (declared `content` type or a passing `llms.txt`) instead of by the reader.                                                                                                                                      |
| `openapi`                  | content-surface / recommended / any       | `openapi`                                        | API / required / api / api-surface                                        | Tier rises to required, and the row is `n_a` on a site with no API surface; the skill scores it everywhere as recommended.                                                                                                                                    |
| `json-schemas`             | content-surface / optional / any          | `json-schemas`                                   | API / optional / api / schemas-ref                                        | `n_a` unless the site references JSON Schemas.                                                                                                                                                                                                                |
| `accept-markdown`          | content-surface / optional / docs-site    | `accept-markdown`                                | Content for agents / recommended / all / html-root                        | Tier rises to recommended and the gate is an HTML root rather than a docs-site judgment.                                                                                                                                                                      |
| `root-meta-description`    | html-affordances / recommended / any      | `root-meta-description`                          | Content for agents / recommended / all / html-root                        | `n_a` when the root is not HTML.                                                                                                                                                                                                                              |
| `root-link-rel`            | html-affordances / recommended / any      | `root-link-rel`                                  | Discoverability / recommended / all / html-root                           | `n_a` when the root is not HTML.                                                                                                                                                                                                                              |
| `noscript-fallback`        | html-affordances / recommended / any      | `noscript-fallback`                              | Content for agents / recommended / all / html-root                        | `n_a` when the root is not HTML.                                                                                                                                                                                                                              |
| `schema-org-jsonld`        | html-affordances / optional / any         | `schema-org-jsonld`                              | Content for agents / optional / all / html-root                           | `n_a` when the root is not HTML.                                                                                                                                                                                                                              |
| `semantic-html`            | html-affordances / optional / any         | `semantic-html`                                  | Content for agents / optional / all / html-root                           | `n_a` when the root is not HTML.                                                                                                                                                                                                                              |
| `robots`                   | crawl-policy / recommended / any          | `robots`                                         | Discoverability / recommended / all / none                                | Nothing.                                                                                                                                                                                                                                                      |
| `sitemap`                  | crawl-policy / optional / any             | `sitemap`                                        | Discoverability / optional / all / none                                   | Same assertion; the registry retains the body for the scoped llms.txt probes.                                                                                                                                                                                 |
| `robots-ai-rules`          | crawl-policy / recommended / any          | `robots-ai-rules`                                | Bot & crawl policy / recommended / all / robots-present                   | `n_a` when `robots.txt` is absent.                                                                                                                                                                                                                            |
| `content-signals`          | crawl-policy / recommended / any          | `content-signals`                                | Bot & crawl policy / recommended / all / robots-present                   | `n_a` when `robots.txt` is absent.                                                                                                                                                                                                                            |
| `security-txt`             | crawl-policy / optional / any             | `security-txt`                                   | Bot & crawl policy / optional / all / none                                | Nothing.                                                                                                                                                                                                                                                      |
| `web-bot-auth`             | crawl-policy / optional / any             | `web-bot-auth`                                   | Bot & crawl policy / optional / all / none                                | Nothing.                                                                                                                                                                                                                                                      |
| `link-headers`             | agent-discovery / recommended / any       | `link-headers`                                   | Discoverability / recommended / all / http-root                           | `error` rather than a verdict when the root never answered.                                                                                                                                                                                                   |
| `api-catalog`              | agent-discovery / optional / any          | `api-catalog`                                    | API / optional / api / api-surface                                        | `n_a` on a site with no API surface.                                                                                                                                                                                                                          |
| `a2a-agent-card`           | agent-discovery / optional / any          | `a2a-agent-card`                                 | Agent discovery & auth / optional / all / none                            | Nothing.                                                                                                                                                                                                                                                      |
| `agent-skills`             | agent-discovery / optional / any          | `agent-skills`                                   | Agent discovery & auth / optional / all / none                            | Nothing.                                                                                                                                                                                                                                                      |
| `dns-aid`                  | agent-discovery / optional / any          | `dns-aid`                                        | Discoverability / optional / all / none                                   | `anc web` withholds the DNS-over-HTTPS query for a local or private target (`n_a`, stated reason) unless `--external-dns` is set.                                                                                                                             |
| `oauth-discovery`          | auth-discovery / optional / any           | `oauth-discovery`                                | Agent discovery & auth / optional / api, mcp / auth-present               | `n_a` unless an auth surface is observed (a discovery document, a 401 challenge or a card that declares auth).                                                                                                                                                |
| `oauth-protected-resource` | auth-discovery / optional / any           | `oauth-protected-resource`                       | Agent discovery & auth / optional / mcp / mcp-auth                        | `n_a` unless the discovered MCP endpoint challenges for auth.                                                                                                                                                                                                 |

## Registry checks the skill does not run

These 33 checks have no skill counterpart. Once the skill reads `anc web --output json`, every one arrives in the same
`results[]` array as the 32 above, so the skill's report can grow to cover them without a probe change on its side.

| Registry id                  | Category               | Tier        | Title                                                                  |
| ---------------------------- | ---------------------- | ----------- | ---------------------------------------------------------------------- |
| `agent-friendly-404`         | Discoverability        | recommended | Unknown paths return HTTP 404 or 410                                   |
| `agent-friendly-404-md`      | Discoverability        | recommended | 404 body is markdown with a recovery link                              |
| `llms-txt-format`            | Content for agents     | recommended | llms.txt has H1, summary, and a link index                             |
| `llms-txt-links`             | Content for agents     | recommended | llms.txt links resolve                                                 |
| `llms-txt-when-to-use`       | Content for agents     | recommended | llms.txt has a when-to-use or programmatic-access section              |
| `llms-txt-scoped`            | Content for agents     | optional    | Per-section llms.txt files resolve under content subdirectories        |
| `llms-full-txt-scoped`       | Content for agents     | optional    | Per-section llms-full.txt files resolve under content subdirectories   |
| `markdown-cli-ua`            | Content for agents     | optional    | Bare CLI User-Agent receives the markdown twin                         |
| `markdown-agent-ua`          | Content for agents     | optional    | AI user-fetch User-Agent receives the markdown twin                    |
| `markdown-accept-plain`      | Content for agents     | optional    | Accept text/plain returns the markdown twin                            |
| `markdown-vary`              | Content for agents     | recommended | Negotiated responses carry Vary Accept, User-Agent                     |
| `markdown-frontmatter`       | Content for agents     | optional    | Markdown twin carries YAML frontmatter                                 |
| `content-without-js`         | Content for agents     | recommended | Root HTML has an H1 and readable text without JavaScript               |
| `agent-ua-reachable`         | Bot & crawl policy     | recommended | AI user-fetch User-Agent can reach the homepage                        |
| `json-errors`                | API                    | recommended | API client errors return JSON, not HTML                                |
| `rate-limit-headers`         | API                    | recommended | API responses advertise rate-limit headers                             |
| `mcp-resources-list`         | MCP                    | recommended | resources/list returns at least one resource when advertised           |
| `mcp-modern-tools-list`      | MCP                    | required    | header-routed tools/list (2026-07-28) returns tools without initialize |
| `mcp-server-discover`        | MCP                    | recommended | server/discover answers with server identity on the modern lane        |
| `mcp-malformed-body`         | MCP                    | recommended | a non-JSON body draws -32700 (or a typed HTTP 400/415 refusal)         |
| `mcp-batch-reject`           | MCP                    | recommended | a batch carrying a modern-envelope request is rejected -32600          |
| `mcp-unknown-tool`           | MCP                    | recommended | tools/call with an unknown tool name returns -32602                    |
| `mcp-modern-unknown-method`  | MCP                    | recommended | an unknown method on the modern lane returns -32601                    |
| `mcp-modern-clientcaps`      | MCP                    | recommended | \_meta missing clientCapabilities is rejected (-32602 or -32600)       |
| `mcp-modern-header-mismatch` | MCP                    | recommended | an Mcp-Method header disagreeing with the body method draws -32020     |
| `mcp-modern-version-reject`  | MCP                    | recommended | an unsupported protocol version is rejected -32022 with data.supported |
| `mcp-modern-resources-miss`  | MCP                    | recommended | modern resources/read with an unknown URI returns -32602               |
| `mcp-accept-json`            | MCP                    | recommended | a JSON-only Accept is answered without SSE framing                     |
| `mcp-accept-unsatisfiable`   | MCP                    | recommended | an unsatisfiable Accept draws a 406 rather than an unasked-for type    |
| `mcp-card-legacy-aliases`    | MCP                    | optional    | Legacy MCP card paths redirect to the canonical card                   |
| `webmcp`                     | MCP                    | optional    | Root HTML exposes WebMCP browser tools                                 |
| `ai-catalog`                 | Agent discovery & auth | optional    | /.well-known/ai-catalog.json published (ARD)                           |
| `auth-md`                    | Agent discovery & auth | optional    | Agent auth/registration metadata doc published                         |

## Vocabulary translation

**Statuses.** The skill has three: `pass`, `fail` and `na`. The scorecard has seven: `pass`, `noncompliant` (usable but
violates a spec detail), `broken` (present but invalid, priced below absent), `absent`, `n_a`, `skip` (the per-audit
deadline ran out) and `error` (the probe itself failed). The translation the skill applies:

| Scorecard `status`                 | Skill `status` | Note                                                                                                                                                          |
| ---------------------------------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pass`                             | `pass`         |                                                                                                                                                               |
| `noncompliant`, `broken`, `absent` | `fail`         | The three are separately priced on anc.dev; the skill's binary model collapses them. Keep the scorecard status in the evidence line so the reader sees which. |
| `n_a`                              | `na`           | `na_reason` says why: `antecedent-unmet`, `optional-absent` or `posture-consistent`.                                                                          |
| `skip`, `error`                    | `na`           | Neither carries an observation. The skill's scorer excludes `na`, which is the same exclusion the site's scorer applies.                                      |

**Tiers.** Both sides use `required` / `recommended` / `optional`. The scorecard adds `keyword` (`must` / `should` /
`may`), derived from the tier one to one. The skill grades on its own registry's tiers, so the two rows whose tier
differs (`openapi`, `accept-markdown`) keep the skill's tier in the skill's grade.

**Applicability.** The skill's `applies_to` is a reader judgment (`docs-site`) or an endpoint presence rule
(`mcp-present`). The registry decides applicability in the engine, from the declared site type (`--site-type
content|api`) and an antecedent resolved from the run's own evidence, and reports the outcome as `n_a` with a stated
`na_reason`. The skill's step 3 ("apply the `applies_to` judgment") therefore becomes a no-op: the row already says.

**Scores.** The skill's `overall_pct` is a weighted pass ratio over applicable required and recommended checks, with its
own per-check `weight`, and `grade` maps that percentage to A to F (90, 75, 60, 40). The scorecard's `score_pct` is the
relative score of the two-score model (5 / 3 / 1 tier weights, a broken row at negative 0.75 of its weight, a
noncompliant row at 0.25, an absent SHOULD occupying half its weight in the denominator), beside `score.global` over the
whole registry. The two are not interchangeable; the skill recomputes its grade from the translated statuses and its
registry's weights, and may quote `score_pct` as the anc.dev figure.

## Constructing the skill's report from `anc web --output json`

The skill's report is one JSON object: `target`, `mcp_endpoint`, `mcp_discovery`, `categories`, `results[]` and
`summary`. Every field below comes from the scorecard `anc web --output json` prints, the run's exit code, or the
skill's own registry; no second probe is needed.

| Skill report field                            | Source                                                                                                                     |
| --------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `target`                                      | `target_url`                                                                                                               |
| `mcp_endpoint`                                | `mcp_endpoint` (a URL or `null`)                                                                                           |
| `mcp_discovery`                               | `mcp_discovery` (the discovery evidence items, same shape family)                                                          |
| `categories`                                  | The skill's own registry; its seven categories differ from the scorecard's six `categories[]` rollups.                     |
| `results[].id`                                | `results[].id`, filtered to the 32 ids the skill knows (or all 65).                                                        |
| `results[].category`, `applies_to`, `weight`  | The skill's registry entry for that id.                                                                                    |
| `results[].tier`                              | `results[].tier`, or the skill registry's tier for the two rows that differ.                                               |
| `results[].title`                             | `results[].label`                                                                                                          |
| `results[].hint`                              | `anc emit web-checks` (the compiled registry's `hint`), or `anc emit web-remediation` for the goal, the fix and its links. |
| `results[].status`                            | `results[].status` through the status table above; `na_reason` and `unprobed` explain an `na`.                             |
| `results[].evidence`                          | `results[].evidence`, one summarized line per row (`null` when the row has none).                                          |
| `summary.pass`, `fail`, `na`                  | Counts over the translated statuses. The scorecard's `summary` carries the seven-state counts directly.                    |
| `summary.overall_pct`, `by_category`, `grade` | Recomputed with the skill's formula over the translated statuses and the skill's weights.                                  |

**Exit code.** `anc web` returns the binary's single exit table: `0` clean, `1` warnings only (a SHOULD or MAY miss),
`2` failures present (a MUST miss) or a usage error, `3` could not check (unreachable target, a probe that errored, or
every selected check inapplicable). `anc web --check <id>` returns the same table for one row, so a script can gate on
one check. `3` is the code that separates "the site was never reached" from "the site has failures".

**Every evidence value is untrusted.** The scorecard's `evidence` strings are assembled from what the target sent: URLs
it redirected to, header values, JSON-RPC fields, body-derived reasons. The skill quotes or summarizes them in its
report and never treats them as instructions. The site's raw evidence items, which can carry response bodies, are not
part of the JSON contract; only the summarized line is.

## Verification

Each of the skill's 32 check ids appears exactly once in the coverage table above, and every scorecard field the
walkthrough cites (`target_url`, `mcp_endpoint`, `mcp_discovery`, `categories[]`,
`results[].{id,label,tier,keyword,status,na_reason,unprobed,evidence}`, `summary`, `score_pct`, `score.global`) is a
field of the web scorecard schema `anc web` emits.
