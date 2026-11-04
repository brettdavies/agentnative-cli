---
title: "Spike: anc web wall clock, an embedded site engine versus the Rust port, and binary size"
plan: docs/plans/2026-09-01-1611-feat-local-web-audit-plan.md
status: done
completed: 2026-11-03
date: 2026-11-03
---

# Spike: anc web wall clock, an embedded site engine versus the Rust port, and binary size

## Questions

1. How fast is `anc web` against real targets, compared with the site engine it ports?
2. Would replacing the hand-ported Rust engine with the site's own TypeScript engine, bundled by the site and run
   in-process by an embedded JavaScript runtime, make vendoring painless without costing speed (KD2)?
3. Where do the `web` feature's 3.8 MB go, and which of them can go without costing audit speed?

The owner's bar for replacing the port: zero hand-ports, byte parity on the corpus, a user-visible wall clock (start to
finish, startup included) no slower than the port, a release binary under 20 MiB on every release target, and nothing
for the user to install. Comparisons are like for like: a transport fix applies to both designs.

## TL;DR

- **Keep the port.** The embedded engine (QuickJS through `rquickjs`, Rust answering each single-hop fetch) is slower
  than the port on every target measured: 1.35x on the sum of medians after its two largest costs were removed, 2.28x on
  `developers.cloudflare.com`, with 2-4x the CPU per audit. It fails the speed bar, so the remaining criteria were not
  carried to completion.
- **The port trails the site engine on Bun by 12-20% of wall clock** on a clean GitHub runner, with identical request
  sets and scores (three runs, "Clean-runner benchmark" below). The development host's load had inflated Bun's startup
  and hidden this. The gap is per-request time to response headers, not scheduling: both run six checks at a time.
- **Brotli breaks connection reuse.** With ureq's `brotli` feature on, a brotli-encoded body never returns its
  connection to the pool, so an anc.dev audit takes 44 TLS handshakes for 63 requests against 12 without `brotli`. The
  smaller costs: an idle pool of 3 connections per host (ureq's default) under six concurrent requests, which alone
  closes about a third of the gap, and no per-audit DNS cache (108 resolver queries for one `vercel.com` audit against
  4 for Bun, about 0.18 s of summed lookup time).
- **Sharing is not where the time is.** The site engine's request memo already answers repeats (one re-send in 20
  audits). The wall clock goes to phases run in series, one-at-a-time request chains inside checks, the wave barrier,
  and full-body reads behind status-only expectations ("Where an audit's wall clock goes" below).
- **Release toolchain:** NASM and cmake are not needed on any release target with today's aws-lc build, so plan task T1
  is unnecessary.
- **Size:** the network crates at `opt-level = "s"` save 217,536 bytes with no measured slowdown. ring would save
  630,040 bytes but fails the owner's health rule (no release since 0.17.14, March 2025), so the provider stays aws-lc.

## Method

- Release binaries built from `dev` (`2c0efd8`, web pin `d5772289`) with `--features web`; the site engine at site `dev`
  `031ec6e`, run under Bun through `scripts/web-audit/audit.ts` (process wall clock) and through `runWebAudit` timed
  in-process (engine-only time).
- Ten public targets from the web board seed: anc.dev, developers.cloudflare.com, supabase.com, docs.stripe.com,
  vercel.com, modelcontextprotocol.io, mcp.linear.app, github.com, en.wikipedia.org, huggingface.co.
- Engines alternate run by run, three rounds. The development host ran at load average 40-120 on 16 cores throughout,
  from unrelated builds, so absolute times carry wide error bars and only alternating comparisons are reported. A clean
  re-run on a GitHub-hosted runner is recorded under "Clean-runner benchmark" below.

## Findings

### 1. The port against the site engine

Process wall clock, median of three rounds per target, seconds:

| Target                    |  Port | Bun process | Port / Bun | Port peak RSS (MB) | Bun peak RSS (MB) |
| ------------------------- | ----: | ----------: | ---------: | -----------------: | ----------------: |
| anc.dev                   |  1.71 |        1.48 |       1.16 |                 24 |                89 |
| developers.cloudflare.com |  2.14 |        3.95 |       0.54 |                 69 |               300 |
| supabase.com              |  3.28 |        3.87 |       0.85 |                 57 |               131 |
| docs.stripe.com           |  3.40 |        4.16 |       0.82 |                 33 |               102 |
| vercel.com                |  4.12 |        2.88 |       1.43 |                 51 |               114 |
| modelcontextprotocol.io   |  7.80 |        8.16 |       0.96 |                 27 |                95 |
| mcp.linear.app            |  1.41 |        2.34 |       0.60 |                 30 |               100 |
| github.com                |  3.36 |        3.31 |       1.02 |                 30 |               100 |
| en.wikipedia.org          |  0.82 |        0.79 |       1.04 |                 20 |                88 |
| huggingface.co            |  1.31 |        1.67 |       0.78 |                 38 |                95 |
| **Sum of medians**        | 29.35 |       32.61 |       0.90 |                    |                   |

