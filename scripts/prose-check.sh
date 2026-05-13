#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Pre-push prose-check orchestrator.
#
# Walks the in-scope *.md set, runs Vale (custom Brand+Spec packs +
# write-good + proselint baseline) at MinAlertLevel=warning with JSON
# output, splits severity orchestrator-side, then probes LanguageTool
# over Tailscale and runs grammar checks if reachable. Vale errors and
# category-whitelisted LT matches block the push; warnings annotate but
# do not block. When LT is unreachable, the LT stage skips with a
# notice and the push proceeds on Vale's verdict alone (R9 graceful
# skip).
#
# Downstream LLM-judgment step: `unslop`.
#
# This script covers the deterministic floor (Vale + LT). The unslop
# skill (~/.claude/skills/unslop/SKILL.md) is the LLM-judgment ceiling
# that runs *after* Vale + LT pass — it catches AI-unique structural
# patterns (em-dash density, "It's not X, it's Y", forced enthusiasm,
# AI self-references) that no deterministic rule pack covers.
#
# CRITICAL when invoking unslop on this repo's prose:
#   - Always run `score.py --json` and read the `findings[]` array
#     per-occurrence (line, column, contextual snippet, rule_id).
#     The bare invocation only prints the aggregate score line, which
#     is enough to gate but not enough to recast well.
#   - Recasting is per-occurrence judgment, NOT bucket substitution.
#     Do not reduce em-dashes to {colon, parens, period} based on the
#     surrounding construction shape — the recasting.md table lists
#     six different em-dash jobs (aside, explanation, contrast,
#     because-substitute, list-separator, stylistic pause) with six
#     different right moves. If a pass touching N>5 findings used only
#     2-3 distinct moves, it almost certainly bucketed.
#
# This warning exists because of a 2026-05-07 incident: a v0.4.0 unslop
# pass on 9 principle files in this repo produced clean scores via
# three mechanical substitutions; a 10-agent per-occurrence re-pass
# found 16 cases where a different move was more faithful, including
# 4 because-substitutes the original pass missed.
#
# Canonical references:
#   ~/.claude/skills/unslop/SKILL.md
#   ~/.claude/skills/unslop/references/recasting.md
#
# Usage:
#   scripts/prose-check.sh                 full scope, errors only (pre-push default)
#   scripts/prose-check.sh --changed-only  only files changed vs $PROSE_CHECK_BASE (default origin/dev)
#   scripts/prose-check.sh --warnings      surface warning-tier findings too
#   scripts/prose-check.sh --vale-only     skip LT entirely (offline iteration)
#   scripts/prose-check.sh --lt-only       skip Vale entirely (LT debugging)
#
# Env:
#   LANGUAGETOOL_URL    LT base URL (default: http://pool.tail42ba87.ts.net:8081)
#                       FQDN avoids macOS+Tailscale short-name DNS timeouts.
#   PROSE_CHECK_BASE    git ref to diff against in --changed-only (default: origin/dev)

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

LT_URL_DEFAULT="http://pool.tail42ba87.ts.net:8081"
LT_URL="${LANGUAGETOOL_URL:-$LT_URL_DEFAULT}"
PROSE_CHECK_BASE="${PROSE_CHECK_BASE:-origin/dev}"
# LT blocking whitelist — narrowed from the plan's 7-category default
# (TYPOS|GRAMMAR|PUNCTUATION|TYPOGRAPHY|CASING|COMPOUNDING|CONFUSED_WORDS)
# to the three categories that are reliably high-signal on markdown corpora.
# PUNCTUATION/TYPOGRAPHY/CASING/COMPOUNDING fired ~95% noise on the spec
# corpus from LT misreading markdown syntax (table whitespace, `->` arrows,
# code-fence quotes); they remain on the warning tier (visible via
# --warnings). Re-promote to blocking when LT gains markdown awareness or
# a per-rule allowlist lands.
LT_BLOCKING_CATEGORIES='^(TYPOS|GRAMMAR|CONFUSED_WORDS)$'

