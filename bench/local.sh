#!/usr/bin/env bash
# Whole audits against a local keep-alive TLS server with gzip bodies, interleaved: the anc binary's
# own CPU with the network's noise taken out. Then the handshake harness against local servers.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
OUT=bench/out
TLS=$(mktemp -d)
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout "$TLS/ca.key" -out "$TLS/ca.pem" \
  -days 2 -subj /CN=bench-ca -addext basicConstraints=critical,CA:TRUE -addext keyUsage=critical,keyCertSign 2>/dev/null
leaf() {
  local name=$1
  shift
  openssl req -newkey "$@" -nodes -keyout "$TLS/$name.key" -out "$TLS/$name.csr" -subj /CN=bench 2>/dev/null
  printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\n' >"$TLS/$name.ext"
  openssl x509 -req -in "$TLS/$name.csr" -CA "$TLS/ca.pem" -CAkey "$TLS/ca.key" -CAcreateserial \
    -out "$TLS/$name.pem" -days 2 -extfile "$TLS/$name.ext" 2>/dev/null
}
leaf ec ec -pkeyopt ec_paramgen_curve:prime256v1
leaf rsa rsa:2048

python3 -B - "$TLS" <<'PY' &
import gzip, http.server, socketserver, ssl, sys
tls = sys.argv[1]
page = ('<!doctype html><html><head><meta name="description" content="bench"></head><body><main>'
        + '<p>Readable prose about the fixture service and its agent surfaces.</p>' * 3000 + '</main></body></html>').encode()
body = gzip.compress(page)
class H(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def log_message(self, *a): pass
    def reply(self):
        length = int(self.headers.get('content-length') or 0)
        if length:
            self.rfile.read(length)
        self.send_response(200)
        self.send_header('content-type', 'text/html')
        self.send_header('content-encoding', 'gzip')
        self.send_header('content-length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    do_GET = do_POST = do_OPTIONS = reply
    def do_HEAD(self):
        self.send_response(200)
        self.send_header('content-length', '0')
        self.end_headers()
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(f'{tls}/ec.pem', f'{tls}/ec.key')
class S(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True
    request_queue_size = 128
    def finish_request(self, request, client_address):
        try:
            request = context.wrap_socket(request, server_side=True)
        except (ssl.SSLError, OSError):
            return
        self.RequestHandlerClass(request, client_address, self)
    def handle_error(self, request, client_address):
        pass
with S(('127.0.0.1', 18555), H) as httpd:
    httpd.serve_forever()
PY
server=$!
openssl s_server -quiet -accept 18443 -key "$TLS/ec.key" -cert "$TLS/ec.pem" -www &
ec=$!
openssl s_server -quiet -accept 18444 -key "$TLS/rsa.key" -cert "$TLS/rsa.pem" -www &
rsa=$!
trap 'kill $server $ec $rsa 2>/dev/null || true' EXIT
for _ in $(seq 50); do
  (exec 3<>/dev/tcp/127.0.0.1/18555) 2>/dev/null && break
  sleep 0.2
done

mapfile -t ENGINES < <(awk 'NF {print $1}' bench/variants)
printf 'engine\tround\twall_s\tuser_s\tsys_s\tcode\n' >"$OUT/local.tsv"
for round in $(seq "${LOCAL_ROUNDS:-40}"); do
  n=${#ENGINES[@]}
  off=$(((round - 1) % n))
  for i in $(seq 0 $((n - 1))); do
    e=${ENGINES[$(((i + off) % n))]}
    /usr/bin/time -f '%e\t%U\t%S' -o "$OUT/time.txt" timeout 60 "bin/$e" web localhost:18555 --ca-cert "$TLS/ca.pem" --output json >/dev/null 2>&1
    code=$?
    printf '%s\t%s\t%s\t%s\n' "$e" "$round" "$(tail -1 "$OUT/time.txt")" "$code" >>"$OUT/local.tsv"
  done
done

cd bench/hs || exit 1
for v in small default; do
  cp Cargo.toml Cargo.toml.orig
  if [ "$v" = small ]; then
    printf '\n[profile.release.package.aws-lc-sys]\nopt-level = "s"\n' >>Cargo.toml
  fi
  CARGO_TARGET_DIR=target-$v cargo build --release 2>&1 | tail -1
  mv Cargo.toml.orig Cargo.toml
done
cd ../..
printf 'target\tround\tvariant\tmedian_us\tp90_us\n' >"$OUT/handshake.tsv"
measure() {
  local label=$1
  shift
  for round in 1 2 3 4 5 6 7; do
    for v in small default; do
      printf '%s\t%s\t%s\t%s\n' "$label" "$round" "$v" "$("bench/hs/target-$v/release/hs" "$@")" >>"$OUT/handshake.tsv"
    done
  done
}
measure local-ec localhost 18443 300 "$TLS/ca.pem"
measure local-rsa localhost 18444 300 "$TLS/ca.pem"
python3 - "$OUT" <<'PY' | tee "$OUT/local-summary.md" >>"${GITHUB_STEP_SUMMARY:-/dev/null}"
import csv, statistics as st, sys
out = sys.argv[1]
rows = list(csv.DictReader(open(f'{out}/local.tsv'), delimiter='\t'))
print('| variant | median wall s | median user s | median sys s | exit codes |')
print('|---|---|---|---|---|')
for e in dict.fromkeys(r['engine'] for r in rows):
    rs = [r for r in rows if r['engine'] == e]
    med = lambda k: st.median(float(r[k]) for r in rs)
    print(f"| {e} | {med('wall_s'):.3f} | {med('user_s'):.3f} | {med('sys_s'):.3f} | {sorted({r['code'] for r in rs})} |")
hs = list(csv.DictReader(open(f'{out}/handshake.tsv'), delimiter='\t'))
print('\n| target | aws-lc at s, us | aws-lc default, us | difference |')
print('|---|---|---|---|')
for t in dict.fromkeys(r['target'] for r in hs):
    m = lambda v: st.median(int(r['median_us']) for r in hs if r['target'] == t and r['variant'] == v)
    print(f"| {t} | {m('small'):.0f} | {m('default'):.0f} | {m('small') - m('default'):.0f} us ({(m('small') - m('default')) / m('default') * 100:.1f}%) |")
PY
