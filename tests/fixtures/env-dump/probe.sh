#!/bin/sh
# Fixture: appends the environment it starts with to $ANC_TEST_ENV_DUMP, so
# a test can read what an audited binary inherits from anc.
env >> "$ANC_TEST_ENV_DUMP"
case "$1" in
    --help|-h)
        echo "usage: probe [--help] [--version]"
        exit 0
        ;;
    --version|-V)
        echo "probe 1.0.0"
        exit 0
        ;;
    *)
        echo "probe"
        exit 0
        ;;
esac
