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
# With `scripts/smoke-web-audit.sh <binary> --panic-probe`, on a release binary
# built with the `panic-probe` feature, it then audits the site twice more: with
# one handler panicking, which costs that row and not the run, and with the run
# itself panicking, which ends in the could-not-check exit and an error
# envelope. It then audits a command with one audit panicking, which costs that
# audit its row. A test build always unwinds, so only a release-profile build
# can show any of them.
#
# The fixture is served from a temp directory by Python's own http.server
# over TLS, so the check needs no fixture host and makes no external
# request. `anc web` sends HTTPS only, so the script mints a throwaway CA
# and a certificate it signs for 127.0.0.1 with the openssl CLI, and passes
# the CA with `--ca-cert`.
set -euo pipefail

readonly BINARY="${1:-target/release/anc}"
readonly MODE="${2:-}"
case "$MODE" in
  "" | --panic-probe) ;;
  *)
    printf 'error: unknown mode %s; expected --panic-probe\n' "$MODE" >&2
    exit 2
    ;;
esac
# Ten times the measured local run, and well inside the promise, so this
# fails on a hang rather than on a slow runner.
readonly BUDGET_SECONDS=30

if [[ ! -x "$BINARY" ]]; then
  printf 'error: %s not found or not executable; run "cargo build --release --features web" first\n' "$BINARY" >&2
  exit 2
fi
for tool in python3 jaq openssl; do
  command -v "$tool" >/dev/null || {
    printf 'error: %s is required\n' "$tool" >&2
    exit 2
  }
done

fixture=$(mktemp -d)
tls=$(mktemp -d)
server_pid=""
cleanup() {
  [[ -n "$server_pid" ]] && kill "$server_pid" 2>/dev/null
  rm -rf "$fixture" "$tls"
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

# A CA and a leaf it signs for 127.0.0.1, minted fresh each run and kept
# outside the served directory. The leaf is CA:FALSE with serverAuth, the
# shape a private CA issues, so the run exercises a real chain to a root it
# trusts only through --ca-cert.
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
  -keyout "$tls/ca.key" -out "$tls/ca.pem" -days 1 -subj "/CN=anc smoke CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign" 2>/dev/null
openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
  -keyout "$tls/leaf.key" -out "$tls/leaf.csr" -subj "/CN=anc smoke fixture" 2>/dev/null
printf 'subjectAltName=IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\n' \
  >"$tls/leaf.ext"
openssl x509 -req -in "$tls/leaf.csr" -CA "$tls/ca.pem" -CAkey "$tls/ca.key" -CAcreateserial \
  -out "$tls/leaf.pem" -days 1 -extfile "$tls/leaf.ext" 2>/dev/null

# Bind port 0 so the check never collides with anything already listening,
# and report the port the OS handed out.
port_file="$fixture/.port"
# The stock handler answers HTTP/1.0 and so closes every connection, which
# is the shape that made the transport's pooled-connection retry necessary;
# leaving it that way keeps that path under the smoke check.
python3 - "$fixture" "$port_file" "$tls/leaf.pem" "$tls/leaf.key" <<'PY' &
import functools
import http.server
import pathlib
import socketserver
import ssl
import sys

directory, port_file, cert, key = sys.argv[1:5]
http.server.SimpleHTTPRequestHandler.log_message = lambda *a, **k: None
handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=directory)
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(cert, key)


