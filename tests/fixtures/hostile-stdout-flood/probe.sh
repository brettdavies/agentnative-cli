#!/bin/sh
# Hostile fixture: floods stdout with ~2 MiB on `--version` to verify the
# scorecard's tool.version probe survives a binary that ignores reasonable
# output sizes. The runner's read_capped() primitive enforces a 1 MiB
# ceiling — anc must complete the run without exhausting memory or
# panicking, regardless of what the captured first line looks like.
case "$1" in
    --version|-V)
        # Emit ~2 MiB of 'x' across many lines so the first line is
        # bounded but total output exceeds the cap.
        i=0
        while [ "$i" -lt 32768 ]; do
            printf 'xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n'
            i=$((i + 1))
        done
        exit 0
        ;;
    --help|-h)
        echo "hostile-stdout-flood: probe --version to trigger flood"
        exit 0
        ;;
    *)
        echo "hostile-stdout-flood"
        exit 0
        ;;
esac
