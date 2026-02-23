#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────────────────────
# fork_release.sh — Release script for a maintained fork
#
# Designed for repos that track an upstream and maintain
# fork-specific changes on top. Uses a distinct tag namespace
# (fork-v*) to avoid collision with upstream tags.
#
# What it does:
#   1. Validates working tree state
#   2. Bumps version in Cargo.toml + Cargo.lock
#   3. Generates fork changelog (commits since last fork tag
#      or since upstream divergence)
#   4. Creates annotated tag
#   5. Optionally pushes tag + commit to origin
#
# Usage:
#   scripts/release/fork_release.sh <version> [--push] [--dry-run]
#
# Examples:
#   scripts/release/fork_release.sh 0.2.0
#   scripts/release/fork_release.sh 0.2.0 --push
#   scripts/release/fork_release.sh 0.2.0 --dry-run
# ──────────────────────────────────────────────────────────────

UPSTREAM_REMOTE="upstream"
UPSTREAM_BRANCH="main"
ORIGIN_REMOTE="origin"
TAG_PREFIX="fork-v"
CARGO_TOML="Cargo.toml"
CHANGELOG="CHANGELOG.md"

# ── Helpers ──────────────────────────────────────────────────

die()  { echo "error: $*" >&2; exit 1; }
info() { echo ":: $*"; }
warn() { echo "warning: $*" >&2; }

usage() {
  cat <<'USAGE'
Usage: scripts/release/fork_release.sh <version> [--push] [--dry-run]

Arguments:
  <version>     Semver version WITHOUT prefix, e.g. 0.2.0, 0.2.0-rc.1

Options:
  --push        Push the version commit and tag to origin after creating them
  --dry-run     Show what would happen without modifying anything

The tag will be created as fork-v<version> to avoid collision with upstream tags.

What happens:
  1. Validates clean tree, version format, no duplicate tags
  2. Bumps version in Cargo.toml
  3. Generates fork-specific changelog from commits since last fork release
     (or since upstream divergence if no prior fork release)
  4. Prepends changelog entry to CHANGELOG.md
  5. Commits version bump + changelog
  6. Creates annotated tag fork-v<version>
  7. (--push) Pushes commit and tag to origin
USAGE
}

# ── Parse args ───────────────────────────────────────────────

VERSION=""
PUSH="false"
DRY_RUN="false"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --push)    PUSH="true"; shift ;;
    --dry-run) DRY_RUN="true"; shift ;;
    -h|--help) usage; exit 0 ;;
    -*)        die "unknown option: $1" ;;
    *)
      if [[ -z "$VERSION" ]]; then
        VERSION="$1"; shift
      else
        die "unexpected argument: $1"
      fi
      ;;
  esac
done

[[ -n "$VERSION" ]] || { usage; exit 1; }

SEMVER_PATTERN='^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$'
[[ "$VERSION" =~ $SEMVER_PATTERN ]] || die "version must be semver (got: $VERSION)"

TAG="${TAG_PREFIX}${VERSION}"

# ── Preconditions ────────────────────────────────────────────

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "not in a git repo"

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

[[ -f "$CARGO_TOML" ]] || die "cannot find $CARGO_TOML"

if [[ "$DRY_RUN" == "false" ]]; then
  if ! git diff --quiet || ! git diff --cached --quiet; then
    die "working tree is not clean; commit or stash changes first"
  fi
fi

# Check tag doesn't already exist
if git show-ref --tags --verify --quiet "refs/tags/$TAG"; then
  die "tag already exists locally: $TAG"
fi

info "Fetching remotes..."
git fetch --quiet "$ORIGIN_REMOTE" --tags 2>/dev/null || warn "could not fetch origin"
git fetch --quiet "$UPSTREAM_REMOTE" --tags 2>/dev/null || warn "could not fetch upstream"

if git ls-remote --exit-code --tags "$ORIGIN_REMOTE" "refs/tags/$TAG" >/dev/null 2>&1; then
  die "tag already exists on origin: $TAG"
