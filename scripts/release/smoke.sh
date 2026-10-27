#!/usr/bin/env bash
# Release smoke gate: does the built binary grade real CLIs without dying, and
# does it still fail the one target it must fail?
#
# The test suite covers audit logic against fixtures in-process. This gate
# covers what it cannot: the shipped binary spawning real third-party CLIs it
# has never seen, across every `--audit-profile` the release documents.
#
# Usage:
#   scripts/release/smoke.sh [--bin PATH] [--targets SPEC] [--result-file PATH]
#
# Flags:
#   --bin PATH          Binary under test (default: $BIN_PATH, else target/release/anc)
#   --targets SPEC      Space-separated `name[:profile]` list replacing the default
#                       matrix. A target missing from PATH skips rather than fails,
#                       so the gate reports the same way on a dev box and in CI.
#   --result-file PATH  Write "<pass> <fail> <skip>" to PATH at exit for _lib.sh's
#                       delegate_to_subscript; suppresses the standalone summary
#
# Gates:
#   1. The schema the binary emits matches committed schema/scorecard.schema.json,
#      the copy consumers validate against. The binary embeds its own copy, so the
#      two can disagree without any test noticing.
#   2. Every target on PATH grades: anc exits 0/1/2, writes parseable JSON with a
#      non-empty result set, and echoes back the profile it was asked for.
#   3. Every scorecard validates against the committed schema (needs uvx).
#   4. Negative control: the no-version fixture still fails `p3-must-version`.
#   5. Every `--command` target the help text advertises resolves on PATH, so a
#      reader copying an example gets a verdict instead of a usage error.
#
# Exit codes:
#   0 = all gates passed (or skipped with reason)
#   1 = one or more gates failed

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
readonly REPO_ROOT

# shellcheck disable=SC1091  # sibling _lib.sh, always vendored alongside
. "$(dirname "$0")/_lib.sh"

# Real CLIs over the four documented profiles plus two unprofiled. Each name
# resolves from PATH; the profile is the suppression category that CLI's shape
# warrants, so the matrix exercises every `--audit-profile` value the release
# accepts rather than only the default path.
readonly DEFAULT_TARGETS="yt-dlp gh jq:posix-utility lazygit:human-tui fd:file-traversal vmstat:diagnostic-only"

readonly NO_VERSION_FIXTURE="tests/fixtures/no-version-flag/noversion"
readonly NO_VERSION_REQUIREMENT="p3-must-version"
readonly SCHEMA_FILE="schema/scorecard.schema.json"

BIN="${BIN_PATH:-$REPO_ROOT/target/release/anc}"
TARGETS="$DEFAULT_TARGETS"
RESULT_FILE=""
SCORECARD_DIR=""

cleanup_scorecards() {
  [[ -n "$SCORECARD_DIR" && -d "$SCORECARD_DIR" ]] && rm -rf "$SCORECARD_DIR"
  return 0
}
trap cleanup_scorecards EXIT

# Gate 1: emitted schema matches the committed copy ---------------------------

gate_schema_parity() {
  header "Scorecard schema parity"
  if [[ ! -f "$REPO_ROOT/$SCHEMA_FILE" ]]; then
    gate_fail "schema parity" "$SCHEMA_FILE missing from the checkout"
    return
  fi
  if "$BIN" emit schema 2>/dev/null | diff -q - "$REPO_ROOT/$SCHEMA_FILE" >/dev/null; then
    gate_pass "anc emit schema matches $SCHEMA_FILE"
  else
    gate_fail "schema parity" "anc emit schema differs from $SCHEMA_FILE; regenerate the committed copy"
  fi
}

# Gate 2: real CLIs grade without abnormal termination -----------------------
#
# anc's exit code is a verdict, not an error: 0 clean, 1 warn, 2 fail. Anything
# above 2 is the binary dying rather than grading -- a Rust panic exits 101 and
# a signal exits 128+N -- which is the condition this gate exists to catch.

