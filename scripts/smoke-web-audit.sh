#!/usr/bin/env bash
# First-run smoke check: the release binary audits a local site, end to end,
# inside a wall-clock budget.
#
# The product promise is a website audit in one or two minutes with nothing
# to install but the binary itself. The install half of that is a single
# download with no runtime, which the release workflow already proves by
# publishing the artifacts; this script measures the other half, which is
# the half that can regress silently. A handler that stops honoring the
# per-audit deadline, a probe that blocks on a socket, or a DNS lookup that
# sneaks into a local run all show up here as wall clock.
#
# The fixture is served from a temp directory by Python's own http.server,
# so the check needs no fixture host and makes no external request.
set -euo pipefail

readonly BINARY="${1:-target/release/anc}"
# Ten times the measured local run, and well inside the promise, so this
# fails on a hang rather than on a slow runner.
readonly BUDGET_SECONDS=30
# The registry's check count: a run that reports fewer has lost rows.
readonly EXPECTED_ROWS=65

if [[ ! -x "$BINARY" ]]; then
  printf 'error: %s not found or not executable; run "cargo build --release" first\n' "$BINARY" >&2
  exit 2
fi
for tool in python3 jaq; do
  command -v "$tool" >/dev/null || {
    printf 'error: %s is required\n' "$tool" >&2
    exit 2
  }
done

fixture=$(mktemp -d)
server_pid=""
cleanup() {
  [[ -n "$server_pid" ]] && kill "$server_pid" 2>/dev/null
  rm -rf "$fixture"
}
trap cleanup EXIT

# A site with enough agent surface that the run exercises real handlers:
# an HTML root with a description and a markdown alternate, an llms.txt with
# a resolvable link, and a robots.txt.
{
  printf '<!doctype html><html><head>'
  printf '<meta name="description" content="A fixture site for the first-run smoke check.">'
  printf '<link rel="alternate" type="text/markdown" href="/index.md">'
  printf '</head><body><main><h1>Fixture</h1><p>'
  for _ in $(seq 1 8); do printf 'Readable prose about the fixture service and its agent surfaces. '; done
  printf '</p></main><noscript><a href="/llms.txt">llms.txt</a></noscript></body></html>'
} >"$fixture/index.html"
printf '# Fixture\n\n> A fixture site for the smoke check.\n\n## When to use\n\n- [Home](/index.html)\n' \
  >"$fixture/llms.txt"
printf 'User-agent: *\nAllow: /\n' >"$fixture/robots.txt"

# Bind port 0 so the check never collides with anything already listening,
# and report the port the OS handed out.
port_file="$fixture/.port"
# The stock handler answers HTTP/1.0 and so closes every connection, which
# is the shape that made the transport's pooled-connection retry necessary;
# leaving it that way keeps that path under the smoke check.
python3 - "$fixture" "$port_file" <<'PY' &
import functools
import http.server
import pathlib
import socketserver
import sys

directory, port_file = sys.argv[1], sys.argv[2]
http.server.SimpleHTTPRequestHandler.log_message = lambda *a, **k: None
handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=directory)


class Quiet(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True

    def handle_error(self, request, client_address):
        pass


with Quiet(("127.0.0.1", 0), handler) as httpd:
    pathlib.Path(port_file).write_text(str(httpd.server_address[1]))
    httpd.serve_forever()
PY
server_pid=$!

for _ in $(seq 1 50); do
  [[ -s "$port_file" ]] && break
  sleep 0.1
done
if [[ ! -s "$port_file" ]]; then
  printf 'error: the fixture server never reported a port\n' >&2
  exit 1
fi
port=$(cat "$port_file")
target="127.0.0.1:$port"
printf 'auditing the fixture site at http://%s/\n' "$target"

scorecard="$fixture/scorecard.json"
started=$(date +%s)
set +e
timeout "$BUDGET_SECONDS" "$BINARY" web "$target" --output json >"$scorecard"
code=$?
set -e
elapsed=$(($(date +%s) - started))

if ((code == 124)); then
  printf 'error: the run did not finish inside %ss\n' "$BUDGET_SECONDS" >&2
  exit 1
fi
# 0, 1 and 2 are verdicts; 3 means nothing answered, which for a fixture
# this script just started is a failure of the run, not of the fixture.
if ((code > 2)); then
  printf 'error: the run returned %s; expected a verdict (0, 1 or 2)\n' "$code" >&2
  exit 1
fi

rows=$(jaq '.results | length' <"$scorecard")
passed=$(jaq '.summary.pass' <"$scorecard")
if [[ "$rows" != "$EXPECTED_ROWS" ]]; then
  printf 'error: the scorecard carries %s rows; expected %s\n' "$rows" "$EXPECTED_ROWS" >&2
  exit 1
fi
if ((passed < 1)); then
  printf 'error: no check passed against the fixture site\n' >&2
  exit 1
fi

printf 'first run: %ss (budget %ss), exit %s, %s rows, %s passed\n' \
  "$elapsed" "$BUDGET_SECONDS" "$code" "$rows" "$passed"