fi

# ── Determine changelog base ────────────────────────────────

# Find the last fork release tag, or fall back to upstream divergence point
LAST_FORK_TAG=$(git tag --list "${TAG_PREFIX}*" --sort=-v:refname | head -1)
if [[ -n "$LAST_FORK_TAG" ]]; then
  CHANGELOG_BASE="$LAST_FORK_TAG"
  info "Last fork release: $LAST_FORK_TAG"
else
  # No prior fork release — use merge-base with upstream
  CHANGELOG_BASE=$(git merge-base "$UPSTREAM_REMOTE/$UPSTREAM_BRANCH" HEAD 2>/dev/null || true)
  if [[ -z "$CHANGELOG_BASE" ]]; then
    die "cannot determine changelog base: no prior fork tag and no common ancestor with upstream"
  fi
  info "No prior fork release found; using upstream divergence point: ${CHANGELOG_BASE:0:12}"
fi

# ── Generate changelog entry ────────────────────────────────

TODAY=$(date +%Y-%m-%d)

# Collect fork-only commits (exclude merges for cleaner log)
COMMIT_LOG=$(git log --oneline --no-merges "${CHANGELOG_BASE}..HEAD" -- . ':!docs/' ':!.github/' 2>/dev/null || true)
DOCS_LOG=$(git log --oneline --no-merges "${CHANGELOG_BASE}..HEAD" -- 'docs/' '.github/' 2>/dev/null || true)

# Also capture upstream sync merges for context
MERGE_LOG=$(git log --oneline --merges "${CHANGELOG_BASE}..HEAD" --grep="upstream" 2>/dev/null || true)

# Build the entry
ENTRY="## [${VERSION}] - ${TODAY} (fork)"
ENTRY+="\n"
ENTRY+="\n### Fork Changes"
ENTRY+="\n"

if [[ -n "$COMMIT_LOG" ]]; then
  while IFS= read -r line; do
    ENTRY+="\n- ${line}"
  done <<< "$COMMIT_LOG"
else
  ENTRY+="\n- (no code changes since last fork release)"
fi

if [[ -n "$DOCS_LOG" ]]; then
  ENTRY+="\n"
  ENTRY+="\n### Docs / CI Changes"
  ENTRY+="\n"
  while IFS= read -r line; do
    ENTRY+="\n- ${line}"
  done <<< "$DOCS_LOG"
fi

if [[ -n "$MERGE_LOG" ]]; then
  ENTRY+="\n"
  ENTRY+="\n### Upstream Syncs"
  ENTRY+="\n"
  while IFS= read -r line; do
    ENTRY+="\n- ${line}"
  done <<< "$MERGE_LOG"
fi

# Track upstream version for reference
UPSTREAM_HEAD=$(git rev-parse --short "$UPSTREAM_REMOTE/$UPSTREAM_BRANCH" 2>/dev/null || echo "unknown")
ENTRY+="\n"
ENTRY+="\n### Upstream Baseline"
ENTRY+="\n"
ENTRY+="\n- upstream/${UPSTREAM_BRANCH}: ${UPSTREAM_HEAD}"

ENTRY+="\n"

# ── Show plan ────────────────────────────────────────────────

info ""
info "Release plan:"
info "  Version:        $VERSION"
info "  Tag:            $TAG"
info "  Changelog base: ${CHANGELOG_BASE:0:12}"
info "  Commits:        $(echo "$COMMIT_LOG" | grep -c . || echo 0) code, $(echo "$DOCS_LOG" | grep -c . || echo 0) docs/ci"
info "  Push:           $PUSH"
info ""
info "Changelog entry:"
echo -e "$ENTRY" | sed 's/^/  /'
info ""

if [[ "$DRY_RUN" == "true" ]]; then
  info "[dry-run] Would bump $CARGO_TOML version to $VERSION"
  info "[dry-run] Would prepend changelog entry to $CHANGELOG"
  info "[dry-run] Would create tag: $TAG"
  [[ "$PUSH" == "true" ]] && info "[dry-run] Would push to $ORIGIN_REMOTE"
  info "Done (dry-run)."
  exit 0
