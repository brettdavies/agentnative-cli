"""aws-lc-sys at the release profile's default opt-level."""
import pathlib

p = pathlib.Path('Cargo.toml')
s = p.read_text()
old = '[profile.release.package.aws-lc-sys]\nopt-level = "s"\n'
assert old in s
p.write_text(s.replace(old, '', 1))
