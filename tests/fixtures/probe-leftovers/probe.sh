#!/bin/sh
# Fixture: starts a process that outlives this script, then exits, or with
# ANC_TEST_PROBE_HANG set, hangs. Appends the leftover's pid and its own to
# $ANC_TEST_PID_FILE, so a test can check whether the probe's kill reached
# both. The leftover holds no pipe of the probe's, so a runner that does not
# kill it returns at once and leaves it running.
sleep 300 >/dev/null 2>&1 &
echo "$!" >> "$ANC_TEST_PID_FILE"
echo "$$" >> "$ANC_TEST_PID_FILE"
if [ -n "$ANC_TEST_PROBE_HANG" ]; then
    exec sleep 300
fi
echo "probe-leftovers"
exit 0