fi

# ── Bump version in Cargo.toml ───────────────────────────────

info "Bumping version to $VERSION in $CARGO_TOML..."

# Replace only the FIRST version = "..." line (the [package] version).
# Use Python for cross-platform reliability (macOS sed lacks GNU extensions).
python3 -c "
import re, sys
cargo = open('$CARGO_TOML').read()
cargo = re.sub(r'^version = \"[^\"]*\"', 'version = \"$VERSION\"', cargo, count=1, flags=re.MULTILINE)
open('$CARGO_TOML', 'w').write(cargo)
"

# Update Cargo.lock to reflect the version change
info "Updating Cargo.lock..."
cargo generate-lockfile --quiet 2>/dev/null || cargo check --quiet 2>/dev/null || warn "could not update lockfile automatically"

# ── Update CHANGELOG.md ─────────────────────────────────────

info "Updating $CHANGELOG..."

if [[ -f "$CHANGELOG" ]]; then
  # Insert after the "## [Unreleased]" line (or at top if missing)
  TMPFILE=$(mktemp)
  INSERTED="false"
  while IFS= read -r line; do
    echo "$line" >> "$TMPFILE"
    if [[ "$INSERTED" == "false" && "$line" =~ ^##\ \[Unreleased\] ]]; then
      echo "" >> "$TMPFILE"
      echo -e "$ENTRY" >> "$TMPFILE"
      INSERTED="true"
    fi
  done < "$CHANGELOG"

  if [[ "$INSERTED" == "false" ]]; then
    # No [Unreleased] section — prepend after header
    {
      head -6 "$CHANGELOG"
      echo ""
      echo -e "$ENTRY"
      echo ""
      tail -n +7 "$CHANGELOG"
    } > "$TMPFILE"
  fi

  mv "$TMPFILE" "$CHANGELOG"
else
  # Create new changelog
  {
    echo "# Changelog"
    echo ""
    echo -e "$ENTRY"
  } > "$CHANGELOG"
fi

# ── Commit + Tag ─────────────────────────────────────────────

info "Committing version bump and changelog..."
git add "$CARGO_TOML" "$CHANGELOG"
# Also stage Cargo.lock if it was modified
git diff --quiet Cargo.lock 2>/dev/null || git add Cargo.lock

git commit -m "release: ${TAG}

Fork release ${VERSION}.
Upstream baseline: ${UPSTREAM_REMOTE}/${UPSTREAM_BRANCH} @ ${UPSTREAM_HEAD}"

info "Creating annotated tag: $TAG"
git tag -a "$TAG" -m "zeroclaw fork release ${VERSION}

Upstream baseline: ${UPSTREAM_REMOTE}/${UPSTREAM_BRANCH} @ ${UPSTREAM_HEAD}
Fork-only commits since ${CHANGELOG_BASE:0:12}: $(echo "$COMMIT_LOG" | grep -c . || echo 0)"

# ── Push ─────────────────────────────────────────────────────

if [[ "$PUSH" == "true" ]]; then
  CURRENT_BRANCH=$(git branch --show-current)
  info "Pushing $CURRENT_BRANCH and tag $TAG to $ORIGIN_REMOTE..."
  git push "$ORIGIN_REMOTE" "$CURRENT_BRANCH"
  git push "$ORIGIN_REMOTE" "$TAG"
  info "Done. Tag $TAG pushed to $ORIGIN_REMOTE."
else
  info "Done. To push:"
  info "  git push $ORIGIN_REMOTE $(git branch --show-current)"
  info "  git push $ORIGIN_REMOTE $TAG"
fi

info ""
info "Release $TAG complete."
info ""
info "Next steps:"
info "  - Verify the tag: git show $TAG"
info "  - Build release artifacts: cargo build --release"
info "  - Docker: docker build --target release -t zeroclaw:$TAG ."
