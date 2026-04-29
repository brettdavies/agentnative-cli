#!/bin/sh
# Hostile fixture: exits nonzero on every probe variant the scorecard
# tries (`--version`, `-V`). Verifies the fall-through contract — when
# every version-probe attempt fails, tool.version must be null and the
# overall run must succeed (a target that refuses to self-report is not
# a scoring error).
case "$1" in
    --help|-h)
        echo "hostile-nonzero-exit: --help works, version probes fail"
        exit 0
        ;;
    --version|-V)
        echo "version probe rejected" >&2
        exit 1
        ;;
    *)
        echo "hostile-nonzero-exit"
        exit 0
        ;;
esac
