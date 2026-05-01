# Vendored agentnative-spec

This directory is a **vendored copy** of [`brettdavies/agentnative`](https://github.com/brettdavies/agentnative) — the
canonical specification of agent-native CLI principles. Files here are not edited by hand; they are mirrored from the
latest upstream `v*` tag and consumed by `build.rs` to generate the `REQUIREMENTS` slice at build time. The currently
vendored version is recorded in [`VERSION`](./VERSION).

## Resync

Run from the repo root:

```sh
scripts/sync-spec.sh    # queries the remote for the latest v* tag; falls back to local on network failure
```

The script queries `https://github.com/brettdavies/agentnative.git` for the latest `v*` tag and shallow-clones that tag
into a temp directory for extraction. If the remote is unreachable, it falls back to a local checkout
(`$HOME/dev/agentnative-spec` by default; override with `SPEC_ROOT`). Override `SPEC_REMOTE_URL` to query a different
remote. The script extracts files via `git show`, so neither source's working tree is perturbed.

## Layout

| Path               | Source in `agentnative-spec` | Purpose                                           |
| ------------------ | ---------------------------- | ------------------------------------------------- |
| `VERSION`          | `VERSION`                    | Spec version string; surfaced as `SPEC_VERSION`   |
| `CHANGELOG.md`     | `CHANGELOG.md`               | Spec change history; informational                |
| `principles/p*.md` | `principles/p*.md`           | Frontmatter parsed by `build.rs` → `REQUIREMENTS` |

## Licensing

Upstream content is licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). This crate is
dual-licensed MIT / Apache-2.0; vendoring a CC BY 4.0 source requires attribution only, satisfied by this README plus
the upstream project link in each principle's frontmatter `id` field.