Engine-only, second run under heavier load (sum of medians): port 30.36 s, site engine in-process 22.83 s. The site
engine's audit is shorter on 8 of 10 targets.

Parity on real targets: 28 of 30 target-round pairs agree on every row status and on the score. The two that differ are
one transient row error on each side (`llms-txt-links` on the port, `mcp-modern-clientcaps` on Bun).

### 2. Where the port's time goes

- `strace` of one `vercel.com` audit: 30 TCP connects to port 443 for the port and 27 for Bun (Bun's count includes
  Happy Eyeballs parallel dials), but 108 DNS queries and 218 address-sorting probes for the port against 4 queries for
  Bun. `locality::SystemResolver` resolves every hop for classification and ureq resolves it again at connect. The
  lookups sum to about 0.18 s of a 4.7 s audit, so DNS is a minority of the gap, and the double resolve also leaves a
  window between classification and connect.
- ureq keeps at most 3 idle connections per host (`max_idle_connections_per_host`, default 3 in ureq 3.5) while the
  engine runs six requests at a time, so up to half the connections close after each use and the next request to that
  host pays a full TLS handshake. A local alternating A/B with 16 per host was inconclusive under the host's load; the
  clean-runner benchmark measures it.
- A status-only read drops the body and with it the connection; a capped read leaves the rest of the body unread, which
  also prevents reuse.
