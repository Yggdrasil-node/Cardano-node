#!/usr/bin/env bash
# sync_docs.sh — sync Cardano dev portal source markdown from GitHub.
#
# Auto-discovers every .md file under the configured paths in
# cardano-foundation/developer-portal, fetches from raw.githubusercontent.com,
# and diffs against references/sources/<slug>.md.
#
# Usage from .claude/skills/cardano-haskell-node/:
#   ./scripts/sync_docs.sh                # check mode — report drift, no changes
#   ./scripts/sync_docs.sh --update       # apply changes to references/sources/
#   ./scripts/sync_docs.sh --quiet        # summary only, no per-page diffs
#   ./scripts/sync_docs.sh --branch main  # override branch
#   ./scripts/sync_docs.sh --commit SHA   # pin to a specific commit (reproducible)
#   ./scripts/sync_docs.sh --help
#
# Usage from the repository root:
#   ./.claude/skills/cardano-haskell-node/scripts/sync_docs.sh
#
# Config: skill-local scripts/sources.json
# Requires: bash, curl, jq
#
# Exit codes:
#   0 — all snapshots match upstream (check mode) or update applied
#   1 — drift detected (check mode only)
#   2 — fetch / API errors
#   3 — usage or config error

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SKILL_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CONFIG="$SCRIPT_DIR/sources.json"
SNAPSHOT_DIR="$SKILL_ROOT/references/sources"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

UPDATE_MODE=0
QUIET_MODE=0
BRANCH_OVERRIDE=""
COMMIT_OVERRIDE=""

usage() {
  sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
  exit 3
}

# ── Parse args ─────────────────────────────────────────────────────────────
while (( $# > 0 )); do
  case "$1" in
    --update) UPDATE_MODE=1; shift ;;
    --quiet)  QUIET_MODE=1;  shift ;;
    --branch) BRANCH_OVERRIDE="$2"; shift 2 ;;
    --commit) COMMIT_OVERRIDE="$2"; shift 2 ;;
    -h|--help) usage ;;
    *) echo "Unknown arg: $1" >&2; usage ;;
  esac
done

# ── Preconditions ──────────────────────────────────────────────────────────
for cmd in curl jq; do
  command -v "$cmd" >/dev/null || { echo "ERROR: '$cmd' is required" >&2; exit 3; }
done

[[ -f "$CONFIG" ]] || { echo "ERROR: config not found at $CONFIG" >&2; exit 3; }

REPO=$(jq -r '.repo' "$CONFIG")
BRANCH=${BRANCH_OVERRIDE:-$(jq -r '.branch' "$CONFIG")}
PIN=${COMMIT_OVERRIDE:-$(jq -r '.pin_commit // empty' "$CONFIG")}

REF=${PIN:-$BRANCH}
echo "Repo:   $REPO"
echo "Ref:    $REF$([ -n "$PIN" ] && echo ' (pinned commit)' || echo " (branch tip)")"
echo ""

mkdir -p "$SNAPSHOT_DIR"

