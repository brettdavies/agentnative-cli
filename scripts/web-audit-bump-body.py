#!/usr/bin/env python3
"""Write the body of a web-audit bump PR, in the repo's PR template's shape.

Called by scripts/web-audit-bump.sh with the run's results. Prints to stdout.
"""

import argparse
import json
import os
import re
from collections import defaultdict
from pathlib import Path

SITE = "brettdavies/agentnative-site"
HANDLING = "[Handling a bump PR](../blob/dev/RELEASES.md#handling-a-bump-pr)"
# GitHub refuses a body past 65,536 characters, and a re-pin can touch every
# scenario. Each capped list points at the file or log that holds the rest, and
# MAX_BODY bounds what they add up to.
MAX_BODY = 65_000
CUT_NOTE = "\n\n(Cut at GitHub's body limit; this run's log holds the rest.)"
ENTRY_LINES = 200
FILE_LINES = 60
LOG_LINES = 40
PROBLEM_LINES = 40
LINE_CHARS = 200

FAILED_TEST = re.compile(r"^test (\S+) \.\.\. FAILED$", re.M)
# libtest's closing list, which a line split by another test's output cannot hide.
FAILURES_BLOCK = re.compile(r"^failures:\n\n?((?:    \S+\n)+)", re.M)
LEDGER_PROBLEM = re.compile(r"^\S+/\S+: (?:diverges from the site|now matches the site).*$", re.M)
STAND_DOWN = re.compile(r"standing down: (.*)$", re.M)
ENTRY = re.compile(r'^scenario = "(.*)"\nelement = "(.*)"\nowner = "#(\d+)"$', re.M)


def capped(lines, limit, where):
    shown = [line[:LINE_CHARS] for line in lines[:limit]]
    if len(lines) > limit:
        shown.append(f"- ... and {len(lines) - limit} more; see {where}")
    return shown


def pin_link(sha):
    return f"[`{sha[:7]}`](https://github.com/{SITE}/commit/{sha})"


def ledger_section(ledger_text):
    by_owner = defaultdict(list)
    for scenario, element, owner in ENTRY.findall(ledger_text):
        by_owner[owner].append(f"{scenario}/{element}")
    if not by_owner:
        return ["### Divergences", "", "None: the engine reproduces the whole corpus at this pin."], 0
    total = sum(map(len, by_owner.values()))
    owners = sorted(by_owner, key=lambda o: (-len(by_owner[o]), int(o)))
    out = [
        "### Divergences",
        "",
        f"The ledger holds {total} entries at this pin, each owned by the site PR that set the golden value the engine misses. Port PRs delete the entries they close, and the release gate refuses a release while any remain (`web-audit-ledger`, see {HANDLING}).",
        "",
        "| Site PR | Entries |",
        "| ------- | ------: |",
    ]
    out += [f"| {SITE}#{o} | {len(by_owner[o])} |" for o in owners]
    entries = [f"- {SITE}#{o}: `{e}`" for o in owners for e in by_owner[o]]
    out += ["", "<details><summary>Entries</summary>", ""]
    out += capped(entries, ENTRY_LINES, "`tests/fixtures/web-audit-divergences.toml`")
    out += ["", "</details>"]
    return out, total