# Per-rule denylist within the blocking categories — specific LT rule
# IDs that misfire on RFC 2119 keyword conventions or on technical-prose
# patterns the rule pack does not cover. Override via LT_DENY_RULES env.
#
#   MD_BASEFORM            "MUST <verb>" / "MAY <verb>" — LT does not
#                          recognize RFC 2119 keywords; treats them as
#                          modal-verb usage and demands base form.
#   MUST_HAVE_TO           Same root cause for "must" usage.
#   HAVE_PART_AGREEMENT    Misfires on "if: CLI has X" YAML-prose.
#   PREPOSITION_VERB       Misfires on workflow names ("deploy / publish").
#   THIS_NNS               Misfires on "all of these hold" technical claims.
#   NON_STANDARD_WORD      Misfires on identifier strings inside code spans.
#   POSSESSIVE_APOSTROPHE  Misfires on code-comment-style prose.
#   A_INSTALL              Misfires on "an install path" / "a full reinstall"
#                          — CLI-domain noun usage of install/reinstall that
#                          LT's noun lexicon does not cover.
#   IS_AND_ARE             Misfires on parenthetical-clause subjects, e.g.
#                          "runtimes (Claude Code, Cursor, ... and others as
#                          the ecosystem evolves)" — LT picks the wrong head
#                          noun when a parenthetical sits between subject and
#                          verb.
#   SINGULAR_NOUN_ADV_AGREEMENT
#                          Same class of misfire on subordinate-clause
#                          subjects, e.g. "Agents consuming JSON output still
#                          receive interleaved diagnostic text" — LT parses
#                          "JSON output" as the head noun and demands a
#                          singular verb when the actual subject ("Agents")
#                          is plural.
LT_DENY_RULES_DEFAULT='^(MD_BASEFORM|MUST_HAVE_TO|HAVE_PART_AGREEMENT|PREPOSITION_VERB|THIS_NNS|NON_STANDARD_WORD|POSSESSIVE_APOSTROPHE|A_INSTALL|IS_AND_ARE|SINGULAR_NOUN_ADV_AGREEMENT)$'
LT_DENY_RULES="${LT_DENY_RULES:-$LT_DENY_RULES_DEFAULT}"

CHANGED_ONLY=0
SHOW_WARNINGS=0
RUN_VALE=1
RUN_LT=1

while (( $# )); do
  case "$1" in
    --changed-only) CHANGED_ONLY=1 ;;
    --warnings) SHOW_WARNINGS=1 ;;
    --vale-only) RUN_LT=0 ;;
    --lt-only) RUN_VALE=0 ;;
    -h|--help) sed -n '3,28p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "prose-check: unknown flag '$1'" >&2; exit 2 ;;
  esac
  shift
done

# --- File enumeration ---
# Exclusion regex has two anchored alternatives:
#   - path-prefix group: matches paths under docs/{brainstorms,ideation,plans,research,solutions},
#     styles/{proselint,write-good,.vale-config}, scripts/__fixtures__/,
#     src/principles/spec/, tests/fixtures/
#   - basename group: matches AGENTS.md / CHANGELOG.md / coverage-matrix.md
#     anywhere in the tree (line start OR after a slash), to keep parity with
#     full-scan mode's `find -not -name 'AGENTS.md'` which matches basename anywhere.
#
# === CLI-LOCAL DIVERGENCE FROM agentnative-spec@v0.4.0 ====================
# The block below patches in consumer-side carveouts the upstream orchestrator
# does not yet support natively:
#   1. `src/principles/spec/` — vendored from agentnative-spec; lints upstream,
#      double-linting wastes findings on register that is wrong for the CLI.
#   2. `docs/ideation/` — CLI-side ideation drafts (analogous to upstream's
#      docs/brainstorms; spec has no `ideation` directory and the upstream
#      exclusion list does not cover it).
#   3. `tests/fixtures/` — Rust integration-test fixtures, some of which carry
#      deliberately malformed prose (perfect-rust/AGENTS.md and siblings).
#   4. `coverage-matrix.md` — generated artifact emitted by `anc generate
#      coverage-matrix`; not authored prose.
# These edits are restored after each `bash scripts/sync-prose-tooling.sh` run
# until upstream lands the `--exclude PATTERN` flag tracked at
# `agentnative-spec/.context/compound-engineering/todos/010-pending-p0-prose-check-consumer-exclusion-config.md`.
# Re-apply after sync; `git diff scripts/prose-check.sh` surfaces the regression.
# ==========================================================================
if (( CHANGED_ONLY )); then
  mapfile -t MD_FILES < <(
    git diff --name-only --diff-filter=ACM "$PROSE_CHECK_BASE"...HEAD -- '*.md' \
      | grep -v -E '^(docs/(brainstorms|ideation|plans|research|solutions)/|styles/(proselint|write-good|\.vale-config)/|scripts/__fixtures__/|src/principles/spec/|tests/fixtures/)|(^|/)(AGENTS|CHANGELOG|coverage-matrix)\.md$' \
      | sort -u
  )
else
  mapfile -t MD_FILES < <(
    find . -type f -name '*.md' \
      -not -path './node_modules/*' \
      -not -path './.git/*' \
      -not -path './.context/*' \
      -not -path './scripts/__fixtures__/*' \
      -not -path './docs/brainstorms/*' \
      -not -path './docs/ideation/*' \
      -not -path './docs/plans/*' \
      -not -path './docs/research/*' \
      -not -path './docs/solutions/*' \
      -not -path './src/principles/spec/*' \
      -not -path './tests/fixtures/*' \
      -not -path './styles/proselint/*' \
      -not -path './styles/write-good/*' \
      -not -path './styles/.vale-config/*' \
      -not -name 'AGENTS.md' \
      -not -name 'CHANGELOG.md' \
      -not -name 'coverage-matrix.md' \
      | sed 's|^\./||' \
      | sort
  )
