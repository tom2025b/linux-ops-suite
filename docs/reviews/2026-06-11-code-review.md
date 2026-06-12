# Linux Ops Suite Code Review

Date: 2026-06-11

Scope: `linux-ops-suite` plus the suite repos reviewed as one codebase:
`bulwark`, `scriptvault`, `toolbox-bridge`, `toolfoundry`, `workstate`,
`proto`, `rexops`, and adjacent `contextrouter`.

No code changes were made during the review.

## 1. Overall Project Assessment

The suite is in good shape inside individual tools, but weaker as a unified
system. Most repos compile, test, and lint cleanly. The code generally shows
good engineering discipline: typed Rust models, focused crates, contract tests,
file-based integration boundaries, and a clear preference for deterministic
machine-consumed output.

The biggest gap is the suite boundary. The docs describe a Workstate-centered
dataflow where producers write feeds, Workstate compiles them into a snapshot,
and RexOps consumes that snapshot. In the current implementation, that flow is
not actually wired end to end. `rex run` looks like a full refresh, but it does
not publish the producer feeds Workstate is supposed to read. Workstate reads
committed fixture files from its own repo instead of live feed files under
`$XDG_DATA_HOME`.

Verdict: the project has strong building blocks, but the integration contract is
not yet trustworthy. Fix the cross-repo dataflow before investing heavily in
polish or new features.

## 2. Biggest Problems

### P0: `rex run` does not produce the suite state it claims to produce

Evidence:

- `linux-ops-suite/bin/rex:59` streams command output directly:

```sh
"$bin" "$@" 2>&1 || echo "  (exited non-zero; continuing)"
```

- `linux-ops-suite/bin/rex:88` runs ToolFoundry without `--output`.
- `linux-ops-suite/bin/rex:89` runs Bulwark without `--output`.
- `linux-ops-suite/bin/rex:96` runs `proto list`, not `proto feed`.
- `linux-ops-suite/bin/rex:100` runs `scriptvault search`, not a Workstate
  export.
- `workstate/src/main.rs:87` to `workstate/src/main.rs:89` hardcode:

```rust
let bulwark_path = format!("{manifest_dir}/feeds/bulwark.json");
let scriptvault_path = format!("{manifest_dir}/feeds/scriptvault.json");
let toolfoundry_path = format!("{manifest_dir}/feeds/toolfoundry.json");
```

Impact: the suite refresh path is mostly a demo/status runner. It does not
build the live snapshot described by the docs.

### P0: Workstate and RexOps disagree on section status shape

Evidence:

- `workstate/src/model/provenance.rs:63` defines `FeedStatus` as an enum with
  data-bearing variants:
  - `UnsupportedVersion { found, supported }`
  - `Failed { reason }`
- `linux-ops-suite/contracts/workstate.snapshot.schema.json:34` allows status
  to be either a string or a tagged object.
- `rexops/crates/rexops-adapters/src/workstate.rs:62` models status as:

```rust
pub status: String,
```

Impact: when Workstate emits `Failed` or `UnsupportedVersion`, RexOps can fail
to deserialize the whole snapshot instead of degrading only the affected
section.

### P1: Workstate stdout and pipe behavior is not clean

Evidence:

- `workstate/src/compile/writer.rs:51` always writes through a sibling temp file.
- `workstate/src/compile/writer.rs:63` requires a normal output filename.
- `workstate/src/main.rs:125` writes the snapshot, then prints a human summary.

Impact: documented pipe patterns such as writing to `/dev/stdout` are unsafe or
polluted by human text. Workstate needs a real stdout JSON mode with summaries
on stderr.

### P1: Proto is documented as ingested, but Workstate has no Proto adapter

Evidence:

- `linux-ops-suite/docs/INTEGRATION_MAP.md:77` says Proto emits a real
  `workstate-feed` into `.../workstate/feeds/proto.json`.
- `linux-ops-suite/docs/INTEGRATION_MAP.md:79` says Workstate ingests recent
  Proto runs the same way it ingests other feeds.
- `workstate/src/ingest/mod.rs:8` to `workstate/src/ingest/mod.rs:10` expose
  only:

```rust
pub mod bulwark;
pub mod scriptvault;
pub mod toolfoundry;
```

Impact: Proto sessions do not currently surface in the central snapshot.

### P1: ScriptVault has no suite-grade Workstate export

Evidence:

- `scriptvault/crates/scriptvault/src/main.rs:41` exposes only `Search` and
  hidden `Gen` commands.
- `scriptvault/crates/scriptvault/src/cli.rs:82` provides JSON/CSV output for
  search results.
- `scriptvault/crates/scriptvault/src/cli/export.rs:1` explicitly describes a
  search export DTO, not a Workstate feed envelope.

