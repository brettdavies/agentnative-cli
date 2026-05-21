#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# === CONSUMER-OWNED (un-vendored 2026-05-13) ===
# This script is no longer vendored from agentnative-spec via
# sync-prose-tooling.sh. The vendored upstream kept clobbering the
# CLI-LOCAL DIVERGENCE block (consumer-specific path exclusions and
# LT denylist additions) on every sync.
#
# Universal changes to the prose-check pipeline (new check stage, LT
# URL change, severity routing) now need coordinated PRs across all
# four channel repos:
#   - agentnative-spec      (origin / SoT)
#   - agentnative-site
#   - agentnative-cli       (this repo)
#   - agentnative-skill
#
# Long-term fix tracked at agentnative-spec/.context/compound-engineering/todos/
# (sidecar-config migration; once shipped, vendoring can resume with
# universal logic vendored and consumer config in a sidecar file).
# === END CONSUMER-OWNED ===
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
#   LANGUAGETOOL_URL    LT base URL (default: http://languagetool:8081).
#                       Consumed by lt_check (~/dotfiles/config/shell/languagetool.sh).
#   LT_DENY_RULES       Extend the baseline 10-rule denylist with repo-specific
#                       rule IDs. See lt_check's LT_DENY_RULES_BASELINE for the
#                       default; format as "${LT_DENY_RULES_BASELINE}|EXTRA_RULE".
#   PROSE_CHECK_BASE    git ref to diff against in --changed-only (default: origin/dev)

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# LanguageTool wrapper: see ~/dotfiles/config/shell/languagetool.sh for the
# baseline 10-rule denylist (LT_DENY_RULES_BASELINE), category whitelist,
# and exit-code contract. Reachability probe and per-file POST live there.
LT_LIB="${DOTFILES_SHELL_DIR:-$HOME/dotfiles/config/shell}/languagetool.sh"
if [[ ! -f "$LT_LIB" ]]; then
  echo "prose-check: required helper $LT_LIB not found (install brettdavies/dotfiles)" >&2
  exit 2
fi
# shellcheck disable=SC1090
source "$LT_LIB"

PROSE_CHECK_BASE="${PROSE_CHECK_BASE:-origin/dev}"

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
  LT_OUT="$(mktemp)"
  trap 'rm -f "$OUT_FILE" "$LT_OUT"' EXIT
  LT_RC=0
  lt_check "${MD_FILES[@]}" > "$LT_OUT" || LT_RC=$?
  case "$LT_RC" in
    0|1) ;;  # findings (if any) are in LT_OUT
    2) echo "prose-check: skipping grammar check (see lt_check notice above)" >&2 ;;
    *) echo "prose-check: lt_check returned unexpected exit $LT_RC" >&2; exit 2 ;;
  esac
  while IFS= read -r ln; do
    [[ -z "$ln" ]] && continue
    if [[ "$ln" == "[warn] "* ]]; then
      WARNING=$((WARNING + 1))
      (( SHOW_WARNINGS )) && printf '%s\n' "$ln" >> "$OUT_FILE"
    else
      BLOCKING=$((BLOCKING + 1))
      printf '%s\n' "$ln" >> "$OUT_FILE"
    fi
  done < "$LT_OUT"
fi

# Print findings sorted by file then line
if [[ -s "$OUT_FILE" ]]; then
  sort -t: -k1,1 -k2,2n "$OUT_FILE"
fi

echo "prose-check: $BLOCKING blocking, $WARNING warning"
(( BLOCKING > 0 )) && exit 1
exit 0