class Quiet(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True
    # The audit opens its probes concurrently; socketserver's default
    # backlog of 5 lets a busy runner drop the connections past it.
    request_queue_size = 64

    # The handshake runs on the connection's own thread, so one slow or
    # failed handshake never stalls the accept loop.
    def finish_request(self, request, client_address):
        try:
            request = context.wrap_socket(request, server_side=True)
        except (ssl.SSLError, OSError):
            return
        self.RequestHandlerClass(request, client_address, self)

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
# Scheme-less on purpose: a local target defaults to https like any other.
target="127.0.0.1:$port"
printf 'auditing the fixture site at https://%s/\n' "$target"

scorecard="$fixture/scorecard.json"
stderr_log="$fixture/stderr.log"
started=$(date +%s)
set +e
timeout "$BUDGET_SECONDS" "$BINARY" web "$target" --ca-cert "$tls/ca.pem" --output json \
  >"$scorecard" 2>"$stderr_log"
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
  printf 'error: the run returned %s; expected a verdict (0, 1 or 2). anc wrote:\n' "$code" >&2
  sed 's/^/  /' "$stderr_log" >&2
  # Exit 3 with a scorecard on stdout means rows errored or were cut short
  # by the deadline rather than the target going unanswered; name them.
  if [[ -s "$scorecard" ]]; then
    jaq -r '.results[] | select(.status == "error") | "  error row: \(.id): \(.evidence // "")"' \
      <"$scorecard" >&2 || true
  fi
  exit 1
fi

# The registry the binary carries says how many rows a run reports; a run
# that reports fewer has lost rows.
expected_rows=$("$BINARY" emit web-checks | jaq '.checks | length')
rows=$(jaq '.results | length' <"$scorecard")
passed=$(jaq '.summary.pass' <"$scorecard")
if [[ "$rows" != "$expected_rows" ]]; then
  printf 'error: the scorecard carries %s rows; expected %s\n' "$rows" "$expected_rows" >&2
  exit 1
fi
if ((passed < 1)); then
  printf 'error: no check passed against the fixture site\n' >&2
  exit 1
fi

printf 'first run: %ss (budget %ss), exit %s, %s rows, %s passed\n' \
  "$elapsed" "$BUDGET_SECONDS" "$code" "$rows" "$passed"

if [[ "$MODE" != "--panic-probe" ]]; then
  exit 0
fi

# Runs the audit with ANC_PANIC_PROBE naming $1, leaving the code in $code and
# the streams in $probed and $stderr_log. Every panic ends at the could-not-check
# exit; anything else is the panic ending the run another way.
probed="$fixture/probed.json"
probe_run() {
  local site=$1
  shift
  set +e
  ANC_PANIC_PROBE="$site" timeout "$BUDGET_SECONDS" "$BINARY" web "$target" \
    --ca-cert "$tls/ca.pem" "$@" >"$probed" 2>"$stderr_log"
  code=$?
  set -e
  if ((code != 3)); then
    printf 'error: a panic in %s returned %s; expected 3. anc wrote:\n' "$site" "$code" >&2
    sed 's/^/  /' "$stderr_log" >&2
    exit 1
  fi
}

# A row that passed reached its handler, so it is one the probe can panic in.
probe_id=$(jaq -r '[.results[] | select(.status == "pass")][0].id' <"$scorecard")
probe_run "$probe_id" --output json
if grep -q panicked "$stderr_log"; then
  printf 'error: raw panic text reached stderr in JSON mode:\n' >&2
  sed 's/^/  /' "$stderr_log" >&2
  exit 1
fi
# shellcheck disable=SC2016 # $id is jaq's variable, not the shell's.
row=$(jaq -r --arg id "$probe_id" '.results[] | select(.id == $id) | "\(.status): \(.evidence)"' <"$probed")
if [[ "$row" != "error: handler error: panic probe in $probe_id"* ]]; then
  printf 'error: the %s row reads %q; expected the handler error\n' "$probe_id" "$row" >&2
  exit 1
fi
rows=$(jaq '.results | length' <"$probed")
if [[ "$rows" != "$expected_rows" ]]; then
  printf 'error: the probed scorecard carries %s rows; expected %s\n' "$rows" "$expected_rows" >&2
  exit 1
fi
printf 'panic in %s: exit %s, %s rows, its row: %s\n' "$probe_id" "$code" "$rows" "$row"

probe_run run --output json
if ! jaq -e '.error == "panic" and .exit_code == 3 and .message == "panic probe in run"
  and .next_step.action == "report-bug" and (.next_step.command | startswith("anc --verbose "))' \
  <"$stderr_log" >/dev/null; then
  printf 'error: a panic in the run left no error envelope on stderr:\n' >&2
  sed 's/^/  /' "$stderr_log" >&2
  exit 1
fi
printf 'panic in the run, JSON: exit %s, envelope on stderr\n' "$code"

probe_run run
if ! grep -q 'error: anc panicked: panic probe in run' "$stderr_log"; then
  printf 'error: a panic in the run named no error in text mode:\n' >&2
  sed 's/^/  /' "$stderr_log" >&2
  exit 1
fi
printf 'panic in the run, text: exit %s, named on stderr\n' "$code"

set +e
ANC_PANIC_PROBE=p3-help timeout "$BUDGET_SECONDS" "$BINARY" audit --command ls --output json \
  >"$probed" 2>"$stderr_log"
code=$?
set -e
# shellcheck disable=SC2016 # $id is jaq's variable, not the shell's.
row=$(jaq -r '[.results[] | select(.audit_id == "p3-help")][0] | "\(.status): \(.evidence)"' <"$probed")
if ((code != 3)) || [[ "$row" != "error: panic: panic probe in p3-help"* ]]; then
  printf 'error: a panic in one audit returned %s, its row: %q. anc wrote:\n' "$code" "$row" >&2
  sed 's/^/  /' "$stderr_log" >&2
  exit 1
fi
printf 'panic in p3-help: exit %s, its row: %s\n' "$code" "$row"