Impact: Workstate has a ScriptVault adapter, but there is no real producer
command that writes the feed that adapter should consume.

### P1: Toolbox Bridge fresh install is incomplete

Evidence:

- `toolbox-bridge/bridge.py:12` requires PyYAML.
- `toolbox-bridge/README.md:43` tells users to `pip install pyyaml`.
- `linux-ops-suite/install.sh:258` only installs Python dependencies if
  `requirements.txt` exists.
- `toolbox-bridge` has no `requirements.txt` or `pyproject.toml`.

Impact: a fresh suite install can create a Toolbox Bridge launcher that fails at
runtime with a missing PyYAML dependency.

### P2: Proto docs understate execution behavior

Evidence:

- `proto/README.md:24` says command steps are only displayed and not executed.
- `proto/src/cli/mod.rs:250` routes bare interactive `proto` into autocheck.
- `proto/src/cli/autocheck.rs:66` runs selected profiles.
- `proto/src/core/executor/runner.rs:56` spawns real child processes.

Impact: the safety model in the docs does not match the code. Proto may still
be appropriately gated, but the documentation must be honest about execution.

### P2: RexOps has stale Bulwark adapter code

Evidence:

- `rexops/crates/rexops-adapters/src/bulwark.rs:3` says it wraps
  `bulwark inspect scan --format json`.
- `rexops/crates/rexops-adapters/src/bulwark.rs:101` invokes:

```rust
let args = ["inspect", "scan", "--format", "json", "--text", text];
```

- Current Bulwark exposes `scan --json` and `workstate-feed`, not
  `inspect scan`.

Impact: the stale adapter is misleading and likely broken if used directly.

### P2: ContextRouter is not integrated yet

Evidence:

- `contextrouter` has no commits yet; all files are untracked.
- `contextrouter/docs/ARCHITECTURE.md:30` says source collectors should read
  Workstate snapshots, repositories, documentation, and Proto sessions.
- `contextrouter/src/sources/workstate.rs:9`,
  `contextrouter/src/sources/docs.rs:9`, and
  `contextrouter/src/sources/proto.rs:9` all return `Ok(Vec::new())`.

Impact: ContextRouter is currently Phase 1 scaffolding, not a real suite
consumer.

## 3. Issues Found By Crate/Repo

### linux-ops-suite

Strengths:

- Good role as the suite contract hub.
- `suite-ui` has solid test coverage and passed clippy.
- JSON contracts and examples validate with `jq`.

Issues:

- `bin/rex` does not publish producer feeds.
- `docs/INTEGRATION_MAP.md` describes a more complete dataflow than the code
  implements.
- Installer misses Toolbox Bridge Python dependencies.
- Current working tree contains untracked local files:
  - `.CLAUDE.md.swp`
  - `CLAUDE.md`
  - `TUI_REFACTOR_SUMMARY.md`

### bulwark

Strengths:

- Strong local design.
- Clear split between `scan --json` and `workstate-feed`.
- Contract tests pass.
- Clippy passes with `-D warnings`.

Issues:

- Some CLI/TUI files are still large, but this is not a current architectural
  blocker.
- Suite orchestration does not use Bulwark's `--output` feed publishing path.

### toolfoundry

Strengths:

- Strong Workstate feed producer.
- Clear deterministic contract surface.
- Tests and clippy pass.

Issues:

- Suite-level orchestration calls `workstate-feed` but does not write it to the
  expected Workstate feed location.
- Some feed-generation code is large enough to merit later polish, but not
  before the integration contract is fixed.

### workstate

Strengths:

- Good compiler model.
- Typed feed status and provenance design are sound locally.
- Missing and malformed feeds degrade into section status instead of panicking.
- Tests and clippy pass.

Issues:

- Reads committed fixture files by default instead of live `$XDG_DATA_HOME`
  feeds.
- No Proto adapter despite suite docs claiming Proto ingestion.
- Stdout/pipe mode is not safe.
- Status shape is not consumed correctly by RexOps.

### rexops

Strengths:

- Full workspace tests pass.
- Clippy passes.
- Current TUI refactor still compiles and tests despite many moved files.

Issues:

- Workstate status parsing is too narrow.
- Stale Bulwark adapter still targets old `inspect scan` API.
- Workspace `default-members` can under-test RexOps if contributors run plain
  `cargo test`; use `cargo test --workspace` for full verification.
- Working tree is heavily dirty in `crates/rexops-tui`.

### scriptvault

Strengths:

- Strong local app.
- Good split between TUI and scriptable `search`.
- Tests and clippy pass.

Issues:

- No suite-grade Workstate export command.
- Existing JSON/CSV export is a search result format, not a Workstate feed.
- Several modules remain large and should be split after the contract work.

