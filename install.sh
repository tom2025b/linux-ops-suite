#!/usr/bin/env bash
# =============================================================================
# Linux Ops Suite — one-command installer / reinstaller
# -----------------------------------------------------------------------------
# Rebuilds the WHOLE suite on a fresh Linux box. The umbrella repo is a
# "contracts HQ", not a monorepo: each tool lives in its own repo (except the
# small workspace crates like toolbox-bridge, which live right here). So this
# orchestrator clones (or updates) every tool repo, builds it, and puts the
# binaries on your PATH — then installs the per-tool `r-<tool>` wrapper
# scripts + shell aliases. The whole suite is Rust; the only prerequisites
# are git and cargo.
#
# Usage:
#   ./install.sh                 # clone/update + build everything that's missing
#   ./install.sh --force         # rebuild/reinstall even if already present
#   ./install.sh --local         # use existing local clones; never `git clone`/pull
#   ./install.sh --skip-aliases  # don't write r-<tool> wrappers or aliases
#   ./install.sh --dry-run       # print what would happen; change nothing
#   ./install.sh --only a,b      # operate on just these tools (comma list)
#   ./install.sh -h | --help
#
# It is IDEMPOTENT: safe to re-run. It NEVER edits your shell rc — if something
# isn't on PATH it prints the one line to add. Aliases go in ~/.rust_aliases.sh
# (created if absent), which you source from your rc yourself.
# =============================================================================

set -Eeuo pipefail

# --- configuration -----------------------------------------------------------

GITHUB_USER="tom2025b"
GITHUB_BASE="https://github.com/${GITHUB_USER}"

# Where tool repos are cloned (siblings of this umbrella repo by default).
# Override with SUITE_SRC_DIR=/some/path ./install.sh
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUITE_SRC_DIR="${SUITE_SRC_DIR:-$(cd "${SCRIPT_DIR}/.." && pwd)}"

# Install targets.
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"          # cargo install + wrappers
WRAPPER_DIR="${WRAPPER_DIR:-$HOME/bin}"          # r-<tool> wrappers (user convention)
ALIASES_FILE="${ALIASES_FILE:-$HOME/.rust_aliases.sh}"

# The Rust tools: "repo_name:binary_name". Repo and binary usually match, but
# keep them separate so a repo whose binary differs is handled correctly.
RUST_TOOLS=(
  "bulwark:bulwark"
  "scriptvault:scriptvault"
  "toolfoundry:toolfoundry"
  "workstate:workstate"
  "rexops:rexops"
)

# Rust tools that live in THIS repo's cargo workspace (not a sibling repo):
# "crate_name:binary_name". toolbox-bridge replaced the retired Python bridge;
# rex-check is the suite-repo health dashboard (ported from ~/bin/rex-check).
WORKSPACE_TOOLS=(
  "toolbox-bridge:toolbox-bridge"
  "rex-check:rex-check"
  "conductor:conductor"
  "proto:proto"
)

# --- flags -------------------------------------------------------------------

FORCE=0
LOCAL_ONLY=0
SKIP_ALIASES=0
DRY_RUN=0
ONLY=""

while [ $# -gt 0 ]; do
  case "$1" in
    --force)        FORCE=1 ;;
    --local)        LOCAL_ONLY=1 ;;
    --skip-aliases) SKIP_ALIASES=1 ;;
    --dry-run)      DRY_RUN=1 ;;
    --only)         ONLY="${2:-}"; shift ;;
    --only=*)       ONLY="${1#*=}" ;;
    -h|--help)
      sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
      exit 0 ;;
    *)
      echo "install.sh: unknown option: $1 (try --help)" >&2
      exit 2 ;;
  esac
  shift
done

# --- output helpers ----------------------------------------------------------

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  C_BOLD="$(printf '\033[1m')"; C_DIM="$(printf '\033[2m')"
  C_OK="$(printf '\033[32m')"; C_WARN="$(printf '\033[33m')"
  C_ERR="$(printf '\033[31m')"; C_RST="$(printf '\033[0m')"
else
  C_BOLD=""; C_DIM=""; C_OK=""; C_WARN=""; C_ERR=""; C_RST=""
fi

# ALL human-facing progress goes to STDERR, so stdout stays clean for the one
# place we capture it: `dir="$(ensure_repo ...)"` must receive ONLY the path.
# (Mixing status into stdout silently corrupts the captured path — a real bug.)
say()  { printf '%s\n' "$*" >&2; }
step() { printf '%s==>%s %s\n' "$C_BOLD" "$C_RST" "$*" >&2; }
ok()   { printf '  %s✓%s %s\n' "$C_OK" "$C_RST" "$*" >&2; }
warn() { printf '  %s!%s %s\n' "$C_WARN" "$C_RST" "$*" >&2; }
err()  { printf '%sERROR:%s %s\n' "$C_ERR" "$C_RST" "$*" >&2; }
skip() { printf '  %s·%s %s\n' "$C_DIM" "$C_RST" "$*" >&2; }

