# ToolFoundry

ToolFoundry is the lifecycle and ownership layer for the Personal Linux Ops
Suite. It records what each tool is supposed to be, where it came from, how it
is installed, who owns it, what lifecycle state it is in, and how it should be
handled when it becomes stale, risky, broken, deprecated, or ready to archive.

ToolFoundry manages human-authored manifests as desired state. It contributes
only ToolFoundry-owned facts to Workstate's central snapshot feed; suite-level
correlation and consumers stay outside this repository.

## What ToolFoundry Is

- A manifest-driven lifecycle and ownership layer: it records what each tool is,
  who owns it, how it should be installed, and what lifecycle state it is in.
- Read-only by default. The only command that touches the filesystem is
  `install-apply`, which requires an explicit `--yes`.
- A producer of one neutral JSON contract: the Workstate feed.
- A strict two-layer Rust workspace: a typed core crate and a thin CLI binary.

## What ToolFoundry Is Not

- Not an inventory scanner. It evaluates only the paths declared in a manifest;
  it never walks projects or discovers tools (that is Bulwark's job).
- Not a reconciler. `lifecycle-transition` checks the FSM and reports; it never
  rewrites or archives manifests.
- Not a suite integrator. It emits no RexOps-, ScriptVault-, or Bulwark-specific
  projections; the Workstate feed is the only integration boundary.
- Not a package manager. `install-apply` only manages declared symlinks and
  refuses anything that needs manual intervention (regular files, sudo).

## Current Status

This repository contains a working Rust workspace with a core library and CLI.
The implemented flow covers manifest validation, declared health checks,
lifecycle review and transition checks, install drift detection, dry-run install
planning, guarded install application, catalog/TUI summaries, XDG-backed config,
and neutral Workstate feed export.

## Layout

- `src/`: CLI binary entry point and command handling.
- `crates/toolfoundry-core/`: manifest models, parsing, validation, health,
  lifecycle, install, catalog, config, and Workstate feed logic.
- `config/default.yaml`: parseable example manifest based on the source design.
- `schemas/tool-manifest.v1.schema.json`: manifest v1 contract.

## Install Locally

```bash
cargo install --path .
```

This installs the `toolfoundry` binary from the current checkout. Build and test
the workspace first when changing core behavior.

## Build And Verify

```bash
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo test --workspace
```

Run the current CLI validation flow:

```bash
cargo run -- validate config/default.yaml
```

Expected output:

```text
valid manifest: backup-home (active)
```

Run the declared health checks from a manifest:

```bash
cargo run -- health config/default.yaml
cargo run -- health config/default.yaml --json
```

Health checks are not inventory scans. ToolFoundry only evaluates the paths
explicitly declared in `health.checks`.

Report lifecycle state, review status, and allowed transitions:

```bash
cargo run -- lifecycle config/default.yaml
cargo run -- lifecycle config/default.yaml --as-of 2026-09-01 --json
cargo run -- lifecycle-transition config/default.yaml --to stale
cargo run -- lifecycle-transition config/default.yaml --to archived --json
```

The lifecycle commands report state only. `lifecycle-transition` checks the FSM
and exits non-zero when a requested transition is not allowed. It does not
rewrite manifests or archive tools until a future reconciler flow is added.

Report declared install and link drift:

```bash
cargo run -- drift config/default.yaml
cargo run -- drift config/default.yaml --json
```

The drift command is read-only. It compares `install` and `links.desired`
against the filesystem and exits non-zero when drift is found.

Plan installer actions without changing files:

```bash
cargo run -- install-plan config/default.yaml
cargo run -- install-plan config/default.yaml --json
```

The install plan is always dry-run. It proposes safe symlink actions and marks
blocking or destructive cases as manual intervention.

Apply safe installer actions:

```bash
cargo run -- install-apply config/default.yaml --yes
cargo run -- install-apply config/default.yaml --yes --json
```

`install-apply` requires `--yes` and only applies actions from a non-blocked
plan: create parent directories, create symlinks, or replace existing symlinks.
It refuses manual-intervention cases such as regular files at the target path.

List validated manifests from one manifest directory:

```bash
cargo run -- catalog config
cargo run -- catalog config --json
cargo run -- catalog --config ~/.config/toolfoundry/config.yaml --json
cargo run -- tui-catalog config
cargo run -- tui-catalog --config ~/.config/toolfoundry/config.yaml --json
```

The catalog command reads only `.yaml` and `.yml` files directly inside the
given or configured directory. It does not recursively scan projects or
discover tools. `tui-catalog` renders the same validated catalog as a compact
terminal dashboard for quick review.

Export ToolFoundry's neutral Workstate feed:

```bash
cargo run -- workstate-feed config
cargo run -- workstate-feed config --as-of 2026-09-01
cargo run -- workstate-feed --config ~/.config/toolfoundry/config.yaml
cargo run -- workstate-feed config --output ~/.local/share/workstate/feeds/toolfoundry.json
```

The Workstate feed is the only supported suite integration boundary. It is
JSON-only, read-only, exits successfully when tools need attention, and contains
only ToolFoundry-owned lifecycle, ownership, health, and drift facts. Workstate
reads this feed and compiles the central `snapshot.json` for downstream tools.

Inspect configuration paths:

```bash
cargo run -- config init
cargo run -- config init --manifest-directory ~/tool-manifests
cargo run -- config init --config ~/.config/toolfoundry/config.yaml --force
cargo run -- config inspect
cargo run -- config inspect --config ~/.config/toolfoundry/config.yaml --json
```

By default, ToolFoundry looks for config at
`$XDG_CONFIG_HOME/toolfoundry/config.yaml` or `~/.config/toolfoundry/config.yaml`.
If no config exists, the manifest directory defaults to
`$XDG_DATA_HOME/toolfoundry/manifests` or `~/.local/share/toolfoundry/manifests`.
`config init` creates the config parent directory and resolved manifest
directory as needed. It refuses to overwrite an existing config unless `--force`
is provided.
The current config shape is:

```yaml
manifest_directory: ~/.local/share/toolfoundry/manifests
```

## Manifest Source Of Truth

Manifests use schema version `1` and `kind: Tool`. The major sections are:

- `identity`: stable ID, display name, summary, tool kind, and tags.
- `ownership`: owner, maintainer, project, repository, local path, criticality.
- `source`: implementation language, primary file, and build strategy.
- `install`: install method, artifact path, target path, sudo requirement.
- `links`: desired symlink or link state.
- `health`: declared checks such as `file_exists` and `executable`.
- `lifecycle`: lifecycle state, review date, and optional replacement.

For `method: symlink` installs, `install.artifact_path` must match a
`links.desired[].source` and `install.target_path` must match a
`links.desired[].target`. The validator enforces this so the install paths and
the managed links cannot silently diverge.

Manifests are trusted input authored by the tool owner. ToolFoundry expands a
leading `~` or `~/` against the current user's `$HOME`, resolves declared paths,
and creates symlinks at the targets you specify (including through symlinked
parent directories); it does not sandbox or canonicalize them against an
allow-list. `~otheruser` syntax is rejected rather than silently treated as a
literal path. When `install-apply` repoints an existing managed symlink it
stages the new link and renames it over the target, so an interrupted apply
never leaves the target missing.

The manifest contract is documented in
`schemas/tool-manifest.v1.schema.json`. Unknown manifest fields are rejected by
the Rust parser and intentionally absent from the schema. Legacy suite
integration fields are not part of manifest v1.

## Workstate Feed Contract

`toolfoundry workstate-feed` emits ToolFoundry's only suite-facing contract. It
defaults to pretty JSON on stdout and writes atomically when `--output PATH` is
provided. The command is read-only and exits successfully when tools need
attention; attention is data, not command failure.

Top-level feed fields:

- `schema_version`: integer major version, currently `1`.
- `source_tool`: always `toolfoundry`.
- `generated_at`: RFC3339 generation timestamp.
- `as_of`: date used to evaluate review status.
- `tool_count` and `attention_count`: producer summary counts.
- `tools`: sorted tool records.

Each tool record carries ToolFoundry-owned facts only:

- identity: `id`, `display_name`, `manifest_path`.
- ownership: `owner`, `project`.
- lifecycle: `lifecycle_state`, `review_after`, `review_due_flag`.
- health and drift: `health_passed`, `health_total`, `drifted`.
- aggregate state: `status` (`ok` or `attention`).

Workstate consumes this feed and compiles the central snapshot. ToolFoundry does
not emit RexOps-specific, ScriptVault-specific, or Bulwark-specific projections.

## Architecture Notes

Keep ToolFoundry modular. The core crate owns data models, validation, and
future reconciliation. The binary owns CLI parsing, user-facing output, and
top-level error reporting. Production code should use typed errors in core,
`anyhow` in the binary, and avoid `unwrap`, `expect`, `panic!`, `todo!`, or
`unimplemented!`. Crate-level clippy lints enforce those production-code
guardrails while allowing direct fixture setup in tests.
