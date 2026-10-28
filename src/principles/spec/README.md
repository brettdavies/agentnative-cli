# Vendored agentnative-spec

This directory is a **vendored copy** of [`brettdavies/agentnative`](https://github.com/brettdavies/agentnative) — the
canonical specification of agent-native CLI principles. Files here are not edited by hand; they are mirrored from the
latest upstream `v*` tag and consumed by `build.rs` to generate the `REQUIREMENTS` slice at build time. The currently
vendored version is recorded in [`VERSION`](./VERSION).

For the full spec landing page — leaderboard, badge convention, and acknowledgements — see [anc.dev](https://anc.dev) or
the upstream [`README`](https://github.com/brettdavies/agentnative#readme).

## Resync

Run from the repo root:

```sh
scripts/sync-spec.sh                # the latest v* tag
scripts/sync-spec.sh --ref v0.7.0   # a named tag, branch, or commit SHA
```

The script resolves the ref with `gh api` against the repository `SPEC_REMOTE_URL` names
(`https://github.com/brettdavies/agentnative.git` by default) and fetches each file from the GitHub contents endpoint
at that ref. When the API is unreachable, it falls back to a local checkout (`$HOME/dev/agentnative-spec` by default;
override with `SPEC_ROOT`) and reads files with `git show`, so the checkout's working tree is left as it is.

## Layout

| Path                    | Source in `agentnative-spec` | Purpose                                                              |
| ----------------------- | ---------------------------- | -------------------------------------------------------------------- |
| `VERSION`               | `VERSION`                    | Spec version string; surfaced as `SPEC_VERSION`                      |
| `CHANGELOG.md`          | `CHANGELOG.md`               | Spec change history; informational                                   |
| `principles/p*.md`      | `principles/p*.md`           | Frontmatter parsed by `build.rs` → `REQUIREMENTS`                    |
| `principles/scoring.md` | `principles/scoring.md`      | Scoring-formula authority; reference only (not parsed by `build.rs`) |

The spec's own `README.md` is not vendored. Its [Reading the spec](https://github.com/brettdavies/agentnative#reading-the-spec)
section holds the conventions for reading requirement text, including how a requirement that names a double-dash flag
applies to a help that declares only single-dash names; the behavioral audits read `--help` by the same rule.

## Licensing

Upstream content is licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). This crate is
dual-licensed MIT / Apache-2.0; vendoring a CC BY 4.0 source requires attribution only, satisfied by this README plus
the upstream project link in each principle's frontmatter `id` field.