# Run a command, or just print it under --dry-run. Progress to stderr.
run() {
  if [ "$DRY_RUN" -eq 1 ]; then
    printf '  %s(dry-run)%s %s\n' "$C_DIM" "$C_RST" "$*" >&2
    return 0
  fi
  "$@"
}

# A tool is in `--only` (or `--only` was not given).
selected() {
  [ -z "$ONLY" ] && return 0
  case ",$ONLY," in *",$1,"*) return 0 ;; *) return 1 ;; esac
}

trap 'err "failed at line $LINENO (exit $?). Re-run; the installer is idempotent."' ERR

# --- prerequisite checks -----------------------------------------------------

check_prereqs() {
  step "Checking prerequisites"
  local missing=0

  if command -v git >/dev/null 2>&1; then
    ok "git $(git --version | awk '{print $3}')"
  else
    err "git not found — install it first (e.g. sudo apt install git)"; missing=1
  fi

  if command -v cargo >/dev/null 2>&1; then
    ok "cargo $(cargo --version | awk '{print $2}')"
  else
    err "cargo not found — install Rust: https://rustup.rs   (curl https://sh.rustup.rs -sSf | sh)"
    missing=1
  fi

  [ "$missing" -eq 0 ] || { err "missing required tools; aborting."; exit 1; }
}

# --- repo acquisition --------------------------------------------------------

# Ensure a tool repo is present at $SUITE_SRC_DIR/<repo>, cloning or pulling as
# allowed. Echoes the repo path on success; returns 1 if it can't be obtained.
ensure_repo() {
  local repo="$1"
  local dir="${SUITE_SRC_DIR}/${repo}"

  if [ -d "$dir/.git" ]; then
    if [ "$LOCAL_ONLY" -eq 1 ]; then
      skip "$repo: using local clone (--local)"
    else
      step "Updating $repo"
      # Best-effort pull; a dirty tree or no network shouldn't abort the suite.
      if ! run git -C "$dir" pull --ff-only >/dev/null 2>&1; then
        warn "$repo: 'git pull --ff-only' skipped (local changes, detached, or offline)"
      fi
    fi
    printf '%s' "$dir"
    return 0
  fi

  if [ -d "$dir" ]; then
    warn "$repo: $dir exists but is not a git repo — using it as-is"
    printf '%s' "$dir"
    return 0
  fi

  if [ "$LOCAL_ONLY" -eq 1 ]; then
    warn "$repo: not found locally and --local set — skipping"
    return 1
  fi

  step "Cloning $repo"
  if run git clone --depth 1 "${GITHUB_BASE}/${repo}.git" "$dir" >/dev/null 2>&1; then
    printf '%s' "$dir"
    return 0
  fi
  warn "$repo: clone failed (${GITHUB_BASE}/${repo}.git) — skipping"
  return 1
}

# --- builders ----------------------------------------------------------------

install_rust_tool() {
  local repo="$1" bin="$2" dir="$3"
  local dest="${BIN_DIR}/${bin}"

  if [ "$FORCE" -eq 0 ] && [ -x "$dest" ]; then
    skip "$bin already installed ($dest) — use --force to rebuild"
    return 0
  fi

  # Build a release binary in the repo, then copy it onto PATH. (Clone/pull
  # already happened in ensure_repo.) We `cargo build --release` rather than
  # `cargo install` so the artifact stays in the repo's target/ and we control
  # exactly what lands in BIN_DIR.
  step "Building $repo (cargo build --release)"
  if ! run cargo build --release --manifest-path "${dir}/Cargo.toml"; then
    warn "$repo: cargo build --release failed — skipping"
    return 1
  fi

  local artifact="${dir}/target/release/${bin}"
  if [ "$DRY_RUN" -eq 0 ] && [ ! -f "$artifact" ]; then
    warn "$repo: built but no binary at target/release/${bin} (different bin name?) — skipping"
    return 1
  fi

  run mkdir -p "$BIN_DIR"
  if run install -m 0755 "$artifact" "$dest"; then
    ok "$bin → ${dest}"
  else
    warn "$repo: failed to copy binary to ${dest} — skipping"
    return 1
  fi
}

