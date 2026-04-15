#!/usr/bin/env bash
# Generate or update CHANGELOG.md using git-cliff with PR body expansion.
#
# Usage:
#   generate-changelog.sh [--tag vX.Y.Z] [repo-path]
#   generate-changelog.sh --check [repo-path]
#
# Options:
#   --tag vX.Y.Z   Override version tag (default: extracted from branch name)
#   --check        Verify CHANGELOG.md has a versioned section (exit 1 if only [Unreleased])
#
# The version tag is extracted from the branch name by matching the pattern
# release/vN.N.N (with optional suffix like release/v1.0.5:ci-migration).
# Use --tag to override when not on a release branch.
#
# Generates entries for commits since the last tag, prepends to existing
# CHANGELOG.md, then expands squash commit entries by fetching categorized
# changelog sections (### Added, ### Changed, ### Fixed, ### Documentation)
# from each PR body's ## Changelog section.
#
# Falls back to ## Changes (flat list) for PRs using the old template.
#
# Run this on a release branch before opening a PR to main.

set -euo pipefail

CHECK_MODE=false
REPO_PATH="."
TAG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --check)
      CHECK_MODE=true
      shift
      ;;
    --tag)
      TAG="$2"
      shift 2
      ;;
    *)
      REPO_PATH="$1"
      shift
      ;;
  esac
done

cd "$REPO_PATH"

# Verify prerequisites
if [[ ! -f cliff.toml ]]; then
  echo "error: cliff.toml not found in $(pwd)" >&2
  exit 1
fi

if ! command -v git-cliff &>/dev/null; then
  echo "error: git-cliff is not installed" >&2
  echo "  Install: cargo install git-cliff" >&2
  echo "  Or:      brew install git-cliff" >&2
  exit 1
fi

if $CHECK_MODE; then
  if [[ ! -f CHANGELOG.md ]]; then
    echo "FAIL: CHANGELOG.md does not exist" >&2
    exit 1
  fi

  # Check for a versioned section (not just [Unreleased])
  LATEST_SECTION=$(awk '/^## \[/{print; exit}' CHANGELOG.md)
  if echo "$LATEST_SECTION" | grep -q '\[Unreleased\]'; then
    echo "FAIL: CHANGELOG.md has [Unreleased] instead of a versioned section" >&2
    echo "Run: generate-changelog.sh (on a release/vX.Y.Z branch)" >&2
    exit 1
  fi

  echo "OK: CHANGELOG.md has versioned section"
  exit 0
fi

# Extract version from branch name if --tag not provided
if [[ -z "$TAG" ]]; then
  BRANCH=$(git branch --show-current 2>/dev/null || true)
  if [[ "$BRANCH" =~ ^release/v([0-9]+\.[0-9]+\.[0-9]+) ]]; then
    TAG="v${BASH_REMATCH[1]}"
    echo "Detected version $TAG from branch $BRANCH"
  else
    echo "error: could not detect version from branch '$BRANCH'" >&2
    echo "Either use a release/vX.Y.Z branch or pass --tag vX.Y.Z" >&2
    exit 1
  fi
fi

# Ensure GitHub token is available for remote integration (PR links, authors)
if [[ -z "${GITHUB_TOKEN:-}" ]]; then
  if command -v gh &>/dev/null && gh auth status &>/dev/null 2>&1; then
    export GITHUB_TOKEN
    GITHUB_TOKEN=$(gh auth token)
  fi
fi

# Step 1: Run git-cliff to prepend entries tagged with the release version
CLIFF_ARGS=(--unreleased --tag "$TAG")
if [[ -f CHANGELOG.md ]]; then
  CLIFF_ARGS+=(--prepend CHANGELOG.md)
else
  CLIFF_ARGS+=(-o CHANGELOG.md)
fi
git cliff "${CLIFF_ARGS[@]}"

# Step 2: Expand squash commit entries using PR body changelog sections
OWNER=$(awk -F'"' '/^\[remote\.github\]/{found=1} found && /^owner/{print $2; exit}' cliff.toml)
REPO=$(awk -F'"' '/^\[remote\.github\]/{found=1} found && /^repo/{print $2; exit}' cliff.toml)

# Strip leading v for version matching in the changelog
VERSION="${TAG#v}"

if [[ -z "$OWNER" || -z "$REPO" ]] || ! command -v gh &>/dev/null; then
  echo "Updated CHANGELOG.md (skipping PR expansion — missing [remote.github] or gh CLI)"
  echo ""
  echo "Next steps:"
  echo "  git add CHANGELOG.md"
  echo "  git commit -m 'docs: update CHANGELOG.md'"
  exit 0
fi

# Extract PR numbers from the new version section only
VERSION_SECTION=$(awk -v ver="$VERSION" '
  /^## \[/{
    if (found) exit
    if (index($0, "[" ver "]")) found=1
  }
  found{print}
' CHANGELOG.md)
PR_NUMBERS=$(echo "$VERSION_SECTION" | grep -oP '\(#\K\d+' | sort -un)

if [[ -z "$PR_NUMBERS" ]]; then
  echo "Updated CHANGELOG.md"
  echo ""
  echo "Next steps:"
  echo "  git add CHANGELOG.md"
  echo "  git commit -m 'docs: update CHANGELOG.md'"
  exit 0
fi

# Pass PR numbers as comma-separated arg to python
PR_LIST=$(echo "$PR_NUMBERS" | tr '\n' ',' | sed 's/,$//')

python3 - "$OWNER" "$REPO" "$PR_LIST" "CHANGELOG.md" "$VERSION" "$TAG" << 'PYEOF'
import json, re, subprocess, sys