- TLS handshake cost on a local fixture (HTTP/1.0, so every request handshakes, 8-10 alternating rounds): ring 236-249
  ms median, aws-lc built at `opt-level = "s"` (the setting #215 chose for size) 405-420 ms.

### 3. The embedded site engine

- **Bundle.** `bun build` turns the site engine into one ESM file, 184 KB raw and 100 KB minified, using no Node or
  Workers API. The site's registry normalizer imports `node:fs`, so the registry would ship pre-normalized as JSON.
- **Host.** `rquickjs` 0.14 on a single-thread runtime, ureq 3.5 for HTTPS with redirects off, and hand-written Web APIs
  the engine needs: `URL`, `Headers`, `Response` with a streamed body, `AbortController`, `DOMException`,
  `TextEncoder`/`TextDecoder`, timers.
- **Speed.** Median wall clock summed over four targets, alternating with the port:

| Host variant                       | Port (s) | Embedded (s) | Embedded / port |
| ---------------------------------- | -------: | -----------: | --------------: |
| As built (`whatwg-url` polyfill)   |    15.81 |        41.96 |            2.65 |
| Native `URL` binding over `url`    |    15.53 |        21.40 |            1.38 |
| Native `URL` and 1 MiB body chunks |    15.27 |        20.69 |            1.35 |

  Per target, best variant: developers.cloudflare.com 2.70 s against 6.15 s (2.28x, RSS 70 MB against 290 MB),
  supabase.com 3.55 against 4.27 (1.20x), modelcontextprotocol.io 5.94 against 6.26 (1.05x), vercel.com 3.08 against
  4.01 (1.30x). CPU per audit: 0.23-0.63 s embedded against 0.12-0.20 s for the port.

- **Where the JavaScript time goes.** A recorded `vercel.com` run (44 exchanges, a 9,316-entry sitemap) replayed
  offline: 11.2 s of CPU in QuickJS against 0.19 s in Node, byte-identical output. About 29,000 `new URL()` calls
  through the `whatwg-url` polyfill's Unicode regexes dominate; a native `URL` binding brings it to 2.88 s. What remains
  is `JSON.parse` (0.68 s) and the registry's body regexes over a 592 KB page at 80-180 ms per pattern.
- **Semantics.** The site engine reads `llms-full.txt` in full (69.7 MB on developers.cloudflare.com); the port caps a
  full read at 16 MiB (`AUDIT_ROOT_MAX_BODY_BYTES`). An embedded engine would need that cap added host-side, a hand-kept
  divergence again. Timers fire late while the JavaScript thread is busy; one embedded run took 51 s against the
  engine's 25 s audit deadline.
- **Other criteria, as far as they went.** Zero hand-ports: met in principle (one generated bundle plus the normalized
  registry). Parity: `run-healthy` and the replayed `vercel.com` run are byte-identical; a live anc.dev run matches the
  port on all 68 rows. Size: the spike host alone is 3.92 MB stripped; the release targets were not built. Nothing to
  install: met. The hand-written Web APIs would become part of the parity surface, and every CLI-only behavior (local
  targets the site engine refuses, the DNS gate, the OS trust store and `--ca-cert`, the bearer token, the body cap)
  would sit outside the vendored engine.

### 4. Binary size

Stripped x86_64 Linux release builds: 5,569,992 bytes without `web`, 9,381,616 with it (+3.8 MB). Section deltas
(unstripped, web minus no-web): `.text` +2.71 MB, `.rodata` +0.58 MB, `.eh_frame` and its header +0.23 MB, `.rela.dyn`
+0.17 MB, `.data.rel.ro` +0.12 MB. `cargo bloat` attributes the added `.text` to the web engine itself (`agentnative`
grows from 590 KiB to 1.2 MiB), `std` (+250 KiB), aws-lc (397 KiB plus about 218 KiB of unattributed assembly), rustls
(348 KiB), ureq (137 KiB), `url` (50 KiB), webpki (45 KiB), `http` (34 KiB), brotli (30 KiB) and idna (21 KiB). The
regex crates are already in the no-web build.

| Lever                                    | Bytes saved (web build) | Cost                                                   |
| ---------------------------------------- | ----------------------: | ------------------------------------------------------ |
| ring instead of aws-lc                   |                 630,040 | No post-quantum key exchange; fails the release rule   |
| Network crates at `opt-level = "s"`      |                 217,536 | None measured                                          |
| C unwind tables dropped through `CFLAGS` |                  91,456 | Build-environment flags (see solutions note on CFLAGS) |
| `-C force-unwind-tables=no`              |                       0 | Byte-identical output                                  |
| `agentnative` at `opt-level = "s"`       |                -256,864 | Larger, and slows CLI audits                           |
| Network crates at `opt-level = "z"`      |        worse than `"s"` |                                                        |
| `idna_adapter` 1.1 (unicode-rs backend)  |                -132,672 | Larger                                                 |

ring's health against the owner's rule (actively maintained, recent releases, no open advisories, first-class in
rustls): maintained (over 100 commits between March and July 2026; RUSTSEC-2025-0007 withdrawn), no open advisories
against 0.17.14, first-class in rustls (the 0.24 line moves it to the first-party `rustls-ring` crate), but no release
since 0.17.14 on 2025-03-11, a single crates.io owner, and no maintained fork or continuation. It fails "recent
releases", so the provider stays aws-lc.

Release toolchain: rustls's `aws_lc_rs` feature enables `aws-lc-rs/prebuilt-nasm`, and aws-lc-sys 0.46 builds with its
`cc` builder, not cmake, wherever prebuilt bindings ship (x86_64 and aarch64 Linux gnu and musl, x86_64 and aarch64
macOS, x86_64 Windows gnu and msvc). A probe crate built and linked on `x86_64-unknown-linux-musl` and
`x86_64-pc-windows-gnu` with `nasm` and `cmake` replaced by stubs that exit 127. MSVC and macOS were not checkable
locally; the CI Windows check and the release-matrix check cover them on the PR that turns `web` on by default.

## Decisions

- KD2 stands: the Rust port stays, and painless vendoring comes from the contract directory, the bump workflow and the
  release gate rather than from sharing engine code.
- The provider stays aws-lc. Its `opt-level = "s"` override stays unless a clean-runner benchmark of real targets
  shows it costs wall clock: removing it adds 1,461,704 bytes for a handshake measured 12% faster on a loaded host.
- The network crates build at `opt-level = "s"`, and ureq's `brotli` feature goes (a speed fix as well as 188,096
  bytes). The C unwind tables stay: dropping them saves 91,456 bytes through build-environment flags that have broken
  aws-lc-sys builds before.
- T1 is dropped; no release job installs NASM or cmake.
- Port-only transport work: drop `brotli`, size the idle pool to the concurrency, add a per-audit DNS cache that
  connects to the classified address, and run one worker pool per audit.
- Engine scheduling work lands in the site engine first and the port mirrors it: ranks 1-4 of the wall-clock table,
  which send no extra request to a healthy target. Ranks 5-7 send extra requests to the target and are not taken.

## Clean-runner benchmark

The development host's load inflated Bun's startup (0.5-2 s there, about 20 ms on a clean runner), which made the port
look faster process to process than it is. Three runs on a GitHub-hosted `ubuntu-24.04` runner, each five interleaved
rounds over the ten targets with the engine order rotated per round. Network conditions vary between runs, so compare
engines only within a run. Sum of per-target median wall clock, seconds:

