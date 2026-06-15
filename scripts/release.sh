#!/usr/bin/env bash
# =============================================================================
# Linux Ops Suite - first-release helper
# -----------------------------------------------------------------------------
# Builds every suite binary, packages each one as a .tar.gz, and creates the
# corresponding GitHub Release with `gh release create`.
#
# Usage:
#   ./release.sh v0.1.0
#
# Optional environment:
#   GH_OWNER=tom2025b     Override the GitHub owner/org.
#   SUITE_SRC_DIR=...     Override the parent directory that holds sibling repos.
#   DRY_RUN=1             Print commands without changing anything.
#   ALLOW_DIRTY=1         Skip the clean-worktree guard.
# =============================================================================

set -Eeuo pipefail

GH_OWNER="${GH_OWNER:-tom2025b}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUITE_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SUITE_SRC_DIR="${SUITE_SRC_DIR:-$(cd "${SUITE_ROOT}/.." && pwd)}"
DRY_RUN="${DRY_RUN:-0}"
ALLOW_DIRTY="${ALLOW_DIRTY:-0}"

usage() {
  cat <<'EOF'
Usage: ./release.sh v0.1.0

Builds and publishes the first GitHub Releases for:
  bulwark, scriptvault, toolfoundry, workstate, proto, rexops, toolbox-bridge
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

step() { printf '==> %s\n' "$*" >&2; }
ok()   { printf '  ok %s\n' "$*" >&2; }
err()  { printf 'ERROR: %s\n' "$*" >&2; }

run() {
  if [[ "${DRY_RUN}" == "1" ]]; then
    printf '  dry-run %q' "$1" >&2
    shift
    for arg in "$@"; do
      printf ' %q' "${arg}" >&2
    done
    printf '\n' >&2
    return 0
  fi
  "$@"
}

check_prereqs() {
  step "Checking prerequisites"
  command -v git >/dev/null 2>&1 || { err "git not found"; exit 1; }
  command -v cargo >/dev/null 2>&1 || { err "cargo not found"; exit 1; }
  command -v gh >/dev/null 2>&1 || { err "gh not found"; exit 1; }
  command -v tar >/dev/null 2>&1 || { err "tar not found"; exit 1; }
  if [[ "${DRY_RUN}" != "1" ]]; then
    gh auth status -h github.com >/dev/null 2>&1 || {
      err "gh auth is not ready; run: gh auth login -h github.com"
      exit 1
    }
  fi
  ok "git, cargo, gh, and tar are available"
}

require_repo_ready() {
  local repo_name="$1"
  local repo_dir="$2"

  [[ -d "${repo_dir}/.git" ]] || {
    err "${repo_name}: repo not found at ${repo_dir}"
    return 1
  }

  local branch
  branch="$(git -C "${repo_dir}" rev-parse --abbrev-ref HEAD)"
  local status
  status="$(git -C "${repo_dir}" status --porcelain)"

  if [[ "${branch}" == "HEAD" ]]; then
    err "${repo_name}: detached HEAD"
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

create_release() {
  local repo_name="$1"
  local repo_dir="$2"
  local manifest_path="$3"
  local package_name="$4"
  local binary_name="$5"
  local asset_prefix="$6"
  local display_name="$7"

  local archive_path="${repo_dir}/dist/${asset_prefix}-${TARGET_TRIPLE}.tar.gz"
  local release_title="${display_name} ${VERSION}"
  local release_notes="First Linux ${TARGET_TRIPLE} release for ${display_name}. Includes the prebuilt ${binary_name} binary packaged for linux-ops-install."

  step "Building ${display_name}"
  run cargo build --release -p "${package_name}" --manifest-path "${manifest_path}"

  step "Packaging ${display_name}"
  run mkdir -p "${repo_dir}/dist"
  if [[ "${DRY_RUN}" != "1" ]]; then
    rm -f "${archive_path}"
  fi
  run tar -C "${repo_dir}/target/release" -czf "${archive_path}" "${binary_name}"
  ok "${archive_path}"

  step "Creating GitHub Release for ${repo_name}"
  if [[ "${DRY_RUN}" != "1" ]] && gh release view "${VERSION}" --repo "${GH_OWNER}/${repo_name}" >/dev/null 2>&1; then
    err "${repo_name}: release ${VERSION} already exists"
    exit 1
  fi
  run gh release create "${VERSION}" "${archive_path}" \
    --repo "${GH_OWNER}/${repo_name}" \
    --target "$(git -C "${repo_dir}" rev-parse HEAD)" \
    --title "${release_title}" \
    --notes "${release_notes}"
  ok "${repo_name}: ${VERSION}"
}

main() {
  check_prereqs

  local blockers=0
  step "Checking repo state"
  for entry in "${TOOLS[@]}"; do
    IFS='|' read -r repo_name repo_dir manifest_path package_name binary_name asset_prefix display_name <<<"${entry}"
    require_repo_ready "${repo_name}" "${repo_dir}" || blockers=$((blockers + 1))
  done

  if [[ "${blockers}" -ne 0 ]]; then
    err "${blockers} repo(s) are not release-ready"
    exit 1
  fi

  for entry in "${TOOLS[@]}"; do
    IFS='|' read -r repo_name repo_dir manifest_path package_name binary_name asset_prefix display_name <<<"${entry}"
    create_release "${repo_name}" "${repo_dir}" "${manifest_path}" "${package_name}" "${binary_name}" "${asset_prefix}" "${display_name}"
  done

  step "Done"
  ok "All releases created for ${VERSION}"
}

main "$@"
