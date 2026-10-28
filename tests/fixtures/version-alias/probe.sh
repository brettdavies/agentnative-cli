#!/bin/sh
# Fixture: `--version` fails and `--help` declares `-v` as the version flag,
# so the scorecard's tool.version can come only from `-v`. Appends every first
# argument to $ANC_TEST_ARGV_LOG, so a test can check that `-V` never ran.
echo "$1" >> "$ANC_TEST_ARGV_LOG"
case "$1" in
    --help|-h)
        cat <<'HELP'
Usage: version-alias [OPTIONS]

Options:
  -v, --version  Print version
  -h, --help     Print help
HELP
        exit 0
        ;;
    -v)
        echo "version-alias 2.0.0"
        exit 0
        ;;
    -V)
        echo "an undeclared alias ran"
        exit 0
        ;;
    *)
        echo "version-alias: unknown argument $1" >&2
        exit 1
        ;;
esac
