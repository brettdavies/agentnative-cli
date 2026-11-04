"""The web-only crates at opt-level "s"."""
import pathlib

CRATES = """adler2 aws-lc-rs base64 bytes crc32fast flate2 form_urlencoded http httparse icu_collections
icu_locale_core icu_normalizer icu_normalizer_data icu_properties icu_properties_data icu_provider idna
idna_adapter litemap miniz_oxide percent-encoding potential_utf rustls rustls-native-certs rustls-pki-types
rustls-webpki simd-adler32 smallvec subtle tinystr untrusted ureq ureq-proto url utf8-zero utf8_iter
webpki-roots writeable yoke zerofrom zerotrie zerovec zeroize""".split()

p = pathlib.Path('Cargo.toml')
s = p.read_text().rstrip('\n')
s += '\n\n[profile.release.package]\n' + ''.join(f'{c}.opt-level = "s"\n' for c in CRATES)
p.write_text(s)
