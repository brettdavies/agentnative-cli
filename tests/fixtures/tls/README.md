# TLS test fixture

A self-signed end-entity certificate and key for the in-test rustls server in `tests/web_audit_transport.rs`. The
bundled Mozilla roots reject the chain, which is the behavior the test pins: an unknown issuer must surface as the
bundled-root-rejection evidence class, not as a generic TLS error.

The certificate is a leaf (`CA:FALSE`, `serverAuth`), the shape a private CA issues to an internal host, so the verifier
reaches the issuer lookup and reports the unknown issuer. A certificate minted with `CA:TRUE`, which is `openssl req
-x509`'s default, is refused earlier as a CA used as an end entity and never exercises that class.

The key is test-only material and secures nothing. Expiry is a century out so the fixture never rots. Regenerate both
files with:

```bash
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
  -keyout localhost.key.pem -out localhost.cert.pem -days 36500 \
  -subj "/CN=anc-web-audit self-signed test" \
  -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "extendedKeyUsage=serverAuth"
```

## Private-CA fixtures

A test CA (`ca.cert.pem`) and a leaf it signed for `localhost` and `127.0.0.1` (`ca-signed.cert.pem` and
`ca-signed.key.pem`), for the HTTPS test server in `tests/common/mod.rs` (`spawn_https`). No root store the tests use
holds the CA, so a run rejects the chain until `--ca-cert tests/fixtures/tls/ca.cert.pem`, or an OS store that holds it,
vouches for it. The CA key is discarded after signing; regenerating mints a new CA and leaf together:

```bash
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
  -keyout ca.key.pem -out ca.cert.pem -days 36500 \
  -subj "/CN=anc-web-audit test CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign"
openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
  -keyout ca-signed.key.pem -out leaf.csr -subj "/CN=anc-web-audit CA-signed test"
printf 'subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\n' >leaf.ext
openssl x509 -req -in leaf.csr -CA ca.cert.pem -CAkey ca.key.pem -CAcreateserial \
  -out ca-signed.cert.pem -days 36500 -extfile leaf.ext
rm ca.key.pem ca.cert.srl leaf.csr leaf.ext
```