def unported_section(path):
    try:
        unported = json.loads(Path(path).read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return ["### Unported handler kinds and eval rules", "", "Not read: the registry did not build at this pin."]
    rows = [
        f"- {label} `{name}`: " + ", ".join(f"`{c}`" for c in checks)
        for label, table in (("handler", unported["handlers"]), ("eval", unported["evals"]))
        for name, checks in sorted(table.items())
    ]
    if not rows:
        return ["### Unported handler kinds and eval rules", "", "None: this build runs every check."]
    return [
        "### Unported handler kinds and eval rules",
        "",
        f"Port work owed. These checks skip wherever they apply, so a full `anc web` run there reads could not check, and the release gate refuses a release until they run (`web-audit-unported`, see {HANDLING}):",
        "",
    ] + capped(rows, PROBLEM_LINES, "`anc emit web-checks`")


def suite_section(args):
    log = Path(args.suite_log).read_text(errors="replace")
    stood_down = sorted(set(STAND_DOWN.findall(log)))
    gave_up = []
    if stood_down:
        gave_up = ["Tests that gave up a verdict:", ""]
        gave_up += capped([f"- `{line}`" for line in stood_down], PROBLEM_LINES, "this run's log")
        gave_up.append("")
    if args.suite_status == 0 and args.ledger_status == 0:
        return ["### Web suites", "", "Passing at this pin, with the ledger above.", ""] + gave_up
    out = ["### Web suites", ""] + gave_up
    if args.ledger_status != 0:
        ledger_log = Path(args.ledger_log).read_text(errors="replace").splitlines()
        out += ["The ledger did not regenerate, so the committed one stands:", "", "```text"]
        out += [line[:LINE_CHARS] for line in ledger_log[-LOG_LINES:]] + ["```", ""]
    failed = sorted(
        set(FAILED_TEST.findall(log))
        | {name.strip() for block in FAILURES_BLOCK.findall(log) for name in block.splitlines()}
    )
    problems = LEDGER_PROBLEM.findall(log)
    if failed:
        out += ["Failing tests:", ""]
        out += capped([f"- `{t}`" for t in failed], PROBLEM_LINES, "this run's log")
        out.append("")
    if problems:
        out += ["Failing scenarios:", ""]
        out += capped([f"- `{p}`" for p in sorted(set(problems))], PROBLEM_LINES, "this run's log")
        out.append("")
    if args.suite_status != 0 and not failed:
        heading = "The suites failed without naming a test:" if "test result:" in log else "The suites did not build:"
        out += [heading, "", "```text"]
        out += [line[:LINE_CHARS] for line in log.splitlines()[-LOG_LINES:]] + ["```", ""]
    return out


def fit_the_limit(report, template):
    """The report and the template below it, cutting the report when both won't fit."""
    text = "\n".join(report)
    rest = "\n".join(template)
    room = MAX_BODY - len(rest) - len(CUT_NOTE) - len("\n```\n\n</details>") - 2
    if len(text) > room:
        text = text[:room]
        # Whatever the cut left open would swallow the template below it.
        if text.count("```") % 2:
            text += "\n```"
        if text.count("<details>") > text.count("</details>"):
            text += "\n\n</details>"
        text += CUT_NOTE
    return f"{text}\n\n{rest}"


def count_of_vendored(paths):
    if not paths:
        return "None."
    noun = "file" if len(paths) == 1 else "files"
    return f"{len(paths)} vendored {noun}, listed under Vendored files above."


def changelog_bullet(changed, new):
    if any(re.search(r"(^|/)(registry|remediation|constants)\.", f) for f in changed):
        return f"- Update `anc web`'s checks, fixes and settings to match agentnative-site `{new[:7]}`."
    return f"- Update `anc web`'s reported site pin to agentnative-site `{new[:7]}`."


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    for flag in ("old", "new", "changed", "created", "deleted", "ledger", "unported", "ledger-log", "suite-log"):
        parser.add_argument(f"--{flag}", required=True)
    for flag in ("ledger-status", "suite-status"):
        parser.add_argument(f"--{flag}", required=True, type=int)
    args = parser.parse_args()

    changed = Path(args.changed).read_text().splitlines()
    created = Path(args.created).read_text().splitlines()
    divergences, total = ledger_section(Path(args.ledger).read_text())
    green = args.suite_status == 0 and args.ledger_status == 0
    run_url = ""
    if os.environ.get("GITHUB_RUN_ID"):
        run_url = f"{os.environ['GITHUB_SERVER_URL']}/{os.environ['GITHUB_REPOSITORY']}/actions/runs/{os.environ['GITHUB_RUN_ID']}"

    body = [
        "## Summary",
        "",
        f"Moves the vendored web-audit inputs from agentnative-site {pin_link(args.old)} to {pin_link(args.new)}. `scripts/web-audit-bump.sh` opened this PR and updates it on each run; push port work as new commits on this branch and the bot stands down.",
        "",
        f"Site changes: https://github.com/{SITE}/compare/{args.old[:7]}...{args.new[:7]}",
    ]
    if run_url:
        body += ["", f"Run: {run_url}"]
    body += ["", "### Vendored files", ""]
    if changed:
        body += capped([f"- `{f}`" for f in changed], FILE_LINES, "this PR's Files tab")
    else:
        body.append(f"None: the bytes match. `{args.old[:7]}` is not on site dev, so the pin moves onto dev before that commit can vanish from the site's history.")
    deleted = Path(args.deleted).read_text().splitlines()
    if deleted:
        body += [
            "",
            f"**{len(deleted)} of these are deleted.** A site change that removes scenarios shrinks the parity evidence, and no test can see that, so read this list before merging:",
            "",
        ]
        body += capped([f"- `{f}`" for f in deleted], FILE_LINES, "this PR's Files tab")
    body += [""] + divergences + [""] + unported_section(args.unported) + [""] + suite_section(args)
    template = [
        "## Changelog",
        "",
        "### Changed",
        "",
        changelog_bullet(changed, args.new),
        "",
        "## Type of Change",
        "",
        "- [x] `feat`: New feature (non-breaking change which adds functionality)",
        "",
        "## Related Issues/Stories",
        "",
        "- Story: n/a",
        "- Issue: n/a",
        "- Architecture: `scripts/SYNCS.md`",
        "- Related PRs: n/a",
        "",
        "## Testing",
        "",
        f"- [{'x' if green else ' '}] All tests passing: `cargo test --features web` over the web suites" + (f", with {total} ledgered divergences" if total else ""),
        "",
        "## Files Modified",
        "",
        "**Modified:** the vendored files above, `src/web_audit/vendored/SITE_SHA`, and `tests/fixtures/web-audit-divergences.toml` when its entries moved.",
        "",
        f"**Created:** {count_of_vendored(created)}",
        "",
        "**Renamed:** None.",
        "",
        f"**Deleted:** {count_of_vendored(deleted)}",
        "",
        "## Breaking Changes",
        "",
        "- [x] No breaking changes",
        "",
        "## Deployment Notes",
        "",
        "- [x] No special deployment steps required",
    ]
    print(fit_the_limit(body, template))


if __name__ == "__main__":
    main()
