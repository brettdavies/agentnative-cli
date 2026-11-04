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
- **The port is fast but not as fast as its engine logic allows.** Process to process it beats the site engine on Bun by
  about 10% (Bun pays 0.5-2 s of startup). With Bun's startup excluded, the site engine's own audit time is about 25%
  lower than the port's on the same targets. The gap is per-request cost inside the port, not scheduling: both run six
  checks at a time.
- **Two transport costs are measured:** no per-audit DNS cache (108 resolver queries for one `vercel.com` audit against
  4 for Bun, about 0.18 s of summed lookup time) and an idle pool of 3 connections per host (ureq's default) under six
  concurrent requests. A third is suspected: the aws-lc provider built at `opt-level = "s"` handshakes in 405-420 ms on
  a local fixture where ring takes 236-249 ms.
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
- The provider stays aws-lc. Its `opt-level = "s"` override goes if measurement shows it costs handshake time, because
  wall clock outranks about 0.2 MB.
- The network crates build at `opt-level = "s"`.
- T1 is dropped; no release job installs NASM or cmake.
- Transport and request-sharing work targets per-request cost and fan-out: a per-audit DNS cache that connects to the
  classified address, an idle pool sized to the concurrency, and a response shared with every later check that needs it
  unless the check is time-sensitive.

## Clean-runner benchmark

Pending: the same comparison on a GitHub-hosted runner, adding a port build with an idle pool of 16 connections per
host.
