// The one owner of every audit-funnel path, the target classifier, target
// normalization, and the reserved names under the result namespace.
//
// Imported by three module graphs with incompatible type environments:
// the Worker (Workers types, no DOM), the browser client (DOM, no Workers
// or Bun globals), and the Bun build. Only bare-string constants and pure
// functions over strings live here, so all three accept it.
//
// Classification order (the target's shape is authoritative; the server
// re-runs it on every request, the client's copy is a convenience):
//
//   raw input
//     |-- longer than TARGET_MAX_LENGTH ........... rejected, before any parsing
//     |-- empty after trim ........................ rejected
//     |-- GitHub repo (URL or owner/repo) ......... CLI, target as entered
//     |     '-- /tree/<branch> or owner/repo@branch  CLI branch-scoped,
//     |           target owner/repo@branch; a branch whose last segment is
//     |           md, json, or html is rejected
//     |-- any other scheme://... URL .............. website, target = host
//     |     (http and https only; a host that would not re-classify as a
//     |      website on its own is rejected so normalization is idempotent)
//     |-- IP literal, localhost, or dotted host ... website, target = host
//     |     (parsed through the URL parser: lowercase, punycode, port kept,
//     |      path and query dropped)
//     |-- a reserved name ......................... rejected
//     '-- anything else (slug, install command) ... CLI, target as entered
//
// Result paths select their representation with a trailing segment
// (`/score/<target>/md`, `/score/<target>/json`), never an extension, so a
// host under a TLD such as `.md` or `.map` is an ordinary target and no
// extension is parsed under `/score/`.

export type Lane = 'cli' | 'web';
export type Representation = 'html' | 'md' | 'json';

export const TARGET_MAX_LENGTH = 128;

/** Top-level page names a target can never shadow. */
export const RESERVED_NAMES: readonly string[] = ['scoring', 'scorecards', 'audit', 'fix', 'api'];

/** Trailing segments the result route reads as a representation. */
export const RESERVED_REPRESENTATION_SEGMENTS: readonly string[] = ['md', 'json', 'html'];

export const SCORING_PATH = '/scoring';
export const SCORE_PREFIX = '/score/';
export const SCORECARDS_PATH = '/scorecards';
export const AUDIT_PATH = '/audit';
export const FIX_PREFIX = '/fix/';
export const API_SCORE_PATH = '/api/score';

/**
 * Retired paths that answer a redirect instead of a 404, and where each one
 * goes. A path earns a row here when it was linked from outside the site and
 * has an exact destination on the new funnel; everything else the funnel
 * retired is a 404, because a redirect to an approximation is worse than a
 * clear miss. Owned here so the paths stay in the one module that knows them
 * and the literal guard keeps its zero.
 */
export const RETIRED_REDIRECTS: Readonly<Record<string, string>> = {
  '/web-audit': `${AUDIT_PATH}?lane=web`,
  '/web-audit.md': `${AUDIT_PATH}.md`,
};

/** The retired website result path; its host is the target the result route serves. */
const RETIRED_WEB_RESULT_RE = /^\/web\/([^/]+?)(\.md)?$/;

/**
 * The destination a retired path redirects to, or null when it is a 404.
 *
 * The website result path carries its target in the URL, so it resolves by
 * shape rather than by table: a host the result route can serve redirects to
 * that result, and anything else under `/web/` (a reserved name, a second
 * segment, a target the classifier refuses) is a 404 rather than a redirect
 * into a page that would 404 anyway.
 */
export function retiredRedirectFor(pathname: string): string | null {
  if (Object.hasOwn(RETIRED_REDIRECTS, pathname)) return RETIRED_REDIRECTS[pathname];
  const match = RETIRED_WEB_RESULT_RE.exec(pathname);
  if (!match) return null;
  const raw = decodeURIComponentSafe(match[1]);
  if (raw === null || laneOf(raw) !== 'web') return null;
  // Normalize before building the destination. A host that differs from its
  // canonical form only in case or encoding would otherwise land on a result
  // path that immediately redirects again, and one 301 is the whole point.
  const host = normalizeTarget(raw);
  if (host === null || !isResultTarget(host)) return null;
  return match[2] ? scoreMarkdownPath(host) : scorePath(host);
}

function decodeURIComponentSafe(value: string): string | null {
  try {
    return decodeURIComponent(value);
  } catch {
    return null;
  }
}