grade_one_target() {
  local name="$1" profile="$2" json="$3" err="$4"
  local args=(audit --command "$name" --output json)
  [[ -n "$profile" ]] && args+=(--audit-profile "$profile")

  local code=0
  "$BIN" "${args[@]}" >"$json" 2>"$err" || code=$?

  local label="$name${profile:+ (--audit-profile $profile)}"

  if [[ "$code" -gt 2 ]]; then
    gate_fail "$label" "anc exited $code (panic or signal, not a verdict): $(head -c 200 "$err")"
    return
  fi

  local shape
  if ! shape=$(jaq -er '
    if (.schema_version // "") == "" then "no schema_version"
    elif (.results | length) == 0 then "empty result set"
    else "ok \(.schema_version) \(.results | length) rows score=\(.badge.score_pct)"
    end' "$json" 2>/dev/null); then
    gate_fail "$label" "scorecard is not parseable JSON: $(head -c 200 "$err")"
    return
  fi
  if [[ "$shape" != ok* ]]; then
    gate_fail "$label" "$shape"
    return
  fi

  # A profile the runner accepted but dropped would silently grade the default
  # path, so the scorecard has to name the profile it was asked for.
  local echoed
  echoed=$(jaq -r '.audit_profile // ""' "$json" 2>/dev/null || echo "")
  if [[ "$echoed" != "$profile" ]]; then
    gate_fail "$label" "scorecard audit_profile is '${echoed:-null}', asked for '${profile:-null}'"
    return
  fi

  gate_pass "$label exit=$code ${shape#ok }"
}

gate_real_targets() {
  header "Real-CLI grading"
  if ! command -v jaq >/dev/null 2>&1; then
    gate_skip "real-CLI grading" "jaq not on PATH (needed to read the scorecards)"
    return
  fi
  local spec name profile
  for spec in $TARGETS; do
    name="${spec%%:*}"
    profile=""
    [[ "$spec" == *:* ]] && profile="${spec#*:}"
    if ! command -v "$name" >/dev/null 2>&1; then
      gate_skip "$name" "not on PATH"
      continue
    fi
    grade_one_target "$name" "$profile" "$SCORECARD_DIR/$name.json" "$SCORECARD_DIR/$name.err"
  done
}

# Gate 3: every scorecard validates against the committed schema -------------

gate_schema_validation() {
  header "Scorecard schema validation"
  local cards=()
  while IFS= read -r -d '' card; do cards+=("$card"); done \
    < <(find "$SCORECARD_DIR" -maxdepth 1 -name '*.json' -print0 2>/dev/null)
  if [[ ${#cards[@]} -eq 0 ]]; then
    gate_skip "schema validation" "no scorecards were produced to validate"
    return
  fi
  if ! command -v uvx >/dev/null 2>&1; then
    gate_skip "schema validation" "uvx not on PATH (needed for check-jsonschema)"
    return
  fi
  # Status captured off the command itself: through a pipe, $? reports the last
  # stage, which reads every invalid document as a pass.
  local code=0
  uvx --quiet check-jsonschema --schemafile "$REPO_ROOT/$SCHEMA_FILE" "${cards[@]}" \
    >"$SCORECARD_DIR/validate.log" 2>&1 || code=$?
  if [[ "$code" -eq 0 ]]; then
    gate_pass "${#cards[@]} scorecard(s) validate against $SCHEMA_FILE"
  else
    gate_fail "schema validation" "$(head -c 400 "$SCORECARD_DIR/validate.log")"
  fi
}

# Gate 4: the negative control ------------------------------------------------
#
# Every gate above passes when the binary grades everything green, which is also
# what a thoroughly broken binary does. This one target must come back failing,
# so the suite can tell a working auditor from one that has stopped auditing.

gate_negative_control() {
  header "Negative control (a target that must fail)"
  local fixture="$REPO_ROOT/$NO_VERSION_FIXTURE"
  if [[ ! -x "$fixture" ]]; then
    gate_fail "negative control" "$NO_VERSION_FIXTURE missing or not executable"
    return
  fi
  if ! command -v jaq >/dev/null 2>&1; then
    gate_skip "negative control" "jaq not on PATH (needed to read the scorecard)"
    return
  fi

  local json="$SCORECARD_DIR/negative-control.json"
  local code=0
  "$BIN" audit "$fixture" --output json >"$json" 2>/dev/null || code=$?
  if [[ "$code" -gt 2 ]]; then
    gate_fail "negative control" "anc exited $code grading the fixture (panic or signal)"
    return
  fi

  local status
  # shellcheck disable=SC2016  # $id is a jaq variable bound by --arg, not a shell expansion
  status=$(jaq -r --arg id "$NO_VERSION_REQUIREMENT" \
    'first(.results[] | select(.id == $id) | .status) // "absent"' "$json" 2>/dev/null || echo "absent")
  case "$status" in
    fail) gate_pass "$NO_VERSION_REQUIREMENT fails on the no-version fixture" ;;
    absent) gate_fail "negative control" "$NO_VERSION_REQUIREMENT is absent from the fixture's scorecard" ;;
    *) gate_fail "negative control" "$NO_VERSION_REQUIREMENT is '$status' on a CLI with no --version flag" ;;
  esac
}

# Gate 5: the advertised examples name commands that exist --------------------
#
# Every `--command <name>` the help text advertises is something a reader will
# copy verbatim. v0.6.0 shipped `--command ripgrep`, which exits 2 everywhere
# because the binary ripgrep installs is `rg`. No unit test catches this: the
# flag parses, and resolution needs a machine with the tool on PATH. The
# release host is that machine, so the check belongs here.

gate_examples_resolve() {
  header "Advertised examples name resolvable commands"
  local names
  names=$(
    {
      "$BIN" --examples 2>/dev/null
      "$BIN" audit --help 2>/dev/null
      "$BIN" --help 2>/dev/null
    } \
      | grep -oE -- '--command[= ]+[A-Za-z0-9_.-]+' \
      | sed -E 's/--command[= ]+//' \
      | sort -u
  )
  if [[ -z "$names" ]]; then
    gate_fail "examples" "no --command example found in --examples or --help (did the block move?)"
    return
  fi

  local name unresolved=""
  while IFS= read -r name; do
    [[ -z "$name" ]] && continue
    command -v -- "$name" >/dev/null 2>&1 || unresolved+=" $name"
  done <<<"$names"

  if [[ -n "$unresolved" ]]; then
    gate_fail "examples" "advertised --command target(s) not on PATH:${unresolved}"
    return
  fi
  gate_pass "every advertised --command target resolves ($(echo "$names" | tr '\n' ' ' | sed 's/ $//'))"
}

# Main -----------------------------------------------------------------------

usage() {
  sed -n '2,31p' "$0" | sed 's/^# \?//'
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bin)
      BIN="$2"
      shift 2
      ;;
    --targets)
      TARGETS="$2"
      shift 2
      ;;
    --result-file)
      RESULT_FILE="$2"
      shift 2
      ;;
    -h | --help) usage ;;
    *)
      echo "unknown arg: $1" >&2
      usage
      ;;
  esac
done

main() {
  if [[ ! -x "$BIN" ]]; then
    header "Real-world smoke"
    gate_fail "binary under test" "$BIN not found or not executable (cargo build --release)"
  else
    SCORECARD_DIR="$(mktemp -d -t anc-smoke-XXXXXX)"
    cd "$REPO_ROOT"
    gate_schema_parity
    gate_real_targets
    gate_schema_validation
    gate_negative_control
    gate_examples_resolve
  fi

  if [[ -n "$RESULT_FILE" ]]; then
    printf "%d %d %d\n" "$PASS_COUNT" "$FAIL_COUNT" "$SKIP_COUNT" >"$RESULT_FILE"
  else
    print_summary
  fi
  [[ $FAIL_COUNT -eq 0 ]] || exit 1
}

main "$@"