fi
# === END CLI-LOCAL DIVERGENCE ============================================

if (( ${#MD_FILES[@]} == 0 )); then
  echo "prose-check: 0 markdown files in scope; nothing to check"
  exit 0
fi

BLOCKING=0
WARNING=0
OUT_FILE="$(mktemp)"
trap 'rm -f "$OUT_FILE"' EXIT

# --- Vale stage ---
if (( RUN_VALE )); then
  VALE_JSON="$(vale --no-global --output=JSON --minAlertLevel=warning "${MD_FILES[@]}" 2>/dev/null || true)"
  if [[ -n "$VALE_JSON" && "$VALE_JSON" != "{}" ]]; then
    while IFS=$'\t' read -r file line col sev rule msg; do
      [[ -z "$file" ]] && continue
      if [[ "$sev" == "error" ]]; then
        BLOCKING=$((BLOCKING + 1))
        printf '%s:%s:%s:%s: %s\n' "$file" "$line" "$col" "$rule" "$msg" >> "$OUT_FILE"
      else
        WARNING=$((WARNING + 1))
        if (( SHOW_WARNINGS )); then
          printf '[warn] %s:%s:%s:%s: %s\n' "$file" "$line" "$col" "$rule" "$msg" >> "$OUT_FILE"
        fi
      fi
    done < <(jaq -r 'to_entries[] | .key as $f | .value[] | [$f, .Line, .Span[0], .Severity, .Check, .Message] | @tsv' <<<"$VALE_JSON")
  fi
fi

# --- LanguageTool stage ---
if (( RUN_LT )); then
  if curl --max-time 2 -fsS "$LT_URL/v2/languages" >/dev/null 2>&1; then
    LT_TMP="$(mktemp -d)"
    trap 'rm -rf "$LT_TMP" "$OUT_FILE"' EXIT

    printf '%s\0' "${MD_FILES[@]}" | xargs -0 -P4 -I{} bash -c '
      file="$1"; tmp="$2"; url="$3"
      out="$tmp/$(echo "$file" | tr "/" "_").json"
      curl -sS --max-time 30 -X POST "$url/v2/check" \
        --data-urlencode "language=en-US" \
        --data-urlencode "text@$file" > "$out" 2>/dev/null || true
    ' _ {} "$LT_TMP" "$LT_URL"

    for f in "${MD_FILES[@]}"; do
      json="$LT_TMP/$(echo "$f" | tr '/' '_').json"
      [[ -s "$json" ]] || continue
      while IFS=$'\t' read -r offset rule_id category message; do
        [[ -z "$offset" ]] && continue
        # Approximate line from byte offset (no exact column conversion at v1).
        line=$(awk -v off="$offset" 'BEGIN{cur=0} {cur+=length($0)+1; if (cur>off) {print NR; exit}}' "$f" 2>/dev/null)
        line="${line:-?}"
        if [[ "$category" =~ $LT_BLOCKING_CATEGORIES ]] && ! [[ "$rule_id" =~ $LT_DENY_RULES ]]; then
          BLOCKING=$((BLOCKING + 1))
          printf '%s:%s:LT.%s (%s): %s\n' "$f" "$line" "$rule_id" "$category" "$message" >> "$OUT_FILE"
        else
          WARNING=$((WARNING + 1))
          if (( SHOW_WARNINGS )); then
            printf '[warn] %s:%s:LT.%s (%s): %s\n' "$f" "$line" "$rule_id" "$category" "$message" >> "$OUT_FILE"
          fi
        fi
      done < <(jaq -r '.matches[]? | [.offset, .rule.id, .rule.category.id, .message] | @tsv' "$json" 2>/dev/null || true)
    done
  else
    rc=$?
    case "$rc" in
      6)  reason="couldn't resolve host (Tailscale likely off, or FQDN drift)" ;;
      7)  reason="couldn't connect (host up, LT service down)" ;;
      28) reason="timed out (>2s; service slow or network impaired)" ;;
      *)  reason="curl exit $rc" ;;
    esac
    echo "prose-check: LanguageTool unreachable at $LT_URL — $reason; skipping grammar check" >&2
  fi
fi

# Print findings sorted by file then line
if [[ -s "$OUT_FILE" ]]; then
  sort -t: -k1,1 -k2,2n "$OUT_FILE"
fi

echo "prose-check: $BLOCKING blocking, $WARNING warning"
(( BLOCKING > 0 )) && exit 1
exit 0