export type TargetRejection =
  | 'target_empty'
  | 'target_too_long'
  | 'target_reserved'
  | 'reserved_branch_segment'
  | 'invalid_target';

export type ClassifiedTarget =
  | { ok: true; lane: 'cli'; kind: 'cli'; target: string }
  | { ok: true; lane: 'cli'; kind: 'cli-branch'; target: string; owner: string; repo: string; branch: string }
  | { ok: true; lane: 'web'; kind: 'web'; target: string }
  | { ok: false; reason: TargetRejection; message: string };

export const REJECTION_MESSAGES: Readonly<Record<TargetRejection, string>> = {
  target_empty: 'Enter a CLI tool or a website.',
  target_too_long: `Targets are limited to ${TARGET_MAX_LENGTH} characters.`,
  target_reserved: 'That name is reserved.',
  reserved_branch_segment: `A branch name cannot end in ${RESERVED_REPRESENTATION_SEGMENTS.join(', ')}.`,
  invalid_target: 'That does not look like a CLI tool, a GitHub repository, or a website.',
};

// GitHub owner and repo shapes mirror GitHub's own rules so the classifier
// admits exactly what github.com would resolve.
const GITHUB_OWNER_RE = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$/;
const GITHUB_REPO_RE = /^[A-Za-z0-9._-]{1,100}$/;
const GITHUB_HOST_RE = /^(?:https?:\/\/)?(?:www\.)?github\.com\//i;
const GITHUB_URL_RE = /^(?:https?:\/\/)?(?:www\.)?github\.com\/([^/]+)\/([^/]+?)(?:\.git)?(?:\/(.*))?$/i;
/** `owner/repo` with an optional `@<branch>` tail; the worker validator shares it. */
export const GITHUB_SHORTHAND_RE = /^([^/\s@]+)\/([^/\s@]+)(?:@(.*))?$/;
const BRANCH_NAME_RE = /^[A-Za-z0-9._/-]{1,250}$/;
const SCHEME_RE = /^[a-z][a-z0-9+.-]*:\/\//i;
// The domain shape the website result route accepts: lowercase labels of
// alphanumerics and hyphens joined by dots, bounded length.
const DOMAIN_RE = /^(?=.{1,253}$)[a-z0-9](?:[a-z0-9-]{0,62})(?:\.[a-z0-9](?:[a-z0-9-]{0,62}))*$/;
const IPV4_RE = /^\d{1,3}(?:\.\d{1,3}){3}$/;
// biome-ignore lint/suspicious/noControlCharactersInRegex: the C0 range is exactly what a hostile target must not carry
const CONTROL_CHAR_RE = /[\u0000-\u001f\u007f]/;

function reject(reason: TargetRejection): ClassifiedTarget {
  return { ok: false, reason, message: REJECTION_MESSAGES[reason] };
}

function isReservedName(value: string): boolean {
  return RESERVED_NAMES.includes(value.toLowerCase());
}

function lastSegmentIsReserved(value: string): boolean {
  const last = value.slice(value.lastIndexOf('/') + 1);
  return RESERVED_REPRESENTATION_SEGMENTS.includes(last);
}

// git's ref rules, applied per component: no empty component, no
// component starting with a dot, no `.lock` suffix, no `..`, no trailing dot.
function validBranchName(branch: string): boolean {
  if (!BRANCH_NAME_RE.test(branch)) return false;
  if (branch.includes('..') || branch.endsWith('.')) return false;
  return branch.split('/').every((c) => c !== '' && !c.startsWith('.') && !c.endsWith('.lock'));
}

function branchScoped(owner: string, repo: string, branch: string): ClassifiedTarget {
  if (!validBranchName(branch)) return reject('invalid_target');
  if (lastSegmentIsReserved(branch)) return reject('reserved_branch_segment');
  return { ok: true, lane: 'cli', kind: 'cli-branch', target: `${owner}/${repo}@${branch}`, owner, repo, branch };
}

function safeDecode(segment: string): string | null {
  try {
    return decodeURIComponent(segment);
  } catch {
    return null;
  }
}

