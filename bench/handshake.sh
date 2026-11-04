#!/usr/bin/env bash
# Handshake time at each aws-lc-sys opt-level, against a local server (crypto only) and real hosts.
set -euo pipefail
cd "$(dirname "$0")/hs" || exit 1
for v in small default; do
  cp Cargo.toml Cargo.toml.orig
  if [ "$v" = small ]; then
    printf '\n[profile.release.package.aws-lc-sys]\nopt-level = "s"\n' >>Cargo.toml
  fi
  CARGO_TARGET_DIR=target-$v cargo build --release
  mv Cargo.toml.orig Cargo.toml
done
cd ..
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -keyout ec.key -out ec.pem -days 2 -subj /CN=localhost -addext subjectAltName=DNS:localhost 2>/dev/null
openssl req -x509 -newkey rsa:2048 -nodes -keyout rsa.key -out rsa.pem -days 2 -subj /CN=localhost -addext subjectAltName=DNS:localhost 2>/dev/null
openssl s_server -quiet -accept 18443 -key ec.key -cert ec.pem -www &
ec=$!
openssl s_server -quiet -accept 18444 -key rsa.key -cert rsa.pem -www &
rsa=$!
trap 'kill $ec $rsa 2>/dev/null || true' EXIT
for _ in $(seq 50); do
  (exec 3<>/dev/tcp/127.0.0.1/18444) 2>/dev/null && break
  sleep 0.2
done
printf 'target\tround\tvariant\tmedian_us\tp90_us\n' >out/handshake.tsv
measure() {
  local label=$1
  shift
  for round in 1 2 3 4 5; do
    for v in small default; do
      printf '%s\t%s\t%s\t%s\n' "$label" "$round" "$v" "$("hs/target-$v/release/hs" "$@")" >>out/handshake.tsv
    done
  done
}
measure local-ec localhost 18443 200 ec.pem
measure local-rsa localhost 18444 200 rsa.pem
measure anc.dev anc.dev 443 20
measure github.com github.com 443 20
measure vercel.com vercel.com 443 20
cat out/handshake.tsv
