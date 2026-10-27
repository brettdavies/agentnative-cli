#!/usr/bin/env python3
"""Regenerate tests/fixtures/web-audit-divergences.toml.

Replays the vendored conformance corpus through the engine
(tests/web_audit_divergences.rs) and records every element that differs from
its golden. Each element is owned by the newest site commit between
--since and the vendored pin (src/web_audit/vendored/SITE_SHA) that changed
that element of the golden; the owner is the PR number in that commit's
subject. An element no commit in the range changed is owned by the newest
commit that touched its scenario.

usage: WEB_AUDIT_SITE_REPO=<agentnative-site checkout> \\
         scripts/web-audit-divergence-ledger.py --since <old site sha>
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "tests/fixtures/web-audit-divergences.toml"
CORPUS = "tests/fixtures/web-audit-conformance/scenarios"
HEADER = """\
# Web-audit divergence ledger: every element of the vendored conformance
# corpus the engine does not yet reproduce at site_sha, one entry per
# (scenario, element). An element is a top-level scorecard field,
# `row:<check id>`, `unreachable`, or `document` (bytes differ, fields match).
# `owner` is the site PR that set the golden value the engine misses.
#
# The conformance and handlers suites assert the observed divergences EQUAL
# this ledger: an unledgered divergence fails, and an entry that starts
# matching fails until it is removed, so each port PR deletes the entries
# it closes. Regenerate after a re-pin, never by hand:
#   WEB_AUDIT_SITE_REPO=<site checkout> scripts/web-audit-divergence-ledger.py --since <old pin>
"""


def git(repo, *args):
    return subprocess.run(
        ["git", "-C", str(repo), *args], check=True, capture_output=True, text=True
    ).stdout


def golden_at(repo, rev, scenario, cache):
    key = (rev, scenario)
    if key not in cache:
        try:
            text = git(repo, "show", f"{rev}:{CORPUS}/{scenario}/scorecard.json")
            cache[key] = json.loads(text)
        except subprocess.CalledProcessError:
            cache[key] = None
    return cache[key]


def element_of(doc, element):
    if doc is None:
        return ("missing",)
    if element in ("unreachable", "document"):
        return json.dumps(doc, sort_keys=True)
    if element.startswith("row:"):
        rid = element[len("row:"):]
        rows = [r for r in doc.get("results", []) if r.get("id") == rid]
        return json.dumps(rows, sort_keys=True)
    return json.dumps(doc.get(element, ("absent",)), sort_keys=True)


def pr_of(subject):
    found = re.findall(r"\(#(\d+)\)", subject)
    if not found:
        sys.exit(f"error: site commit subject names no PR: {subject!r}")
    return f"#{found[-1]}"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--since", required=True, help="the previous site pin")
    args = parser.parse_args()
    repo = os.environ.get("WEB_AUDIT_SITE_REPO")
    if not repo:
        sys.exit("error: set WEB_AUDIT_SITE_REPO to an agentnative-site checkout")
    pin = (ROOT / "src/web_audit/vendored/SITE_SHA").read_text().strip()

    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "divergences.json"
        env = dict(os.environ, WEB_AUDIT_DIVERGENCE_OUT=str(out))
        subprocess.run(
            ["cargo", "test", "--quiet", "--features", "web", "--test", "web_audit_divergences"],
            cwd=ROOT, env=env, check=True,
        )
        pairs = json.loads(out.read_text())

    log = git(repo, "log", "--reverse", "--format=%H %s", f"{args.since}..{pin}", "--", CORPUS)
    commits = [line.split(" ", 1) for line in log.splitlines() if line]
    cache = {}
    entries = []
    for scenario, element in sorted(map(tuple, pairs)):
        owner = None
        latest_touch = None
        for sha, subject in commits:
            before = golden_at(repo, f"{sha}^", scenario, cache)
            after = golden_at(repo, sha, scenario, cache)
            if before != after:
                latest_touch = subject
            if element_of(before, element) != element_of(after, element):
                owner = subject
        subject = owner or latest_touch
        if subject is None:
            sys.exit(f"error: no site commit in range touched {scenario}; widen --since")
        entries.append((scenario, element, pr_of(subject)))

    lines = [HEADER, f'site_sha = "{pin}"\n']
    for scenario, element, owner in entries:
        lines.append(
            f'\n[[divergence]]\nscenario = "{scenario}"\nelement = "{element}"\nowner = "{owner}"\n'
        )
    LEDGER.write_text("".join(lines))
    print(f"wrote {LEDGER.relative_to(ROOT)}: {len(entries)} entries")


if __name__ == "__main__":
    main()
