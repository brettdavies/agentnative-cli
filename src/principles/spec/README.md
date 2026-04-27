# Vendored agentnative-spec

This directory is a **vendored copy** of [`brettdavies/agentnative`](https://github.com/brettdavies/agentnative) — the
canonical specification of agent-native CLI principles. Files here are not edited by hand; they are mirrored from a
pinned upstream tag and consumed by `build.rs` to generate the `REQUIREMENTS` slice at build time.

**Current pin:** `v0.2.0`

## Resync

Run from the repo root:

```sh
scripts/sync-spec.sh                    # default: SPEC_REF=v0.2.0
SPEC_REF=v0.2.1 scripts/sync-spec.sh    # bump to a newer tag
```

The script extracts files at the named git ref via `git show`, so the spec checkout's working tree is not perturbed.
Override `SPEC_ROOT` if your spec checkout is not at `$HOME/dev/agentnative-spec`.

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