# ── Helpers ────────────────────────────────────────────────────────────────
compute_slug() {
  # Apply slug_rules in declaration order. First prefix match wins.
  # After stripping the prefix, if the leaf filename starts with its parent
  # directory's name plus '-' (e.g. monitoring/monitoring-overview), drop the
  # parent directory from the path. The leaf already encodes the section.
  local path="$1"
  local rule_count match replace stripped slug parent leaf grand

  rule_count=$(jq '.slug_rules | length' "$CONFIG")
  for (( i = 0; i < rule_count; i++ )); do
    match=$(jq -r ".slug_rules[$i].match"   "$CONFIG")
    replace=$(jq -r ".slug_rules[$i].replace" "$CONFIG")
    if [[ "$path" == "$match"* ]]; then
      stripped="${path#$match}"
      stripped="${stripped%.md}"
      # Auto-dedup: if leaf starts with "<parent>-", drop the parent dir
      if [[ "$stripped" == */* ]]; then
        parent="$(dirname "$stripped" | awk -F/ '{print $NF}')"
        leaf="$(basename "$stripped")"
        if [[ "$leaf" == "$parent"-* ]]; then
          grand="$(dirname "$(dirname "$stripped")")"
          if [[ "$grand" == "." || -z "$grand" ]]; then
            stripped="$leaf"
          else
            stripped="${grand}/${leaf}"
          fi
        fi
      fi
      slug="${replace}${stripped//\//-}"
      echo "$slug"
      return
    fi
  done
  # Fallback: strip docs/, drop .md, slashes to dashes
  stripped="${path#docs/}"
  stripped="${stripped%.md}"
  echo "${stripped//\//-}"
}

# ── GitHub API helpers (rate limit aware, optional auth) ───────────────────
GH_AUTH_HEADER=()
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  GH_AUTH_HEADER=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
  echo "Auth:   GITHUB_TOKEN provided (5000 req/hr)"
fi

gh_api() {
  curl -sS "${GH_AUTH_HEADER[@]}" \
       -H "Accept: application/vnd.github+json" \
       -H "X-GitHub-Api-Version: 2022-11-28" \
       "$@"
}

check_rate_limit() {
  local rl remaining reset_at now wait
  rl=$(gh_api "https://api.github.com/rate_limit")
  remaining=$(echo "$rl" | jq -r '.resources.core.remaining')
  reset_at=$(echo "$rl" | jq -r '.resources.core.reset')
  if [[ "$remaining" -lt 2 ]]; then
    now=$(date +%s)
    wait=$((reset_at - now))
    echo "ERROR: GitHub API rate limit exhausted (0 remaining)." >&2
    echo "       Resets at: $(date -u -d "@$reset_at" '+%Y-%m-%d %H:%M:%S UTC') (~${wait}s)" >&2
    echo "       For higher limits (5000/hr), set GITHUB_TOKEN env var:" >&2
    echo "         export GITHUB_TOKEN=ghp_xxx" >&2
    echo "         (Create at https://github.com/settings/tokens — public_repo scope is enough)" >&2
    exit 2
  fi
}

# ── Resolve ref to commit SHA for traceability ─────────────────────────────
check_rate_limit
COMMIT_RESP=$(gh_api "https://api.github.com/repos/${REPO}/commits/${REF}")
COMMIT_SHA=$(echo "$COMMIT_RESP" | jq -r '.sha // empty')

if [[ -z "$COMMIT_SHA" ]]; then
  MSG=$(echo "$COMMIT_RESP" | jq -r '.message // "unknown error"')
  echo "ERROR: could not resolve ref '$REF' to a commit: $MSG" >&2
  exit 2
fi
echo "Commit: $COMMIT_SHA"
echo ""

# ── Fetch tree ─────────────────────────────────────────────────────────────
TREE_JSON=$(gh_api "https://api.github.com/repos/${REPO}/git/trees/${COMMIT_SHA}?recursive=1")

TRUNCATED=$(echo "$TREE_JSON" | jq -r '.truncated // false')
if [[ "$TRUNCATED" == "true" ]]; then
  echo "WARN: GitHub tree truncated. Some files may be missing." >&2
fi

# ── Filter to .md files matching include_paths and not exclude_paths ───────
INCLUDES=$(jq -r '.include_paths[]'         "$CONFIG")
EXCLUDES=$(jq -r '.exclude_paths[]? // empty' "$CONFIG")

# Build a jq filter that ORs all includes
include_filter=$(echo "$INCLUDES" | awk '{
  if (NR == 1)         printf "((.path | startswith(\"%s\")) or (.path == \"%s\"))", $0, $0
  else                 printf " or ((.path | startswith(\"%s\")) or (.path == \"%s\"))", $0, $0
}')

PATHS=$(echo "$TREE_JSON" | jq -r --arg none "" "
  .tree[]?
  | select(.type == \"blob\")
  | select(.path | endswith(\".md\"))
  | select($include_filter)
  | .path
" | sort -u)

# Apply excludes
if [[ -n "$EXCLUDES" ]]; then
  for exc in $EXCLUDES; do
    PATHS=$(echo "$PATHS" | grep -v "^${exc}" || true)
  done
fi

PATHS_COUNT=$(echo "$PATHS" | grep -c . || true)

if [[ "$PATHS_COUNT" -eq 0 ]]; then
  echo "ERROR: no files matched include_paths. Check config and branch." >&2
  exit 2
fi

# ── Fetch each file in parallel from raw.githubusercontent.com ─────────────
FETCH_JOBS=0
declare -A PATH_TO_SLUG

while IFS= read -r path; do
  [[ -z "$path" ]] && continue
  slug=$(compute_slug "$path")
  PATH_TO_SLUG["$path"]="$slug"
  raw_url="https://raw.githubusercontent.com/${REPO}/${COMMIT_SHA}/${path}"
  (
    http_code=$(curl -sSL --max-time 30 -w "%{http_code}" \
                  -o "$TMP_DIR/${slug}.md" "$raw_url")
    if [[ "$http_code" != "200" ]]; then
      echo "FAIL|$slug|HTTP $http_code|$path" > "$TMP_DIR/${slug}.status"
      rm -f "$TMP_DIR/${slug}.md"
    else
      size=$(wc -c < "$TMP_DIR/${slug}.md")
      echo "OK|$slug|$size|$path" > "$TMP_DIR/${slug}.status"
    fi
  ) &
  FETCH_JOBS=$((FETCH_JOBS + 1))
  if (( FETCH_JOBS % 12 == 0 )); then
    wait
  fi
done <<< "$PATHS"
wait

# ── Compare ────────────────────────────────────────────────────────────────
ADDED=()
CHANGED=()
UNCHANGED=()
FAILED=()
ORPHANED=()

declare -A EXPECTED_SLUGS
for path in "${!PATH_TO_SLUG[@]}"; do
  EXPECTED_SLUGS["${PATH_TO_SLUG[$path]}"]=1
done

# Find orphans (snapshots with no upstream counterpart)
for f in "$SNAPSHOT_DIR"/*.md; do
  [[ -e "$f" ]] || continue
  slug="$(basename "$f" .md)"
  if [[ -z "${EXPECTED_SLUGS[$slug]:-}" ]]; then
    ORPHANED+=("$slug")
  fi
done

# Compare each fetched file
while IFS= read -r path; do
  [[ -z "$path" ]] && continue
  slug="${PATH_TO_SLUG[$path]}"
  status_file="$TMP_DIR/${slug}.status"
  [[ -f "$status_file" ]] || { FAILED+=("$slug:no-status"); continue; }

  IFS='|' read -r result rslug rest1 rest2 < "$status_file"
  if [[ "$result" == "FAIL" ]]; then
    FAILED+=("$slug:$rest1")
    continue
  fi

  fetched="$TMP_DIR/${slug}.md"
  snapshot="$SNAPSHOT_DIR/${slug}.md"

  if [[ ! -f "$snapshot" ]]; then
    ADDED+=("$slug")
    (( UPDATE_MODE )) && cp "$fetched" "$snapshot"
  elif cmp -s "$fetched" "$snapshot"; then
    UNCHANGED+=("$slug")
  else
    CHANGED+=("$slug")
    (( UPDATE_MODE )) && cp "$fetched" "$snapshot"
  fi
done <<< "$PATHS"

# Optionally remove orphan snapshots in update mode (only if user opts in)
# We don't auto-delete; instead we list orphans and let the user remove them.

# ── Report ─────────────────────────────────────────────────────────────────
echo "================================================================"
echo "Cardano docs sync — $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
echo "================================================================"
echo "Tracked pages:    $((${#UNCHANGED[@]} + ${#CHANGED[@]} + ${#ADDED[@]}))"
echo "  Unchanged:      ${#UNCHANGED[@]}"
echo "  Changed:        ${#CHANGED[@]}"
echo "  New:            ${#ADDED[@]}"
echo "  Failed:         ${#FAILED[@]}"
echo "  Orphaned:       ${#ORPHANED[@]}"
echo ""

if (( ${#ADDED[@]} > 0 )); then
  echo "── New pages (no prior snapshot) ──"
  for s in "${ADDED[@]}"; do
    lines=$(wc -l < "$TMP_DIR/${s}.md" 2>/dev/null || echo "?")
    # find the path for this slug
    for p in "${!PATH_TO_SLUG[@]}"; do
      [[ "${PATH_TO_SLUG[$p]}" == "$s" ]] && { srcpath="$p"; break; }
    done
    printf "  + %-50s  (%s lines)  ← %s\n" "$s" "$lines" "$srcpath"
  done
  echo ""
fi

if (( ${#CHANGED[@]} > 0 )); then
  echo "── Changed pages ──"
  for s in "${CHANGED[@]}"; do
    if (( QUIET_MODE )); then
      if [[ -f "$SNAPSHOT_DIR/${s}.md" ]]; then
        old=$(wc -l < "$SNAPSHOT_DIR/${s}.md")
      else
        old=0
      fi
      new=$(wc -l < "$TMP_DIR/${s}.md")
      delta=$((new - old))
      sign=""; (( delta > 0 )) && sign="+"
      printf "  ~ %-50s  (%s%d lines, %d→%d)\n" "$s" "$sign" "$delta" "$old" "$new"
    else
      echo ""
      echo "  ~ $s"
      echo "  ─────────────────────────────────────"
      diff -u "$SNAPSHOT_DIR/${s}.md" "$TMP_DIR/${s}.md" | head -120 | sed 's/^/    /'
      total=$(diff -u "$SNAPSHOT_DIR/${s}.md" "$TMP_DIR/${s}.md" | wc -l)
      if (( total > 120 )); then
        echo "    ... ($((total - 120)) more diff lines truncated)"
      fi
    fi
  done
  echo ""
fi

if (( ${#FAILED[@]} > 0 )); then
  echo "── Fetch failures ──"
  for f in "${FAILED[@]}"; do echo "  ! $f"; done
  echo ""
fi

if (( ${#ORPHANED[@]} > 0 )); then
  echo "── Orphaned snapshots (in references/sources/ but not in upstream tree) ──"
  for s in "${ORPHANED[@]}"; do
    echo "  ? $s.md  (consider removing if this file no longer exists upstream)"
  done
  echo ""
fi

# ── Exit ───────────────────────────────────────────────────────────────────
if (( UPDATE_MODE )); then
  if (( ${#CHANGED[@]} > 0 || ${#ADDED[@]} > 0 )); then
    echo "Snapshots updated in $SNAPSHOT_DIR"
    echo "Source ref: ${REPO}@${COMMIT_SHA:0:12}"
    echo ""
    echo "Next: review the diffs above and update SKILL.md / references/*.md"
    echo "      to maintain parity with the source docs."
  else
    echo "All snapshots already current — no updates applied."
    echo "Source ref: ${REPO}@${COMMIT_SHA:0:12}"
  fi
  exit 0
fi

if (( ${#FAILED[@]} > 0 )); then
  echo "Some pages failed to fetch — see failures above." >&2
  exit 2
fi

if (( ${#CHANGED[@]} > 0 || ${#ADDED[@]} > 0 )); then
  echo "Drift detected. Run with --update to apply, then update skill content."
  exit 1
fi

echo "All tracked pages match upstream. Skill is current."
echo "Source ref: ${REPO}@${COMMIT_SHA:0:12}"
exit 0
