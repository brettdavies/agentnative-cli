#!/bin/sh
# Hostile fixture: hangs on `--version` to verify the scorecard probe's
# 2-second timeout actually fires. probe_tool_version() builds a fresh
# BinaryRunner with Duration::from_secs(2); without that bound, anc would
# wait the full sleep duration on a wedged target. We sleep 30s so the
# timeout's effect is unambiguous: a successful guard returns in ~2-3s.
case "$1" in
    --version|-V)
        # `exec` replaces the shell with `sleep` so SIGKILL on the spawned
        # PID kills sleep directly. Without exec, the shell forks sleep as
        # a child; SIGKILL on the shell leaves an orphan sleep holding the
        # inherited stdout/stderr pipes, which blocks the runner's reader
        # threads until sleep finishes. That defeats the timeout's purpose
        # and inflates total runtime to ~90s instead of ~4s.
        exec sleep 30
        ;;
    --help|-h)
        echo "hostile-hang: probe --version to trigger hang"
        exit 0
        ;;
    *)
        echo "hostile-hang"
        exit 0
        ;;
esac