| Run | Site engine on Bun | Port with idle pool 16 per host, 64 total | Port as on `dev` |
| --- | -----------------: | ----------------------------------------: | ---------------: |
| 1   |              20.10 |                                           |            24.06 |
| 2   |              22.72 |                                     23.36 |            25.45 |
| 3   |              14.52 |                                     13.88 |            16.21 |

Scores agree across engines on every target in every run, apart from rounds where one engine saw a transient row error.

Request traces from run 3 (time to response headers per network request, two rounds per target):

| Target                  | Requests | Bun median s | Pool median s | Port median s |
| ----------------------- | -------: | -----------: | ------------: | ------------: |
| vercel.com              |       44 |        0.049 |         0.074 |         0.082 |
| supabase.com            |       77 |        0.061 |         0.080 |         0.080 |
| docs.stripe.com         |       52 |        0.004 |         0.007 |         0.013 |
| modelcontextprotocol.io |       82 |        0.116 |         0.126 |         0.132 |

The engines send identical request sets, so the port's gap is per-request time to headers. Counting TLS handshakes per
audit on the development host (load-independent) finds the cause: with ureq's `brotli` feature on, a brotli-encoded body
does not return its connection to ureq's pool, so later requests to the host handshake again. anc.dev takes 44
handshakes for 63 requests with `brotli` and 12 without; vercel.com 30 against 15; supabase.com 47 against 21. Raising
the idle pool barely moves those counts while `brotli` is on. The inferred mechanism is that the brotli decoder stops
at the end of its stream, so ureq never sees the end-of-body read that returns the connection, while gzip's
multi-member decoder reads on. Dropping `brotli` is therefore a speed fix as well as the 188,096-byte size cut.

## Where an audit's wall clock goes

The site engine, instrumented under Bun (phase marks, the calling check, and a line per logical and network request),
over the ten targets twice: 1,543 logical requests, 1,355 sent. A schedule simulation driven by the measured request
durations reproduces the measured totals (46.4 s simulated against 46.8 s measured over 20 audits).

- **Sharing already works.** The per-audit request memo answers repeats and lets a status-only caller return at the
  headers while it keeps reading up to 1 MiB for later callers. One request in 20 audits went out twice, because its
  body exceeded the memo's per-body cap. Reading the largest cap on first fetch would save about nothing. The other
  same-URL repeats differ in `Accept`, `User-Agent` or body, which is what those checks measure, or are time-sensitive
  (the MCP session and wire probes, the CORS pair, the GET fast-fail).
- **The time is in structure.** 30% of the summed wall clock is root, discovery documents and endpoint of record run
  one after another; multi-request checks run their requests one at a time (`llms-txt-links` up to nine link probes,
  `scoped-llms` up to eight directories, `path_any` up to three candidates); every wave-2 check waits for wave 1's
  slowest check (on developers.cloudflare.com, a 1.94 s full read of a 69.9 MB `llms-full-txt`); and status-only
  expectations read whole bodies (3.45 s over the 20 audits).

| Rank | Change                                                                | Saved per audit (base 2,320 ms) | Request set on the corpus                 |
| ---- | --------------------------------------------------------------------- | ------------------------------: | ----------------------------------------- |
| 1    | Start each wave-2 check when its own sources settle (no wave barrier) |                  245 ms (10.5%) | Unchanged                                 |
| 2    | Fan out `llms-txt-links` probes under the audit's concurrency cap     |                          +96 ms | Unchanged                                 |
| 3    | Root fetch and discovery documents concurrently                       |                         +163 ms | One unreachable scenario gains https GETs |
| 4    | A status-only expectation reads headers only                          |         about 173 ms (measured) | Unchanged; some evidence keys change      |
| 5    | Parallel `scoped-llms` candidates                                     |                          +88 ms | Superset: extra requests to targets       |
| 6    | Endpoint-free wave-1 checks at t=0                                    |                         +143 ms | Superset on unreachable targets           |
| 7    | Parallel `path_any` candidates                                        |                           +4 ms | Superset                                  |
| 8    | Memo reads the largest cap on first fetch                             |                              ~0 | -                                         |
| 9    | Concurrency cap per host instead of per audit                         |                      negligible | -                                         |

Ranks 1-4 save about 0.68 s of a 2.3 s average audit (about 29%) without sending a target more requests; ranks 5-7
send more requests to the target for a small gain. The port mirrors the same phase and barrier structure, plus three
costs of its own: a fresh thread pool per wave, a thread per memo hop, and the `brotli` reuse failure above.