owner = sys.argv[1]
repo = sys.argv[2]
pr_numbers = [int(n) for n in sys.argv[3].split(',')]
changelog_path = sys.argv[4]
version = sys.argv[5]
tag = sys.argv[6] if len(sys.argv) > 6 else f'v{version}'
tag_prefix = 'v' if tag.startswith('v') else ''

CATEGORIES = ['Added', 'Changed', 'Fixed', 'Documentation']

def fetch_pr(num):
    """Fetch PR body and author from GitHub API."""
    try:
        result = subprocess.run(
            ['gh', 'api', f'repos/{owner}/{repo}/pulls/{num}',
             '--jq', '{body: .body, author: .user.login}'],
            capture_output=True, text=True, timeout=10
        )
        if result.returncode == 0:
            return json.loads(result.stdout)
    except Exception:
        pass
    return None

def extract_changelog_sections(body):
    """Extract categorized bullets from ## Changelog section with ### subsections."""
    sections = {}
    if not body:
        return sections

    changelog_match = re.search(r'^## Changelog\s*$', body, re.MULTILINE)
    if not changelog_match:
        return sections

    rest = body[changelog_match.end():]
    next_h2 = re.search(r'^## ', rest, re.MULTILINE)
    changelog_content = rest[:next_h2.start()] if next_h2 else rest

    current_section = None
    for line in changelog_content.split('\n'):
        h3_match = re.match(r'^### (.+)', line)
        if h3_match:
            current_section = h3_match.group(1).strip()
            if current_section not in sections:
                sections[current_section] = []
        elif current_section and re.match(r'^- ', line):
            sections[current_section].append(line)
        elif current_section and sections.get(current_section) and re.match(r'^  \S', line):
            # Continuation line (indented, part of previous bullet) — join to last bullet
            sections[current_section][-1] = sections[current_section][-1].rstrip() + ' ' + line.strip()

    return sections

def extract_flat_changes(body):
    """Fallback: extract flat bullet list from ## Changes section."""
    bullets = []
    if not body:
        return bullets

    changes_match = re.search(r'^## Changes\s*$', body, re.MULTILINE)
    if not changes_match:
        return bullets

    rest = body[changes_match.end():]
    next_h2 = re.search(r'^## ', rest, re.MULTILINE)
    changes_content = rest[:next_h2.start()] if next_h2 else rest

    for line in changes_content.split('\n'):
        if re.match(r'^- ', line):
            bullets.append(line)
        elif bullets and re.match(r'^  \S', line):
            # Continuation line (indented, part of previous bullet) — join to last bullet
            bullets[-1] = bullets[-1].rstrip() + ' ' + line.strip()

    return bullets

# Collect all categorized entries from PR bodies
all_entries = {}  # category -> list of bullets
for num in pr_numbers:
    pr_data = fetch_pr(num)
    if not pr_data:
        continue

    body = pr_data.get('body', '') or ''
    author = pr_data.get('author', '')
    attrib = f' by @{author} in [#{num}](https://github.com/{owner}/{repo}/pull/{num})' if author else ''

    # Try new template format first (## Changelog with ### subsections)
    sections = extract_changelog_sections(body)

    if sections:
        for category, bullets in sections.items():
            if not bullets:
                continue
            if category not in all_entries:
                all_entries[category] = []
            first = True
            for bullet in bullets:
                if first and ' by @' not in bullet:
                    all_entries[category].append(bullet + attrib)
                else:
                    all_entries[category].append(bullet)
                first = False
    else:
        # Fallback: flat ## Changes section
        flat = extract_flat_changes(body)
        if flat:
            category = 'Changed'
            if category not in all_entries:
                all_entries[category] = []
            first = True
            for bullet in flat:
                if first and ' by @' not in bullet:
                    all_entries[category].append(bullet + attrib)
                else:
                    all_entries[category].append(bullet)
                first = False

if not all_entries:
    sys.exit(0)

# Read the changelog
with open(changelog_path, 'r') as f:
    content = f.read()

# Find the version section header line (preserve it with the date)
header_pattern = rf'^## \[{re.escape(version)}\].*$'
header_match = re.search(header_pattern, content, re.MULTILINE)
if not header_match:
    sys.exit(0)

header_line = header_match.group(0)

# Build the new version section content
new_section = header_line + '\n'
for cat in CATEGORIES:
    if cat in all_entries and all_entries[cat]:
        new_section += f'\n### {cat}\n\n'
        for bullet in all_entries[cat]:
            new_section += bullet + '\n'

# Include any categories not in the standard list
for cat in all_entries:
    if cat not in CATEGORIES and all_entries[cat]:
        new_section += f'\n### {cat}\n\n'
        for bullet in all_entries[cat]:
            new_section += bullet + '\n'

# Find the previous version tag for the Full Changelog link
prev_match = re.search(rf'## \[{re.escape(version)}\].*?\n## \[([^\]]+)\]', content, re.DOTALL)
if prev_match:
    prev_version = prev_match.group(1)
    new_section += f'\n**Full Changelog**: [{tag_prefix}{prev_version}...{tag_prefix}{version}](https://github.com/{owner}/{repo}/compare/{tag_prefix}{prev_version}...{tag_prefix}{version})\n'

# Replace the version section in the file
section_pattern = rf'## \[{re.escape(version)}\].*?(?=\n## \[|\Z)'
new_content = re.sub(section_pattern, new_section.rstrip() + '\n', content, count=1, flags=re.DOTALL)

with open(changelog_path, 'w') as f:
    f.write(new_content)
PYEOF

echo "Updated CHANGELOG.md"
echo ""
echo "Next steps:"
echo "  git add CHANGELOG.md"
echo "  git commit -m 'docs: update CHANGELOG.md'"
