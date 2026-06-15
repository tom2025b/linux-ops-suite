#!/usr/bin/env bash
# =============================================================================
# backup-github.sh - mirror-clone every GitHub repo you own
# -----------------------------------------------------------------------------
# Creates a dated backup folder and does a full `git clone --mirror` of every
# repository returned by the GitHub CLI. A mirror clone captures all branches,
# tags, and refs - it is a complete bare copy, ideal for backups.
#
# What it does:
#   1. Makes ~/github-backups/YYYY-MM-DD
#   2. Lists all your repos via `gh repo list`
#   3. Mirror-clones each one into that folder
#   4. Prints a summary (repo count + total size on disk)
#
# Setup (one time):
#   - Install GitHub CLI:   https://cli.github.com/
#   - Authenticate:         gh auth login
#   - Make executable:      chmod +x scripts/backup-github.sh
#
# Run:
#   ./scripts/backup-github.sh
#
# Optional environment:
#   GH_LIMIT=1000   Max repos to fetch (default 1000).
#   BACKUP_ROOT=... Override the backup root (default ~/github-backups).
# =============================================================================

# Safety: stop on errors, undefined variables, and failed pipes.
set -euo pipefail

# ----- Configuration ---------------------------------------------------------
BACKUP_ROOT="${BACKUP_ROOT:-$HOME/github-backups}"
GH_LIMIT="${GH_LIMIT:-1000}"
DEST="$BACKUP_ROOT/$(date +%Y-%m-%d)"

# ----- Preflight checks ------------------------------------------------------
# Make sure the tools we depend on are actually installed.
command -v gh  >/dev/null 2>&1 || { echo "Error: GitHub CLI (gh) not found. Install it: https://cli.github.com/"; exit 1; }
command -v git >/dev/null 2>&1 || { echo "Error: git not found."; exit 1; }

# Make sure we are logged in to GitHub.
if ! gh auth status >/dev/null 2>&1; then
  echo "Error: not authenticated with GitHub. Run: gh auth login"
  exit 1
fi

# ----- Prepare the backup folder ---------------------------------------------
echo "==> Backup destination: $DEST"
mkdir -p "$DEST"

# ----- Fetch the list of repositories ----------------------------------------
# Ask gh for owner/name pairs (e.g. "tom2025b/linux-ops-suite"), one per line.
echo "==> Fetching repository list from GitHub..."
mapfile -t REPOS < <(gh repo list --limit "$GH_LIMIT" --json nameWithOwner --jq '.[].nameWithOwner')

if [ "${#REPOS[@]}" -eq 0 ]; then
  echo "No repositories found. Nothing to back up."
  exit 0
fi
echo "==> Found ${#REPOS[@]} repositories."

# ----- Clone each repository as a mirror -------------------------------------
SUCCESS=0
FAILED=0
for repo in "${REPOS[@]}"; do
  # The mirror is stored as "<name>.git" inside the dated folder.
  name="${repo##*/}"          # strip owner, keep repo name
  target="$DEST/${name}.git"

  if [ -d "$target" ]; then
    echo "  - skip   $repo (already backed up)"
    continue
  fi

  echo "  - clone  $repo"
  if gh repo clone "$repo" "$target" -- --mirror >/dev/null 2>&1; then
    SUCCESS=$((SUCCESS + 1))
  else
    echo "    ! failed to clone $repo"
    FAILED=$((FAILED + 1))
  fi
done

# ----- Summary ---------------------------------------------------------------
TOTAL_SIZE="$(du -sh "$DEST" 2>/dev/null | cut -f1)"
echo ""
echo "=============================================="
echo " GitHub backup complete"
echo "----------------------------------------------"
echo " Location    : $DEST"
echo " Backed up   : $SUCCESS repositories"
[ "$FAILED" -gt 0 ] && echo " Failed      : $FAILED repositories"
echo " Total size  : ${TOTAL_SIZE:-unknown}"
echo "=============================================="