/** A GitHub URL, `owner/repo`, or `owner/repo@branch`; null when the input is none of those. */
function classifyGithub(raw: string): ClassifiedTarget | null {
  // A query or fragment on a github.com URL carries no target meaning.
  const input = GITHUB_HOST_RE.test(raw) ? raw.replace(/[?#].*$/, '') : raw;
  const url = input.match(GITHUB_URL_RE);
  if (url) {
    const [, owner, repo, tail] = url;
    if (!GITHUB_OWNER_RE.test(owner) || !GITHUB_REPO_RE.test(repo)) return reject('invalid_target');
    const path = (tail ?? '').replace(/\/+$/, '');
    if (path === '') return { ok: true, lane: 'cli', kind: 'cli', target: input };
    if (!path.startsWith('tree/')) return reject('invalid_target');
    const branch = safeDecode(path.slice('tree/'.length));
    if (!branch) return reject('invalid_target');
    return branchScoped(owner, repo, branch);
  }
  const shorthand = input.match(GITHUB_SHORTHAND_RE);
  if (!shorthand) return null;
  const [, owner, repo, branch] = shorthand;
  if (!GITHUB_OWNER_RE.test(owner) || !GITHUB_REPO_RE.test(repo)) return null;
  if (branch === undefined) return { ok: true, lane: 'cli', kind: 'cli', target: input };
  if (branch === '') return reject('invalid_target');
  return branchScoped(owner, repo, branch);
}

/** True for the parser-normalized hostnames the website lane serves. */
function isWebHostname(hostname: string): boolean {
  if (hostname === 'localhost') return true;
  if (hostname.startsWith('[') && hostname.endsWith(']')) return true;
  if (IPV4_RE.test(hostname)) return true;
  if (!hostname.includes('.')) return false;
  return DOMAIN_RE.test(hostname);
}

/** The website target (host with a non-default port) for a parsed URL, or null when it is not one. */
function webHostOf(url: URL): string | null {
  if (url.protocol !== 'http:' && url.protocol !== 'https:') return null;
  if (url.username || url.password) return null;
  if (!isWebHostname(url.hostname)) return null;
  // Website audits run over https, so the port a visitor typed against
  // http is re-read against https (drops an explicit :443).
  return new URL(`https://${url.host}/`).host;
}

function parseUrl(candidate: string): URL | null {
  try {
    return new URL(candidate);
  } catch {
    return null;
  }
}

/** A scheme-less host, IP literal, or `host/path` form; null when the first segment is not a website host. */
function classifyBareHost(input: string): ClassifiedTarget | null {
  const first = input.split(/[/?#]/, 1)[0];
  if (!first || /\s/.test(first) || /^\d+$/.test(first)) return null;
  const bracketed = first.includes(':') && !first.startsWith('[') && first.split(':').length > 2 ? `[${first}]` : first;
  const url = parseUrl(`https://${bracketed}/`);
  if (!url) return null;
  const host = webHostOf(url);
  return host ? { ok: true, lane: 'web', kind: 'web', target: host } : null;
}

/**
 * Classify a raw target into its lane and normalized target. The length
 * bound applies to the raw input before any trimming or parsing, because
 * every surface treats the target as hostile input.
 */
export function classifyTarget(raw: string): ClassifiedTarget {
  if (typeof raw !== 'string') return reject('target_empty');
  if (raw.length > TARGET_MAX_LENGTH) return reject('target_too_long');
  const input = raw.trim();
  if (!input) return reject('target_empty');
  if (CONTROL_CHAR_RE.test(input)) return reject('invalid_target');

  const github = classifyGithub(input);
  if (github) return github;

  if (SCHEME_RE.test(input)) {
    const url = parseUrl(input);
    const host = url ? webHostOf(url) : null;
    return host ? { ok: true, lane: 'web', kind: 'web', target: host } : reject('invalid_target');
  }

  const bare = classifyBareHost(input);
  if (bare) return bare;

  if (isReservedName(input)) return reject('target_reserved');
  return { ok: true, lane: 'cli', kind: 'cli', target: input };
}

/** The normalized target for a raw input, or null when the input is rejected. */
export function normalizeTarget(raw: string): string | null {
  const c = classifyTarget(raw);
  return c.ok ? c.target : null;
}

/** The lane a raw input classifies into, or null when it is rejected. */
export function laneOf(raw: string): Lane | null {
  const c = classifyTarget(raw);
  return c.ok ? c.lane : null;
}

// ---------------------------------------------------------------------------
// Result namespace
// ---------------------------------------------------------------------------

export type SplitRepresentation = { target: string; representation: Representation };

/**
 * Split a `/score/<target>[/md|/json]` pathname into its target and
 * representation. A lone trailing `md` or `json` segment is the target
 * itself, never a representation. Null off the namespace or on an empty
 * segment (trailing slash, doubled slash).
 */
export function splitRepresentation(pathname: string): SplitRepresentation | null {
  if (!pathname.startsWith(SCORE_PREFIX)) return null;
  const segments = pathname.slice(SCORE_PREFIX.length).split('/');
  if (segments.some((s) => s === '')) return null;
  let representation: Representation = 'html';
  const last = segments[segments.length - 1];
  if (segments.length > 1 && (last === 'md' || last === 'json')) {
    representation = last;
    segments.pop();
  }
  const decoded = segments.map(safeDecode);
  if (decoded.some((s) => s === null || s.includes('/'))) return null;
  const target = decoded.join('/');
  return resultTargetProblem(target) ? null : { target, representation };
}

/**
 * Why a string cannot be a `/score/<target>` target, or null when it can.
 * The builder throws on it and the splitter returns null on it, so
 * `splitRepresentation(scorePath(t))` is a round trip for every target
 * the builder accepts.
 */
function resultTargetProblem(target: string): string | null {
  if (!target) return 'a result path needs a target';
  if (isReservedName(target)) return `"${target}" is a reserved name, not a result target`;
  const segments = target.split('/');
  if (segments.some((s) => s === '')) return `"${target}" has an empty path segment`;
  if (segments.length > 1 && RESERVED_REPRESENTATION_SEGMENTS.includes(segments[segments.length - 1])) {
    return `"${target}" ends in a reserved representation segment`;
  }
  return null;
}

// Encode a target for a path or query while keeping the characters the
// scheme relies on readable: `/` and `@` for branch targets, `:` for ports.
function encodeTarget(target: string): string {
  return encodeURIComponent(target).replace(/%2F/g, '/').replace(/%40/g, '@').replace(/%3A/g, ':');
}

/** True when the builder would accept `target` and the splitter would read it back. */
export function isResultTarget(target: string): boolean {
  return resultTargetProblem(target) === null;
}

function assertResultTarget(target: string): void {
  const problem = resultTargetProblem(target);
  if (problem) throw new RangeError(problem);
}

export function scorePath(target: string): string {
  assertResultTarget(target);
  return `${SCORE_PREFIX}${encodeTarget(target)}`;
}

export function scoreMarkdownPath(target: string): string {
  return `${scorePath(target)}/md`;
}

export function scoreJsonPath(target: string): string {
  return `${scorePath(target)}/json`;
}

// ---------------------------------------------------------------------------
// Page and endpoint paths
// ---------------------------------------------------------------------------

export function scoringPath(target?: string, opts: { refresh?: boolean } = {}): string {
  if (target === undefined) return SCORING_PATH;
  const refresh = opts.refresh ? '&refresh=1' : '';
  return `${SCORING_PATH}?target=${encodeTarget(target)}${refresh}`;
}

export function auditPath(opts: { lane?: Lane; target?: string } = {}): string {
  const params: string[] = [];
  if (opts.lane) params.push(`lane=${opts.lane}`);
  if (opts.target !== undefined) params.push(`target=${encodeTarget(opts.target)}`);
  return params.length ? `${AUDIT_PATH}?${params.join('&')}` : AUDIT_PATH;
}

export function leaderboardPath(opts: { lane?: Lane; view?: 'all' } = {}): string {
  const params: string[] = [];
  if (opts.lane) params.push(`lane=${opts.lane}`);
  if (opts.view) params.push(`view=${opts.view}`);
  return params.length ? `${SCORECARDS_PATH}?${params.join('&')}` : SCORECARDS_PATH;
}

export function fixPath(checkId: string): string {
  if (!checkId) throw new RangeError('a fix path needs a check id');
  return `${FIX_PREFIX}${encodeURIComponent(checkId)}`;
}

export function apiScorePath(opts: { fromCache?: boolean } = {}): string {
  return opts.fromCache === false ? `${API_SCORE_PATH}?fromCache=false` : API_SCORE_PATH;
}

// ---------------------------------------------------------------------------
// Pathname predicates (pure; no request in scope)
// ---------------------------------------------------------------------------

function isPageOrTwin(pathname: string, page: string): boolean {
  return pathname === page || pathname === `${page}.md`;
}

export function isScorePath(pathname: string): boolean {
  return pathname.startsWith(SCORE_PREFIX);
}

export function isScoringPath(pathname: string): boolean {
  return isPageOrTwin(pathname, SCORING_PATH);
}

export function isLeaderboardPath(pathname: string): boolean {
  return isPageOrTwin(pathname, SCORECARDS_PATH);
}

export function isAuditPath(pathname: string): boolean {
  return isPageOrTwin(pathname, AUDIT_PATH);
}

export function isFixPath(pathname: string): boolean {
  return pathname.startsWith(FIX_PREFIX);
}

/** The progress page is never edge-cached. */
export function isAlwaysMissPath(pathname: string): boolean {
  return isScoringPath(pathname);
}

/** Pages whose HTML is Worker-injected from live data and purged by tag. */
export function isHitMinPath(pathname: string): boolean {
  return pathname === '/' || pathname === '/index.md' || isLeaderboardPath(pathname);
}

// ---------------------------------------------------------------------------
// Spec-derived target
// ---------------------------------------------------------------------------

export type SpecTarget = { pm: string; binary: string; owner?: string; repo?: string; branch?: string };

/**
 * The one string a resolved install spec is keyed by: the binary for
 * package-manager and direct specs, `owner/repo@branch` for a source clone.
 * The route target, the R2 key, the cache tag, and the in-flight pointer
 * all derive from it.
 */
export function targetOfSpec(spec: SpecTarget): string {
  if (spec.pm === 'git-clone') {
    if (!spec.owner || !spec.repo || !spec.branch) {
      throw new TypeError('a git-clone spec needs owner, repo, and branch');
    }
    return `${spec.owner}/${spec.repo}@${spec.branch}`;
  }
  return spec.binary;
}

// ---------------------------------------------------------------------------
// Did-you-mean
// ---------------------------------------------------------------------------

/** Optimal string alignment distance: insertions, deletions, substitutions, and adjacent transpositions. */
export function damerauLevenshtein(a: string, b: string): number {
  const rows = a.length + 1;
  const cols = b.length + 1;
  const d: number[][] = Array.from({ length: rows }, () => new Array<number>(cols).fill(0));
  for (let i = 0; i < rows; i++) d[i][0] = i;
  for (let j = 0; j < cols; j++) d[0][j] = j;
  for (let i = 1; i < rows; i++) {
    for (let j = 1; j < cols; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) {
        d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
      }
    }
  }
  return d[a.length][b.length];
}

export type SuggestOptions = {
  /** Injection point for tests; production uses damerauLevenshtein. */
  distance?: (a: string, b: string) => number;
  limit?: number;
};

/**
 * Up to three candidates within edit distance of the input (two edits at
 * five characters or more, one below), same lane only, exact matches
 * excluded, ordered by distance then length. Candidates whose length
 * differs by more than the bound are skipped before any distance is
 * computed, and the scan stops once the limit is filled with
 * distance-one matches.
 */
export function suggestTargets(input: string, candidates: readonly string[], opts: SuggestOptions = {}): string[] {
  const distance = opts.distance ?? damerauLevenshtein;
  const limit = opts.limit ?? 3;
  const needle = input.trim().toLowerCase();
  if (!needle || needle.length > TARGET_MAX_LENGTH) return [];
  const bound = needle.length >= 5 ? 2 : 1;
  const lane = laneOf(needle);
  const scored: { candidate: string; d: number }[] = [];
  let distanceOne = 0;
  for (const candidate of candidates) {
    const haystack = candidate.toLowerCase();
    if (haystack === needle) continue;
    if (Math.abs(haystack.length - needle.length) > bound) continue;
    if (laneOf(candidate) !== lane) continue;
    const d = distance(needle, haystack);
    if (d > bound) continue;
    scored.push({ candidate, d });
    if (d === 1 && ++distanceOne >= limit) break;
  }
  scored.sort((x, y) => x.d - y.d || x.candidate.length - y.candidate.length || (x.candidate < y.candidate ? -1 : 1));
  return scored.slice(0, limit).map((s) => s.candidate);
}
