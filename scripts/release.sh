#!/usr/bin/env bash
# =============================================================================
# Linux Ops Suite - release helper
# -----------------------------------------------------------------------------
# Builds every suite binary, packages each one as a .tar.gz, and creates (or
# updates) the corresponding GitHub Release with the `gh` CLI.
#
# Designed to be robust:
#   - Existing releases are updated (assets re-uploaded), not treated as fatal.
#   - If the release target commit isn't on the remote yet, the branch is
#     pushed automatically and the release retried.
#   - GitHub API errors are translated into human-readable messages.
#   - A failure in one repo does not abort the others.
#   - A summary of successes and failures is printed at the end.
#
# Usage:
#   ./release.sh v0.1.0
#
# Optional environment:
#   GH_OWNER=tom2025b     Override the GitHub owner/org.
#   SUITE_SRC_DIR=...     Override the parent directory that holds sibling repos.
#   DRY_RUN=1             Print commands without changing anything.
#   ALLOW_DIRTY=1         Skip the clean-worktree guard.
#   SKIP_EXISTING=1       Skip repos whose release already exists (don't update).
#   NO_PUSH=1             Never auto-push; fail the repo if the commit is missing.
# =============================================================================

set -Eeuo pipefail

GH_OWNER="${GH_OWNER:-tom2025b}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUITE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SUITE_SRC_DIR="${SUITE_SRC_DIR:-$(cd "${SUITE_ROOT}/.." && pwd)}"
DRY_RUN="${DRY_RUN:-0}"
ALLOW_DIRTY="${ALLOW_DIRTY:-0}"
SKIP_EXISTING="${SKIP_EXISTING:-0}"
NO_PUSH="${NO_PUSH:-0}"

usage() {
  cat <<'EOF'
Usage: ./release.sh v0.1.0

Builds and publishes GitHub Releases for:
  bulwark, scriptvault, toolfoundry, workstate, proto, rexops, toolbox-bridge

Options (environment variables):
  GH_OWNER         GitHub owner/org (default: tom2025b)
  SUITE_SRC_DIR    Parent dir holding the sibling repos
  DRY_RUN=1        Print commands without changing anything
  ALLOW_DIRTY=1    Skip the clean-worktree guard
  SKIP_EXISTING=1  Skip (don't update) releases that already exist
  NO_PUSH=1        Never auto-push a missing commit; fail that repo instead
EOF
}

VERSION="${1:-}"
if [[ -z "${VERSION}" || "${VERSION}" == "-h" || "${VERSION}" == "--help" ]]; then
  usage
  exit "${VERSION:+0}"
fi

case "$(uname -m)" in
  x86_64) TARGET_TRIPLE="x86_64-unknown-linux-gnu" ;;
  aarch64|arm64) TARGET_TRIPLE="aarch64-unknown-linux-gnu" ;;
  *)
    echo "release.sh: unsupported architecture: $(uname -m)" >&2
    echo "release.sh: supported architectures are x86_64 and aarch64" >&2
    exit 1
    ;;
esac