# Build a crate that lives in THIS repo's cargo workspace (e.g. toolbox-bridge)
# and copy its binary onto PATH. Same install discipline as install_rust_tool;
# the only difference is the source: the umbrella repo itself, no clone/pull.
install_workspace_tool() {
  local crate="$1" bin="$2"
  local dest="${BIN_DIR}/${bin}"

  if [ "$FORCE" -eq 0 ] && [ -x "$dest" ]; then
    skip "$bin already installed ($dest) — use --force to rebuild"
    return 0
  fi

  step "Building $crate (cargo build --release, umbrella workspace)"
  if ! run cargo build --release -p "$crate" --manifest-path "${SCRIPT_DIR}/Cargo.toml"; then
    warn "$crate: cargo build --release failed — skipping"
    return 1
  fi

  local artifact="${SCRIPT_DIR}/target/release/${bin}"
  if [ "$DRY_RUN" -eq 0 ] && [ ! -f "$artifact" ]; then
    warn "$crate: built but no binary at target/release/${bin} (different bin name?) — skipping"
    return 1
  fi

  run mkdir -p "$BIN_DIR"
  if run install -m 0755 "$artifact" "$dest"; then
    ok "$bin → ${dest}"
  else
    warn "$crate: failed to copy binary to ${dest} — skipping"
    return 1
  fi
}

# --- r-<tool> wrappers + aliases (user convention) ---------------------------

# Create ~/bin/r-<tool> wrappers and append aliases to ~/.rust_aliases.sh.
# Both are idempotent; aliases are only appended if not already present.
install_wrappers_and_aliases() {
  [ "$SKIP_ALIASES" -eq 1 ] && { skip "wrappers/aliases skipped (--skip-aliases)"; return 0; }

  step "Installing r-<tool> wrappers + aliases"
  run mkdir -p "$WRAPPER_DIR"

  # Ensure the aliases file exists with a header.
  if [ ! -f "$ALIASES_FILE" ] && [ "$DRY_RUN" -eq 0 ]; then
    printf '# Rust tool aliases — sourced from your shell rc.\n' > "$ALIASES_FILE"
  fi

  local all=()
  for pair in "${RUST_TOOLS[@]}"; do all+=("${pair##*:}"); done
  for pair in "${WORKSPACE_TOOLS[@]}"; do all+=("${pair##*:}"); done

  for tool in "${all[@]}"; do
    selected "$tool" || continue
    command -v "$tool" >/dev/null 2>&1 || { skip "r-$tool: $tool not on PATH yet — wrapper still written"; }

    local wrapper="${WRAPPER_DIR}/r-${tool}"
    if [ "$DRY_RUN" -eq 1 ]; then
      printf '  %s(dry-run)%s write %s and alias r-%s\n' "$C_DIM" "$C_RST" "$wrapper" "$tool" >&2
      continue
    fi
    cat > "$wrapper" <<WRAP
#!/usr/bin/env bash
# Auto-generated by linux-ops-suite/install.sh — wrapper for ${tool}.
exec ${tool} "\$@"
WRAP
    chmod +x "$wrapper"

    # Append the alias only if it's not already defined.
    if ! grep -qE "^alias r-${tool}=" "$ALIASES_FILE" 2>/dev/null; then
      printf "alias r-%s='%s'\n" "$tool" "$tool" >> "$ALIASES_FILE"
    fi
    ok "r-$tool wrapper + alias"
  done
}

# --- PATH guidance (never edits the rc) --------------------------------------

print_path_guidance() {
  step "Done"
  local need_path=0
  case ":$PATH:" in *":$BIN_DIR:"*) ;; *) need_path=1 ;; esac
  case ":$PATH:" in *":$WRAPPER_DIR:"*) ;; *) need_path=1 ;; esac

  if [ "$need_path" -eq 1 ]; then
    say ""
    warn "Add these to your shell rc (~/.bashrc or ~/.zshrc) if not already present:"
    say "    export PATH=\"$BIN_DIR:$WRAPPER_DIR:\$PATH\""
  fi
  if [ "$SKIP_ALIASES" -eq 0 ]; then
    say ""
    say "  Source your Rust aliases from your shell rc (once):"
    say "    [ -f \"$ALIASES_FILE\" ] && source \"$ALIASES_FILE\""
  fi
  say ""
  say "  Then refresh the suite snapshot:  ${C_BOLD}workstate${C_RST}  ${C_DIM}(compiles the canonical snapshot)${C_RST}"
  say "  See the README \"Running a full suite refresh\" for the producers → snapshot → consumer flow."
}

# --- main --------------------------------------------------------------------

main() {
  say "${C_BOLD}Linux Ops Suite installer${C_RST}  ${C_DIM}(src: ${SUITE_SRC_DIR})${C_RST}"
  [ "$DRY_RUN" -eq 1 ] && warn "DRY RUN — nothing will be changed"
  check_prereqs

  for pair in "${RUST_TOOLS[@]}"; do
    local repo="${pair%%:*}" bin="${pair##*:}"
    selected "$repo" || selected "$bin" || continue
    if dir="$(ensure_repo "$repo")"; then
      install_rust_tool "$repo" "$bin" "$dir" || true
    fi
  done

  for pair in "${WORKSPACE_TOOLS[@]}"; do
    local crate="${pair%%:*}" bin="${pair##*:}"
    selected "$crate" || selected "$bin" || continue
    install_workspace_tool "$crate" "$bin" || true
  done

  install_wrappers_and_aliases
  print_path_guidance
}

main "$@"
