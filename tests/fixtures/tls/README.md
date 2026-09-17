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