if [[ ! "${VERSION}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "release.sh: version must look like v0.1.0" >&2
  exit 1
fi

TOOLS=(
  "bulwark|${SUITE_SRC_DIR}/bulwark|${SUITE_SRC_DIR}/bulwark/Cargo.toml|bulwark|bulwark|bulwark|bulwark"
  "scriptvault|${SUITE_SRC_DIR}/scriptvault|${SUITE_SRC_DIR}/scriptvault/Cargo.toml|scriptvault|scriptvault|scriptvault|scriptvault"
  "toolfoundry|${SUITE_SRC_DIR}/toolfoundry|${SUITE_SRC_DIR}/toolfoundry/Cargo.toml|toolfoundry|toolfoundry|toolfoundry|toolfoundry"
  "workstate|${SUITE_SRC_DIR}/workstate|${SUITE_SRC_DIR}/workstate/Cargo.toml|workstate|workstate|workstate|workstate"
  "proto|${SUITE_SRC_DIR}/proto|${SUITE_SRC_DIR}/proto/Cargo.toml|proto|proto|proto|proto"
  "rexops|${SUITE_SRC_DIR}/rexops|${SUITE_SRC_DIR}/rexops/Cargo.toml|rexops-cli|rexops|rexops|rexops"
  "linux-ops-suite|${SUITE_ROOT}|${SUITE_ROOT}/Cargo.toml|toolbox-bridge|toolbox-bridge|linux-ops-suite|toolbox-bridge"
)

# --- logging ----------------------------------------------------------------
step() { printf '==> %s\n' "$*" >&2; }
ok()   { printf '  \033[32mok\033[0m %s\n' "$*" >&2; }
warn() { printf '  \033[33mwarn\033[0m %s\n' "$*" >&2; }
err()  { printf '\033[31mERROR:\033[0m %s\n' "$*" >&2; }

# Accumulators for the end-of-run summary.
SUCCEEDED=()
FAILED=()
SKIPPED=()

run() {
  if [[ "${DRY_RUN}" == "1" ]]; then
    printf '  dry-run %q' "$1" >&2
    shift
    for arg in "$@"; do printf ' %q' "${arg}" >&2; done
    printf '\n' >&2
    return 0
  fi
  "$@"
}

# Run a command, capturing combined output. On failure the caller can surface
# the captured output (in GH_LAST_OUTPUT) so GitHub API errors aren't swallowed.
# Returns the command's exit code.
GH_LAST_OUTPUT=""
run_capture() {
  if [[ "${DRY_RUN}" == "1" ]]; then
    printf '  dry-run %q' "$1" >&2
    shift
    for arg in "$@"; do printf ' %q' "${arg}" >&2; done
    printf '\n' >&2
    GH_LAST_OUTPUT=""
    return 0
  fi
  local rc=0
  GH_LAST_OUTPUT="$("$@" 2>&1)" || rc=$?
  return "${rc}"
}

indent_output() {
  [[ -n "$1" ]] && printf '%s\n' "$1" | sed 's/^/    /' >&2
}

check_prereqs() {
  step "Checking prerequisites"
  local missing=()
  for bin in git cargo gh tar; do
    command -v "${bin}" >/dev/null 2>&1 || missing+=("${bin}")
  done
  if [[ "${#missing[@]}" -gt 0 ]]; then
    err "required tool(s) not found: ${missing[*]}"
    exit 1
  fi
  if [[ "${DRY_RUN}" != "1" ]]; then
    if ! gh auth status -h github.com >/dev/null 2>&1; then
      warn "gh auth status reports a problem; will still attempt API calls"
      warn "if releases fail with auth errors, run: gh auth login -h github.com"
    fi
  fi
  ok "git, cargo, gh, and tar are available"
}

require_repo_ready() {
  local repo_name="$1" repo_dir="$2"

  if [[ ! -d "${repo_dir}/.git" ]]; then
    err "${repo_name}: repo not found at ${repo_dir}"
    return 1
  fi

  local branch status
  branch="$(git -C "${repo_dir}" rev-parse --abbrev-ref HEAD)"
  status="$(git -C "${repo_dir}" status --porcelain)"

  if [[ "${branch}" == "HEAD" ]]; then
    err "${repo_name}: detached HEAD (check out a branch before releasing)"
    return 1
  fi

  if [[ -n "${status}" && "${ALLOW_DIRTY}" != "1" ]]; then
    err "${repo_name}: worktree is not clean (branch: ${branch})"
    err "${repo_name}: commit or stash changes, or rerun with ALLOW_DIRTY=1"
    return 1
  fi

  ok "${repo_name}: branch ${branch}"
  return 0
}

# Ensure the local HEAD commit exists on the remote. GitHub rejects a release
# whose target_commitish it can't resolve, so push proactively when needed.
ensure_commit_pushed() {
  local repo_name="$1" repo_dir="$2" commit="$3"
  local branch
  branch="$(git -C "${repo_dir}" rev-parse --abbrev-ref HEAD)"

  if [[ "${DRY_RUN}" == "1" ]]; then
    return 0
  fi

  # Does any remote branch already contain this commit?
  if [[ -n "$(git -C "${repo_dir}" branch -r --contains "${commit}" 2>/dev/null)" ]]; then
    return 0
  fi

  if [[ "${NO_PUSH}" == "1" ]]; then
    err "${repo_name}: commit ${commit:0:8} is not on the remote and NO_PUSH=1 is set"
    return 1
  fi

  warn "${repo_name}: commit ${commit:0:8} not on remote; pushing ${branch}"
  if run_capture git -C "${repo_dir}" push origin "${branch}"; then
    ok "${repo_name}: pushed ${branch}"
    return 0
  fi

  err "${repo_name}: failed to push ${branch} to origin"
  indent_output "${GH_LAST_OUTPUT}"
  return 1
}

# Translate common gh release failures into actionable messages.
explain_gh_failure() {
  local repo_name="$1" output="$2"
  if grep -qi 'target_commitish' <<<"${output}"; then
    err "${repo_name}: GitHub rejected the target commit (not on the remote)"
  elif grep -qi 'already_exists\|already exists' <<<"${output}"; then
    err "${repo_name}: a release for ${VERSION} already exists"
  elif grep -qi 'HTTP 401\|Bad credentials\|authentication' <<<"${output}"; then
    err "${repo_name}: authentication failed; run: gh auth login -h github.com"
  elif grep -qi 'HTTP 404\|Not Found\|Could not resolve to a Repository' <<<"${output}"; then
    err "${repo_name}: repository ${GH_OWNER}/${repo_name} not found or no access"
  elif grep -qi 'HTTP 403\|rate limit' <<<"${output}"; then
    err "${repo_name}: forbidden or rate-limited by GitHub"
  else
    err "${repo_name}: gh release command failed"
  fi
  indent_output "${output}"
}

# Build, package, and publish a single repo. Never exits the process; returns
# 0 on success/skip, non-zero on failure so the caller can record it. On a skip
# the repo name is appended to SKIPPED (signalled to the caller via return 0).
release_one() {
  local repo_name="$1" repo_dir="$2" manifest_path="$3"
  local package_name="$4" binary_name="$5" asset_prefix="$6" display_name="$7"

  local archive_path="${repo_dir}/dist/${asset_prefix}-${TARGET_TRIPLE}.tar.gz"
  local release_title="${display_name} ${VERSION}"
  local release_notes="First Linux ${TARGET_TRIPLE} release for ${display_name}. Includes the prebuilt ${binary_name} binary packaged for linux-ops-install."
  local commit
  commit="$(git -C "${repo_dir}" rev-parse HEAD)"

  step "[${display_name}] Building"
  if ! run_capture cargo build --release -p "${package_name}" --manifest-path "${manifest_path}"; then
    err "${repo_name}: cargo build failed"
    indent_output "$(printf '%s\n' "${GH_LAST_OUTPUT}" | tail -n 20)"
    return 1
  fi

  step "[${display_name}] Packaging"
  run mkdir -p "${repo_dir}/dist"
  if [[ "${DRY_RUN}" != "1" ]]; then
    if [[ ! -f "${repo_dir}/target/release/${binary_name}" ]]; then
      err "${repo_name}: built binary ${binary_name} not found in target/release"
      return 1
    fi
    rm -f "${archive_path}"
  fi
  if ! run tar -C "${repo_dir}/target/release" -czf "${archive_path}" "${binary_name}"; then
    err "${repo_name}: failed to package ${binary_name}"
    return 1
  fi
  ok "${repo_name}: packaged ${archive_path}"

  # Make sure the target commit is reachable from the remote before tagging it.
  ensure_commit_pushed "${repo_name}" "${repo_dir}" "${commit}" || return 1

  # Does a release already exist?
  local exists=0
  if [[ "${DRY_RUN}" != "1" ]] && gh release view "${VERSION}" --repo "${GH_OWNER}/${repo_name}" >/dev/null 2>&1; then
    exists=1
  fi

  if [[ "${exists}" == "1" ]]; then
    if [[ "${SKIP_EXISTING}" == "1" ]]; then
      warn "${repo_name}: release ${VERSION} exists; skipping (SKIP_EXISTING=1)"
      SKIPPED+=("${repo_name}")
      return 0
    fi
    step "[${display_name}] Updating existing release ${VERSION}"
    if run_capture gh release upload "${VERSION}" "${archive_path}" \
        --repo "${GH_OWNER}/${repo_name}" --clobber; then
      ok "${repo_name}: updated ${VERSION}"
      return 0
    fi
    explain_gh_failure "${repo_name}" "${GH_LAST_OUTPUT}"
    return 1
  fi

  step "[${display_name}] Creating release ${VERSION}"
  if run_capture gh release create "${VERSION}" "${archive_path}" \
      --repo "${GH_OWNER}/${repo_name}" \
      --target "${commit}" \
      --title "${release_title}" \
      --notes "${release_notes}"; then
    ok "${repo_name}: created ${VERSION}"
    return 0
  fi

  # One automatic retry: if the commit wasn't on the remote, push and retry.
  if grep -qi 'target_commitish' <<<"${GH_LAST_OUTPUT}"; then
    warn "${repo_name}: target commit rejected; pushing and retrying"
    if ensure_commit_pushed "${repo_name}" "${repo_dir}" "${commit}"; then
      if run_capture gh release create "${VERSION}" "${archive_path}" \
          --repo "${GH_OWNER}/${repo_name}" \
          --target "${commit}" \
          --title "${release_title}" \
          --notes "${release_notes}"; then
        ok "${repo_name}: created ${VERSION} (after push)"
        return 0
      fi
    fi
  fi

  explain_gh_failure "${repo_name}" "${GH_LAST_OUTPUT}"
  return 1
}

print_summary() {
  step "Summary for ${VERSION}"
  printf '\n' >&2
  if [[ "${#SUCCEEDED[@]}" -gt 0 ]]; then
    printf '  \033[32mSucceeded (%d):\033[0m %s\n' "${#SUCCEEDED[@]}" "${SUCCEEDED[*]}" >&2
  fi
  if [[ "${#SKIPPED[@]}" -gt 0 ]]; then
    printf '  \033[33mSkipped   (%d):\033[0m %s\n' "${#SKIPPED[@]}" "${SKIPPED[*]}" >&2
  fi
  if [[ "${#FAILED[@]}" -gt 0 ]]; then
    printf '  \033[31mFailed    (%d):\033[0m %s\n' "${#FAILED[@]}" "${FAILED[*]}" >&2
  fi
  printf '\n' >&2
}

# Return 0 if the most recent SKIPPED entry is this repo.
was_just_skipped() {
  [[ "${#SKIPPED[@]}" -gt 0 && "${SKIPPED[*]: -1}" == "$1" ]]
}

main() {
  check_prereqs

  step "Checking repo state"
  local ready=()
  for entry in "${TOOLS[@]}"; do
    IFS='|' read -r repo_name repo_dir manifest_path package_name binary_name asset_prefix display_name <<<"${entry}"
    if require_repo_ready "${repo_name}" "${repo_dir}"; then
      ready+=("${entry}")
    else
      FAILED+=("${repo_name}(not-ready)")
    fi
  done

  if [[ "${#ready[@]}" -eq 0 ]]; then
    err "no repos are release-ready"
    print_summary
    exit 1
  fi

  # Release every ready repo, recording outcomes instead of aborting on error.
  for entry in "${ready[@]}"; do
    IFS='|' read -r repo_name repo_dir manifest_path package_name binary_name asset_prefix display_name <<<"${entry}"
    if release_one "${repo_name}" "${repo_dir}" "${manifest_path}" \
        "${package_name}" "${binary_name}" "${asset_prefix}" "${display_name}"; then
      was_just_skipped "${repo_name}" || SUCCEEDED+=("${repo_name}")
    else
      FAILED+=("${repo_name}")
    fi
  done

  print_summary

  if [[ "${#FAILED[@]}" -gt 0 ]]; then
    err "${#FAILED[@]} repo(s) failed; see messages above"
    exit 1
  fi
  ok "All releases completed for ${VERSION}"
}

main "$@"