### proto

Strengths:

- Good session/feed direction.
- Tests and clippy pass.
- Useful project autocheck concept.

Issues:

- Docs say command steps are display-only, but autocheck executes real commands.
- Workstate does not actually ingest Proto feeds yet.
- `rex run` calls `proto list`, so it does not refresh the Proto feed.

### toolbox-bridge

Strengths:

- Focused codebase.
- Pytest suite passes.
- Behavior is intentionally conservative around sidecars.

Issues:

- Missing dependency packaging for PyYAML.
- Suite installer cannot install dependencies because there is no
  `requirements.txt` or `pyproject.toml`.

### contextrouter

Strengths:

- Architecture direction matches the suite's file-based integration style.
- Tests and clippy pass.

Issues:

- Not currently listed as an installed suite tool.
- No commits yet; all files are untracked.
- Workstate, docs, and Proto collectors are stubs.

## 4. Recommendations And Priority List

### 1. Fix suite dataflow first

Make `rex run` publish real feeds:

```sh
toolfoundry workstate-feed <project-dir> --as-of <date> --output "$XDG_DATA_HOME/workstate/feeds/toolfoundry.json"
bulwark workstate-feed <project-dir> --output "$XDG_DATA_HOME/workstate/feeds/bulwark.json"
proto feed --output "$XDG_DATA_HOME/workstate/feeds/proto.json"
workstate --feed-dir "$XDG_DATA_HOME/workstate/feeds" --output "$XDG_DATA_HOME/rexops/feeds/workstate.snapshot.json"
```

Do not treat stdout from producers as a persisted feed.

### 2. Change Workstate input path design

Workstate should default to live feed paths under:

```text
$XDG_DATA_HOME/workstate/feeds/
```

Recommended CLI shape:

```sh
workstate --feed-dir ~/.local/share/workstate/feeds --output ~/.local/share/rexops/feeds/workstate.snapshot.json
```

Keep committed `feeds/*.json` only as fixtures for tests and demos.

### 3. Fix the Workstate/RexOps status contract

RexOps should deserialize the actual schema:

- String statuses: `Fresh`, `Stale`, `Missing`
- Tagged object statuses: `Failed`, `UnsupportedVersion`
- Unknown forward-compatible statuses should degrade safely.

Add RexOps tests for:

- Fresh all sections
- Missing one section
- Failed one section
- UnsupportedVersion one section

### 4. Add a real Workstate stdout mode

Support one of:

```sh
workstate --output -
workstate --json
```

Rules:

- JSON only on stdout.
- Human summaries on stderr.
- Do not use the atomic file writer for stdout.

### 5. Decide the Proto contract

Either:

- Add a real Workstate Proto adapter and snapshot section, or
- Remove the docs claiming Workstate ingests Proto today.

Also update Proto docs to accurately describe autocheck execution.

### 6. Add a ScriptVault Workstate export

Add a command such as:

```sh
scriptvault workstate-feed --output ~/.local/share/workstate/feeds/scriptvault.json
```

It should emit a versioned feed envelope, not search-result JSON.

### 7. Fix Toolbox Bridge packaging

Add one of:

```text
requirements.txt
```

with:

```text
PyYAML
```

or a minimal `pyproject.toml` so the suite installer can install dependencies
reliably.

### 8. Remove or update stale RexOps adapters

Update the Bulwark adapter to the current CLI or delete the scan path if RexOps
should only consume Workstate snapshots.

### 9. Do a consistency and module-size polish pass

After the integration contract works:

- Normalize lint policy across Rust repos.
- Prefer typed errors in library crates and `anyhow` only at CLI/application
  edges.
- Split the largest remaining modules.
- Add suite-level integration tests that run producer -> Workstate -> RexOps.

## 5. Verification Performed

All of the following passed during review:

```sh
jq -e . contracts/*.json examples/*.json
cargo test --all-targets --locked                    # linux-ops-suite
cargo clippy --all-targets --locked -- -D warnings   # linux-ops-suite
cargo test --all-targets --locked                    # bulwark
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked        # scriptvault
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --all-targets --locked                    # toolfoundry
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --all-targets --locked                    # workstate
cargo clippy --all-targets --locked -- -D warnings
cargo test --all-targets --locked                    # proto
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked        # rexops
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --all-targets --locked                    # contextrouter
cargo clippy --all-targets --locked -- -D warnings
.venv/bin/python -m pytest -p no:cacheprovider       # toolbox-bridge
```

## 6. Bottom Line

The project is not in trouble, but its integration story is ahead of its actual
wiring. The right next move is not broad refactoring. The right next move is to
make the documented Workstate-centered pipeline real, test it end to end, and
then clean up stale adapters, docs, and module size.
