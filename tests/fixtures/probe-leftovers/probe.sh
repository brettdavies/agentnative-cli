#!/bin/sh
# Fixture: starts a process that outlives this script, then exits, or with
# ANC_TEST_PROBE_HANG set, hangs. Appends the leftover's pid and its own to
# $ANC_TEST_PID_FILE, so a test can check whether the probe's kill reached
# both. The leftover holds no pipe of the probe's, so a runner that does not
# kill it returns at once and leaves it running. With ANC_TEST_PROBE_SETSID
# set, the leftover leaves the probe's session and process group, as a
# detached GUI app does.
if [ -n "$ANC_TEST_PROBE_SETSID" ]; then
    setsid sleep 300 >/dev/null 2>&1 &
else
    sleep 300 >/dev/null 2>&1 &
fi
echo "$!" >> "$ANC_TEST_PID_FILE"
echo "$$" >> "$ANC_TEST_PID_FILE"
if [ -n "$ANC_TEST_PROBE_HANG" ]; then
    exec sleep 300
fi
echo "probe-leftovers"
exit 0
