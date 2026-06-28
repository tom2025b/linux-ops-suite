# Last Work

## Repo-to-crate consolidation — pull all six standalone tools into the umbrella

2026-06-27. **Committed, pushed, and MERGED to `main`** across six PRs; `main` is at
the final merge (`ffa50ec`) and green. The umbrella went from 13 to **27 member
crates**; it is now effectively a monorepo. **No git references to any sibling repo
remain** in the manifests or `Cargo.lock`.

**Goal (Tom's):** pull the standalone sibling repos into the umbrella workspace
under `crates/`, smallest/lowest-dep first, converting each to workspace dependency
inheritance and flipping cross-repo git deps to in-tree path deps — ending with
Workstate (the shared contract) last because it changes the dependency model for the
whole workspace.

**Order, PRs, and merge commits (each green on the full CI gate before merge):**
- Proto → `crates/proto` — **PR #57** (`9875498`).
- ToolFoundry → `crates/toolfoundry{,-core}` — **PR #59** (`c4df2f9`).
- Bulwark → `crates/bulwark{,-core}` — **PR #60** (`f32a98d`).
- RexOps → `crates/rexops-{core,adapters,app,cli,tui}` — **PR #61** (`e430abf`).
- ScriptVault → `crates/scriptvault{,-core}` — **PR #63** (`00108c6`).
- Workstate → `crates/workstate{,-schema}` — **PR #64** (`ffa50ec`).

**The recipe applied to each (per tool):** copy the crate(s) in (excluding
`target`/lockfiles); convert each manifest to workspace inheritance (`{ workspace =
true }`); set `edition` explicitly only where it differs from the umbrella's 2021
(Proto/ToolFoundry/Bulwark/ScriptVault are 2024); drop per-crate `readme` (the
umbrella defines none); centralize the internal lib crate(s) and any new third-party
deps in `[workspace.dependencies]`; move each tool from the sibling-repo install
path to the in-workspace path (`release.rs` `TOOLS` registry → `repo:
"linux-ops-suite"`, `release.yml` `-p <crate>` + `bins`, `install.sh`
`WORKSPACE_TOOLS`, README); run the EXACT CI gate locally (`fmt --check`, `clippy
--workspace --all-targets --all-features -D warnings`, `build --all-targets
--all-features`, `test --all-features`) before pushing.

**Notable per-tool details and snags fixed:**
- **suite-ui git → path.** Bulwark, RexOps, and ScriptVault consumed `suite-ui` as a
  pinned git dep; all flipped to the in-tree path crate via workspace inheritance.
  ScriptVault keeps the `clap` feature: `suite-ui = { workspace = true, features =
  ["clap"] }` (the umbrella default carries no features; verified `crates/suite-ui`
  exposes `clap = ["thomas-tui/clap"]`).
- **Proto MSRV (caught by CI clippy, then fixed across the stack).** Proto used
  `u64::is_multiple_of` (stable in 1.87) but the umbrella MSRV is 1.85 → clippy's
  `incompatible_msrv`, denied in CI. Fixed to `secs % 60 == 0` and folded into
  Proto's commit; the fix was rebased through the then-stacked ToolFoundry/Bulwark
  branches so all three went green before merge.
- **Bulwark order-coupled test.** A `linux-ops-install` test hardcoded `r-bulwark` as
  the first wrapper alias; moving Bulwark out of the top of the `TOOLS` registry
  broke it. Fixed to assert against `TOOLS[0].binary` (order-independent — the test's
  real intent). Also kept `serde_yaml_bw` inline in `bulwark-core` (the umbrella
  standardizes on classic `serde_yaml` 0.9; the two coexist).
- **RexOps (5 crates, biggest).** `thiserror` 1 → 2 reconciliation (derive usage is
  2.0-clean); no new third-party deps; centralized the four depended-on internal
  crates. `rexops-tui` builds but isn't shipped (the `rexops` binary launches the
  TUI by default).
- **ScriptVault (dep-heaviest).** Centralized **9 new deps** (fuzzy-matcher, globset,
  `dirs` — distinct from `directories` — tracing, tracing-subscriber, clap_complete,
  clap_mangen, arboard, syntect). Relocated its embedded `config/default.yaml` INTO
  the crate (it had reached out to the old repo root via
  `include_str!("../../../../config/default.yaml")`).
- **Workstate (the contract, last).** Its repo wasn't a `crates/` layout — root
  package (`workstate` lib+bin) + a `workstate-schema` subdir crate. Flattened to
  `crates/workstate-schema` (the contract) and `crates/workstate` (producer lib +
  `workstate` binary, with a fresh package-only Cargo.toml). Both `workstate-schema`
  and `workstate` flipped from pinned git deps to path deps; the three consumers
  (toolbox-bridge, conductor, pulse) already used `{ workspace = true }`, so they
  picked up the in-tree crates with no edits. Local HEAD was the pinned rev + one
  docs-only commit, so the contract code is unchanged. New dep: `uuid` (v4). After
  this, `install.sh`'s `RUST_TOOLS` sibling list is empty.

**Process notes:** worktree-per-tool, stacked PRs at first (Proto→ToolFoundry→Bulwark
shared edits to the same files) then sequential off `main` once the stack merged.
`gh pr edit`/`pr merge` hit a deprecated-Projects GraphQL bug, so base-retarget used
the REST API (`gh api PATCH`); merges used `--merge` commits to preserve history.
After each merge: deleted the remote+local branch and worktree, fast-forwarded local
`main`, and re-verified `main`'s push CI green.

**Follow-up:** the six superseded standalone repos (`bulwark`, `scriptvault`,
`toolfoundry`, `proto`, `rexops`, `workstate`) can now be archived.

## Fix "conductor keeps saying workstate snapshot is stale" — wire workstate to LIVE tools

2026-06-22. Spans 4 repos, **NOT committed, NOT pushed** (changes in working
trees only): siblings `workstate`, `scriptvault`, `toolfoundry`, plus `conductor`
inside the `linux-ops-suite` worktree `worktree-workstate-live-feeds`.
Tests: workstate 60 green, scriptvault (full) green, toolfoundry-core green,
conductor 108 green. `cargo clippy` clean on all four. End-to-end verified live.

**Root cause (diagnosed first, then fixed):** `workstate` shipped three COMMITTED
demo fixtures (`workstate/feeds/{scriptvault,toolfoundry,bulwark}.json`) with
frozen `generated_at` dates (June 2/4). Workstate's freshness rule is correct —
`Stale` when `now − source_observed_at > 24h` (`compile/mod.rs:19`) — so the
~18-day-old fixtures were ALWAYS Stale. Re-running `workstate` re-read the SAME
frozen files, so the snapshot was forever made of stale data and Conductor's rule
1 forever said "workstate snapshot is stale". Both tools behaved as written;
neither was "broken" alone. Path was never the issue (write==read ==
`~/.local/share/rexops/feeds/workstate.snapshot.json`).

**Tom's decision:** wire workstate to the REAL tools (option 2), and have workstate
SPAWN them each run (not read published files), adding a ScriptVault exporter.

**What changed:**
- **scriptvault** — NEW `scriptvault workstate-feed` (alias `ws-feed`) subcommand
  (`crates/scriptvault/src/cli/workstate_feed.rs`): emits the versioned envelope
  Workstate's `ScriptVaultRaw` adapter expects (`schema_version/source_tool/
  generated_at/scripts[id,name,description]/favorites/recents`) from the LIVE
  index (`ScriptVault::load().all()`), stable id = file stem, `generated_at`
  stamped now. + `chrono` dep (0.4, `clock` only). Wired in `cli.rs` + `main.rs`.
- **workstate** — NEW `src/ingest/transport.rs`: `FeedTransport::{File,Command}`.
  The three adapters (`bulwark/scriptvault/toolfoundry`) now hold a transport
  instead of a `String` path; `fetch()` = `self.transport.read()? → parse` (this
  REMOVED the triplicated file-read). `main.rs` builds each feed via
  `from_command("<tool>", ["workstate-feed"])` — spawns the tool, ingests stdout.
  Binary resolved as bare name on `$PATH`, overridable by `WORKSTATE_<TOOL>_BIN`.
  Graceful-degradation contract preserved + extended: tool not on PATH → NotFound
  → Missing; ran-but-exited-nonzero / non-UTF8 → Failed. Tests updated to
  `XxxFeed::from_path(...)`. Static fixtures are NO LONGER read (proven by moving
  them aside and still getting Fresh data).
- **toolfoundry** (`crates/toolfoundry-core/src/registry/catalog.rs`) — `manifest_
  paths` now treats an ABSENT registry dir as an empty catalog, not a hard error
  (fail-soft). Without this, a fresh machine with no `~/.local/share/toolfoundry/
  manifests` made `toolfoundry workstate-feed` exit 1 → workstate `tools` section
  Failed. A real read error (file-where-dir) still propagates. Test rewritten.
- **conductor** (`crates/conductor/src/{state.rs,plan/rules.rs}`) — split the
  conflated "stale OR unavailable → refresh" message. Added `stale_feeds()` /
  `unavailable_feeds()`; `situation()` now emits a STALE line ("refresh before
  trusting feeds", names the feed) AND a separate UNAVAILABLE line ("a refresh
  won't fix it"). Rule-1 step relabeled "refresh suite snapshot" (id unchanged).
  This kills the SAME class of false advice (telling you to refresh something a
  refresh can't clear). + 2 new tests.

**End-to-end proof:** live `workstate` (spawning the 3 real release binaries) now
writes scripts/tools/findings all **Fresh** (114 scripts, 229 findings, tools
empty-but-fresh since no manifests exist yet). Conductor from /tmp, ~, and
~/projects/rexops all print "nothing to conduct — every feed is current". The
multi-folder symptom is gone (conductor reads the data-root snapshot, not cwd).

**Follow-ups (none blocking):** (1) the three tool binaries aren't bare-on-`$PATH`
on this box (aliases / built artifacts), so the live flow currently needs the
`WORKSTATE_*_BIN` overrides — install them on PATH for zero-config. (2) Workstate
snapshot is still `schema_version 3` on disk while fixtures/tests reference v4 —
pre-existing drift, NOT this bug, left untouched. (3) Nothing committed yet —
awaiting Tom's review + per-repo commit/push approval.

---

## Conductor → suite-ui migration — Conductor's TUI now matches RexOps

2026-06-22. Worktree branch `worktree-conductor-suite-ui` (`linux-ops-suite`).
Status: **committed locally, NOT pushed** (commits `794f137`→`8911898`).
Tests: full workspace green (conductor 78 lib + 13 cli; suite-ui 33; thomas-tui
92; all consumer tools pass). `cargo build`/`cargo test` clean; `cargo clippy -p
conductor --all-targets` zero warnings. Manual on-terminal smoke pending Tom's
eyeball (a `--dump-view` render proxy was inspected and looks right).

**Why:** Conductor was the ONLY suite TUI still on a hand-rolled ANSI renderer
(`tui/term.rs` raw-mode driver + `tui/frame.rs` String renderers + `tui/style.rs`
escape-string colours; no ratatui/crossterm/suite-ui). That's why its UI looked
nothing like RexOps. Pulse was migrated to suite-ui back in PR #40; Conductor was
the leftover from the old "modeled on pulse" pattern.

**What changed (one crate, `crates/conductor`):**
- `Cargo.toml`: + `suite-ui`/`ratatui`/`crossterm` as `{ workspace = true }` path
  deps (no git pin — it's a workspace member, unlike the external rexops repo).
- `src/tui/` rebuilt on the RexOps structure:
  - `app.rs` — the state machine (App + key→action + the Ring-2 confirm gate +
    exit-code tally), ported from the old `tui/mod.rs` to crossterm `KeyEvent`.
  - `render.rs` — ratatui renderers in the shared look: `pane()` rounded panes
    (Conductor / The situation / The plan), the `▌` accent selection rail on the
    focused step, ▸/○/✓/·/✗ glyphs, `ConfirmModal` (Ring-2 gate) + `HelpSheet` +
    `EmptyState` overlays.
  - `runtime.rs` — the `suite_ui::Tui` guard (alt screen + panic-restore), a
    dirty-flag draw/event loop, and a `Spawner` adapter that forwards child
    spawns to `Tui::suspended` (handoff to rexops, Ring-1/2 step runs).
  - `mod.rs` — thin root (re-exports `run`/`RunReport`; new `dump_view` renders
    one frame via `TestBackend` for `--dump-view` + snapshot tests).
  - DELETED `term.rs`, `style.rs`, `frame.rs` (the hand-rolled stack).
- `main.rs`: `--dump-view` routes through the suite-ui render; `run_bare` path
  unchanged (still `tui::run` + `should_run_interactive`).
- `tests/cli.rs`: the three `--dump-view` assertions updated from old ANSI
  spacing/copy to the new pane + ConfirmModal content.

**Preserved exactly:** the Ring-2 safety gate (a changes-state step never spawns
on Enter — only `y` runs it; guarded by ported tests), the `r` rexops handoff,
exit codes (1 fail / 2 unfinished / 0 clean), and every scriptable path
(`status`/`plan`/`health`/`--json`/non-TTY fallback — all still via `report.rs`,
untouched). Domain layer (`plan/`/`state.rs`/`sources.rs`/`run.rs`/`report.rs`)
not touched.

Design + plan: `docs/superpowers/specs/2026-06-22-conductor-suite-ui-migration-design.md`,
`docs/superpowers/plans/2026-06-22-conductor-suite-ui-migration.md`.

Next: Tom to run `conductor` on a real terminal and confirm the look; then decide
push / PR. (suite-ui API was NOT changed, so no consumer rev-bump is needed.)

---

## RexOps Probe wiring fix — HealthSource::Probe rows are now actually probed

2026-06-21. Branch `feat/probe-rows-into-adapter-health` (`rexops` repo).
Status: **PUSHED, PR #34 open** (commit `dfc1ef9`) on top of merged main
`44c1146`. Tests: full workspace green (310, +7 new); build --all-targets, clippy
--all-targets -D warnings, fmt --all --check all clean. Touches 3 files:
`rexops-app/src/snapshot.rs` (the fix), `rexops-core/src/component_table.rs`
(comments), and `rexops-tui/src/app/tests/cockpit.rs` (tests).

**Three review follow-ups also done this pass:**
- (1) Comments: the four registry rows + `registry_walk`'s health comment now say
  the snapshot builder actually spawns `<bin> --help` each refresh (exits 0→Healthy,
  absent→Unavailable) instead of implying it's unwired/aspirational. No comment
  overstates how "Live" the tools are — they're Live+launchable but health is real.
- (2) `component_vital` Probe arm now prefers any real `status_detail` the probe
  recorded, falling back to "installed"/"not installed" presence, and None when not
  yet probed — so it reflects real data when available rather than a fixed label.
- (3) Tests strengthened: `every_probe_tool_is_unknown_until_probed_then_resolves`
  (app) pins Unknown-until-probed + Healthy-once-probed + Live + launchable + vital
  across all four tools; the cockpit guard test now asserts the full per-tool Probe
  contract (binary==id, version_args==`--help`, Foreground); new
  `probe_tool_card_gates_launch_on_resolved_health` (tui) proves the launch gate
  follows resolved health at the UI boundary.

**The bug (found in review of PR #33):** the four Probe+launch tools
(tripwire/rewind/rex-check/rex-forge) declared `HealthSource::Probe{binary,
version_args}`, but **nothing ever read those fields** — `build_snapshot_with_piped`
only resolved the hardcoded `RESOLVED_TODAY` adapters (bulwark/system/workstate)
plus the StatusCommand loop (pulse). So every Probe row fell through `registry_walk`'s
`_ => adapter_health.get(id).unwrap_or(Unknown)` arm and was **permanently Unknown
with a blank vital**, regardless of whether the binary was installed. The cards said
"Live" but were indistinguishable from Planned except for the label + launch button.
This contradicted the prior LAST_WORK claim ("present→Healthy, absent→Unavailable")
and ("tripwire is on PATH so its card is green now") — both were false until this fix.

**The fix (minimal, one file):**
- New `probe_binary_presence(config, id, version_args)` — mirrors the existing
  `status_command_probe` spawn/timeout structure but maps on exit status only:
  spawn-fail→Unavailable, exit 0→Healthy, non-zero/timeout→Unavailable. Reuses the
  same `status_prefix_args` test seam (sh-reads-script, no `ETXTBSY`).
- New Probe loop in `build_snapshot_with_piped` (right after the StatusCommand
  loop): for every `HealthSource::Probe` row **not** in `real_adapter_ids` (skips
  bulwark — its adapter owns it; no double-probe) and enabled in config, write the
  resolved health into `adapter_health`. The registry walk then projects it onto the
  card AND the launch gate (`health != Unavailable`), so an absent binary is both
  Unavailable and unlaunchable. This also lights **proto** (Probe, FeedReady).
- `component_vital` Probe arm: Healthy→"installed", Unavailable→"not installed",
  Unknown→None (no blank for a resolved card; no guess for an unresolved one).
- Updated 3 roster-anchor tests that hardcoded the old `[bulwark, pulse, system,
  workstate]` adapter_health set — now `+ [proto, tripwire, rewind, rex-check,
  rex-forge]`; fixed the stale "Probe rows are NOT in adapter_health" comment.
- Added 5 tests: exit-status→health mapping, missing-binary, installed-end-to-end
  (Healthy+launchable+"installed" vital), absent-end-to-end (Unavailable+unlaunchable
  +"not installed"), and no-double-probe-bulwark.

**Net:** an installed Probe tool now reads genuinely Healthy; an absent one honestly
Unavailable+unlaunchable. No more permanently-Unknown "Live" cards. Pure addition —
the StatusCommand/Pulse and adapter paths are untouched.

---

## RexOps tools-to-Live — tripwire + rewind + rex-forge flipped (rollup 10/11, the CEILING)

2026-06-21. Branch `rexops-tools-live-finish` (rexops repo). Status: **MERGED to
rexops `main`** — PR #33, merge commit `44c1146`, on top of `83db82c`. Three
per-tool commits: `2adb2f4` tripwire, `b260af3` rewind, `a14c962` rex-forge —
each its own small green commit. Local branch deleted post-merge; **cockpit
redesign is now complete.**

Finished the cockpit tool-wiring: flipped the **last three Planned tools to Live**,
reaching **10/11** — the ceiling (only workstate=Feed and system=Host never
launch). No fake cards.

**Contract check first (the assigned step 1):** none of the three has the
`{healthy,detail,latency_ms}` line, so StatusCommand (the Pulse path) was OFF the
table for all three — verified by reading their real CLIs in
`linux-ops-suite/crates/*`:
- **tripwire** `--json` = a Workstate-style envelope (`source_tool/clean/added…`).
- **rewind** `--json` = a `TimelineEnvelope` (`schema_version/source_tool/captures…`).
- **rex-forge** = a TUI scaffolder with only `new`/`list`, **no JSON contract at all**.

**Decision: all three flipped with the validated rex-check Probe+launch pattern**
(NOT a Feed adapter — that would mean building feed-publish + vital wiring, heavier
and unnecessary; KISS). Per tool: `health: HealthSource::Probe{ binary, --help }`
(present→Healthy, absent→Unavailable — honest, self-degrading), `launch:
LaunchSpec{ Foreground }`, `maturity: Live`.
- tripwire / rewind launch args `&[]` (current-view / capture-timeline).
- rex-forge launch args `&["list"]` — deliberately: bare errors (subcommand
  required, exit 2) and `new` is interactive + **writes a project**; `list` is the
  read-only catalog (zero side effects), proving the tool runs without scaffolding.

**Install gap (Tom chose "flip registry only"):** tripwire is on PATH
(`~/.cargo/bin/tripwire`) so its card is green now; rewind + rex-forge aren't
installed, so their cards are Live but `rexops launch <tool>` **refuses with exit
1 + a precise reason** ("not on PATH, no configured binary") rather than
fake-resolving. Registry vs install are separate concerns; the suite installer
places binaries later. (The installer TOOLS roster in
`crates/linux-ops-install/src/release.rs` still doesn't list these newer crates —
a separate follow-up, not done here.)

**Files (rexops repo only): 4** — `rexops-core/src/component_table.rs` (3 registry
entries + the ordered launchable-list test), `rexops-app/src/snapshot.rs` (live-
count anchor 7→10/11 + the "neutral-not-faulty" test rewritten), `rexops-tui/.../
cockpit.rs` (guard test) + `rexops-tui/src/tools/catalog.rs` (order + stream
lists). Two ceiling tests were rewritten to pin the **terminal state** (0 Planned
rows, nothing fake-green, all 4 Probe+launch tools launchable) instead of chasing
a shrinking Planned set; the `HealthSource::Planned => Unknown` projection arm in
`registry_walk` still guards any future Planned tool.

Quality: four cargo gates green on the final tree — build --all-targets, test
--workspace **303 tests 0 failed**, clippy --all-targets -D warnings, fmt.
Verified at runtime: `rexops components` → **10 live, proto feed-ready**; `launch
tripwire --dry-run` resolves to the installed binary; `launch rewind/rex-forge
--dry-run` refuse honestly ("not on PATH").

**Rollup 10/11 — DONE & MERGED.** Nothing flippable remains; the cockpit redesign
(Phases A–F + the tools-to-Live tail) is finished. Real tails (separate tasks, NOT
done here): add these crates to the installer TOOLS roster in
`crates/linux-ops-install/src/release.rs` so rewind/rex-forge cards go green;
optionally give a tool a real `status` one-line contract for a StatusCommand
upgrade; the pre-existing "hub schema" CI red (fixture `snapshot_v3.json` declares
`schema_version: 3`, hub schema mandates const 4 — a genuine v3→v4 fixture/adapter
migration) is still red on main and was bypassed via `--admin` for this unrelated
PR, exactly as for prior merges.

## RexOps tools-to-Live — rex-check flipped (rollup 7/11)

2026-06-21. Branch `rexops-tools-live`. Status: **MERGED to rexops `main`** —
PR #32, merge commit `83db82c` (impl `e20f8f8`).

Flipped **rex-check / RexDoctor** from Planned to **Live + launchable** using the
**Probe + LaunchSpec** pattern (bulwark/proto), NOT StatusCommand: rex-check has
no `{healthy,detail,latency_ms}` contract, so health = binary presence
(`rex-check --help` exits 0) + a foreground LaunchSpec. Touched only the
launchable-set anchors (rex-check is the 6th launchable, after pulse) and the
live-cards anchor (6/11 → 7/11); the adapter_health/status roster is unchanged
(like proto, an unwired Probe row isn't probed into it). Cockpit guard test
updated. Four gates green (303 tests). Verified: `rexops components` → rex-check
`live`; `rexops launch rex-check --dry-run` resolves.

**Rollup now 7/11 Live.** The remaining three stay **Planned** — deliberately NOT
flipped, because none has a proper status contract and we don't ship fake cards:
- **rex-forge** — no binary installed anywhere (no repo/crate/PATH). Needs its
  binary built/installed first, then a trivial Probe+launch flip like rex-check.
- **tripwire** — binary exists but no `status` contract; its `--json` is a
  Workstate-style envelope (`{schema_version, clean, added, …}`), wrong shape for
  the probe. Best made Live as a **Feed adapter** (map clean→Healthy, drift count
  → vital), not a one-row flip.
- **rewind** — Planned pending a proper status contract / adapter (not yet
  inspected in depth).

NEXT (proper, not fake): for each remaining tool either (A) add a real `status`
subcommand emitting the one-line `{healthy,detail,latency_ms}` contract → Pulse
one-row flip, or (B) build a proper Feed/Probe adapter. Each is a genuine task,
not a one-liner. Reaching 10/11 (the ceiling — workstate=Feed, system=Host are
never "launchable") depends on doing those.

## RexOps Cockpit Phase F — CLI parity (gated `rexops launch`)

2026-06-21. Branch `rexops-cockpit-phase-f` (rexops repo). Status: **MERGED to
rexops `main`** — PR #31, merge commit `3e44028` (impl `a38ad6a`). This is the
**final phase** of the cockpit redesign; roadmap §9 success criteria are met.

Added a gated `rexops launch <tool>` mirroring the TUI confirm-before-run flow:
- **Shared resolver:** moved `resolve_launch_command` + `LaunchCommand` (and their
  helpers + pure tests) from `rexops-tui::tools::launcher` into a new
  `rexops-app::launch` module. Both front-ends already depend on rexops-app and
  the resolver is pure (registry + AppConfig + `which`), so no new dep edges;
  rexops-tui re-exports the two symbols → every TUI call site unchanged.
- **`rexops launch <tool> [--yes] [--dry-run]`** with a pure, exhaustively-tested
  `decide()` gate: unresolved→refuse(exit 1); `--dry-run`→print argv, run nothing
  (wins over `--yes`); `--yes`→run; interactive y/N→run/abort; non-interactive &
  !`--yes`→refuse(exit 1, never hang/run-blind). Child exit code propagated.
  `--dry-run` reuses the resolved argv that would run (CR-2, no preview/run drift).
- README documents `launch` + the previously-undocumented `components`.
- No new binaries/wrappers/aliases. Background launches out of scope for v1.

Quality: four cargo gates green (build --all-targets, test --workspace **303
tests** 0 failed, clippy --all-targets -D warnings, fmt); workspace stable 6/6
under default parallelism. Smoke-tested help/refuse/non-tty/dry-run. CI "Rust
verification" passed on PR #31. (The "hub schema" check is still pre-existing red
— see Phase E note; NOT bumped here because it's a real v3→v4 adapter migration,
not a fixture tweak: the umbrella schema mandates schema_version const 4 while the
rexops workstate adapter parses/asserts v3 and skips unknown versions. Tracked as
a follow-up.)

REMAINING (the redesign's longer tail, not blockers): flip the 4 still-`Planned`
tools (rex-check, tripwire, rewind, rex-forge) to Live — each = give the tool a
`status` contract + one registry-row flip + extend the two literal roster anchors;
rollup climbs 6/11 → 10/11. Plus the v3→v4 Workstate schema migration.

## RexOps Cockpit Phase E — Pulse Live via StatusCommand + Heartbeat widget

2026-06-21. Branch `rexops-cockpit-phase-e` (rexops repo). Status: **MERGED to
rexops `main`** — PR #30, merge commit `3990e74` (branch tip `340ef1a`).

Promoted **Pulse** from a `Planned` card to a launchable `Live` cockpit component
driven by a real `status` contract, plus a per-component **heartbeat** vital:
- `HealthSource::StatusCommand` wired end-to-end — the `COMPONENTS` loop in
  `build_snapshot_with_piped` spawns `pulse status`, bounds it by the configured
  timeout, and parses the one-line JSON liveness contract
  (`{healthy, detail, latency_ms}`) into `AdapterHealth` + latency. Parse/health
  mapping is pure in `status_probe`; spawn glue is in `snapshot.rs`.
- Pulse flips `Planned → Live` and gains a `LaunchSpec`; banner rollup `5/11 → 6/11`.
- Heartbeat vital: transient per-component ring buffer of probe latencies feeds the
  suite-ui Heartbeat sparkline (suite-ui pin bumped `2f5fa82 → 49b71e9`). A
  `Degraded` probe (no latency sample) falls back to the probe `detail` as the vital.
- Roster tests re-anchored to a literal component set so the Pulse flip is explicit.

**Test-robustness fix landed in the same PR (commit `340ef1a`).** `cargo test
--workspace` was intermittently red (~5/5 under load, green in isolation) on
`degraded_status_command_detail_surfaces_as_component_vital`. Root cause (proven by
reproduction: failure vanished under `--test-threads=1`/`--nocapture`, and a
standalone stress harness reproduced **raw OS error 26** deterministically): the
test helper wrote a fresh executable shell script and spawned it; under `cargo
test`'s parallelism a `fork` for an unrelated spawn held that file's write handle
across its exec window, so Linux rejected the exec with **ETXTBSY** ("Text file
busy") and the probe reported `Unavailable("not found")`. Unique filenames/dirs
don't help — the contended state is the process fd table. Fix: **never execute a
freshly written file.** The stub is now a non-executable data script that `/bin/sh`
*reads* as an argument; a new empty-by-default `AdapterConfig::status_prefix_args`
is injected before the status subcommand's own args, so the probe spawns
`/bin/sh <script> status`. Production behaviour unchanged (prefix empty for every
real adapter; field is `#[serde(default, skip_serializing_if)]` → config files
wire-compatible). A 16-thread stress that gave 130 ETXTBSY failures gives 0; the
workspace suite is now **green 20/20 under default parallelism**.

Quality: `cargo build --workspace --all-targets`, `cargo test --workspace`
(rexops-core 65 / app 27 / tui 176 / cli 1, 0 failed), `clippy --workspace
--all-targets -D warnings`, `fmt --all --check` — all green. CI "Rust verification"
passed on PR #30. (The "Validate Workstate fixture against hub schema" check is red,
but that is **pre-existing cross-repo drift** — the umbrella's schema now wants
`schema_version: 4` while rexops's fixture is v3; Phase E touched no fixtures/schemas,
and `main`'s own CI has been red on that check throughout the A–E series, which all
merged on Rust-verification-green.) No binaries installed, no wrappers/aliases added.
Manual TTY smoke of the live cockpit remains deferred (non-TTY env).

## Previous: RexOps Cockpit Phase D — FeedReady tools (registry = single launch source)

2026-06-21. Branch `rexops-cockpit-phase-d` (rexops repo, worktree
`.claude/worktrees/rexops-cockpit-phase-d`, off origin/main 04b41a8). Spec → plan →
inline TDD. **6 tasks committed on the branch, NOT pushed** — PR to rexops main is
a separate gate.

Promoted ScriptVault + ToolFoundry from data-only cards to launchable `Live`
cockpit components, by unifying the two launch sources into one:
- **Registry is now the single source of launch data.** `tools/catalog.rs`'s
  hand-maintained `CATALOG`/`ToolEntry` deleted; `tools::catalog` is a thin view
  over `rexops_core::COMPONENTS` (`launchable()` = `launchable_components()`).
  `resolve_launch_command`, the Launcher screen, palette, availability cache, and
  `is_streamable`/`refreshes_after` all read the registry `LaunchSpec`.
- `Component` gained `blurb` (the human description the Launcher/palette need).
- ScriptVault + ToolFoundry got a `LaunchSpec` (Foreground, bare) + `Maturity::Live`;
  the launchable set is now `[bulwark, proto, scriptvault, toolfoundry]`.
- **Semantic change:** `live` now means "fully wired" = adapter roster + feed-backed
  launchables (the snapshot.rs invariant test was renamed + split accordingly; the
  two cross-source rosters `status`/`adapters` are unchanged — feeds aren't adapters).
- Guard test `launcher_list_is_exactly_the_registry_launchable_set` locks the
  Launcher list to the registry, so the two sources can never drift again.

Launch resolution unchanged (`which <id>` then config `binary`) — no binaries
installed, no wrappers/aliases added; the tools launch the moment one word resolves.
Workspace green: rexops-core 65 / app 17 / tui 168, full `cargo test --workspace` +
`clippy --workspace -D warnings` + `fmt` clean. Headless smoke (`rexops components`
on the fixture) shows ScriptVault + ToolFoundry as `live` with vitals.

## RexOps Cockpit Phase C — interactive cockpit

2026-06-21. Branch `rexops-cockpit-phase-b` (in the rexops repo, worktree
`.claude/worktrees/rexops-cockpit-redesign-doc`). Built via spec → plan → inline
TDD execution. **8 task commits on the branch, NOT pushed** — PR to rexops main is
a separate step Tom gates.

Made the Phase B cockpit landing screen interactive, all 4 requirements met:
- **Card focus** (`j`/`k`/arrows), keyed by component `id` (`App::selected_component`)
  so it survives reordering refreshes; a snapshot auto-focuses the first card.
- **Per-card letter hotkeys** — each card shows a dim `[a]` marker; pressing it
  arms that component through the **existing** `arm_tool → pending_action` confirm
  gate (no new launch path). Marker alphabet curated to exclude every bound nav key
  + digits `1`–`7`; marker/focus order share one source of truth
  (`cockpit_nav::cockpit_visit_order`).
- **Drill-down** — new `Screen::CockpitDetail` (`screens/cockpit_detail.rs`) joins
  the static registry row with the live `ComponentStatus`. `g` drills any card,
  `Enter` drills a non-launchable card (and launches a launchable one), `Esc` backs
  out.
- **Phase B rendering frozen** — StatusCard gained additive `marker`/`focused`
  fields; all Phase B render tests still pass.

New files: `screens/cockpit_nav.rs`, `screens/cockpit_detail.rs`,
`app/tests/cockpit.rs`. Workspace green: rexops-tui lib **163 passed**, full
`cargo test --workspace` + `clippy --workspace -D warnings` + `fmt` clean at every
commit. Headless smoke (`rexops components` fed the Workstate fixture) lists all 11
components with vitals; interactive keypress smoke deferred (ran in a non-TTY job).

## rex-forge v0.1 — new scaffolder crate

2026-06-20. Branch `worktree-rex-forge-spec` (isolated worktree off main). Built
end-to-end via spec → plan → inline TDD execution; **committed on the branch, NOT
pushed** (PR to main is a separate step Tom gates).

- New crate `crates/rex-forge` (13th workspace member): TUI-first scaffolder for
  Rust/Go. Pure engine (resolve → render → merge → in-memory `FileTree`) behind a
  clap CLI + a suite-ui TUI; single `writer` module is the only fs/git boundary.
- Component library authored in-tree under `library/` (`.toml`+`.j2`), embedded via
  `include_dir`; `build.rs` validates every `[[inject]]` anchor against its base.
- Bases: rust-bin, rust-lib, go-bin, go-lib (secure-by-default: forbid-unsafe +
  clippy lints + pinned toolchain). Components: Rust clap/config/tracing/metrics/
  anyhow/thiserror/dockerfile/ci-github; Go flag/slog/dockerfile/ci-github
  (stdlib-only — cobra/viper/zap deferred to v0.2 to keep the offline compile-gate).
- Tests: 46 total — resolver/merge/render/writer/state units, golden snapshots
  (determinism), and a compile-gate that actually `cargo build`s generated Rust and
  `go build`s generated Go (CI gained a Go toolchain step). fmt + clippy `-D
  warnings` clean.
- Two real bugs caught by the compile-gate and fixed: cross-language component
  name collisions (`ci-github`/`dockerfile`) → base/language-aware lookup; and
  Cargo dep-merge corrupting Go `go.mod` → dep-merge is now Rust-only.
- Spec + plan in `docs/superpowers/{specs,plans}/2026-06-20-rex-forge*`.
- REMAINING (human-only): interactive PTY smoke of `rex-forge new`; the no-TTY
  path + pure state-machine flow tests cover the wiring, but a real terminal
  walk-through hasn't been done.

## Release 0.2.0 prep — umbrella

2026-06-20. Branch `release/0.2.0` off main at d2e5d87. Committed on the branch,
**NOT pushed until approved** (PR to main, then tag is a SEPARATE step Tom gates).
Umbrella-only release (sibling `scripts/release.sh` intentionally NOT run).

- Bumped `[workspace.package] version` 0.1.2 → **0.2.0** (one edit; all 12 crates
  inherit via `version.workspace = true`). `cargo build` refreshed Cargo.lock.
- MINOR bump per SemVer: two new library crates (suite-ui, thomas-tui) + suite-core,
  five new tools graduated in (conductor, rewind, tripwire, portman, rex-doctor),
  Pulse's TUI rewritten onto suite-ui, broad suite-core refactor — mostly additive;
  pre-1.0 so 0.2.0 (not 1.0.0).
- Added **CHANGELOG.md** (Keep a Changelog format) with a full 0.2.0 section
  (Added/Changed/Fixed/Notes from the 94 commits since v0.1.2) + backfilled
  0.1.x stubs + compare links.
- README: the manual `gh release create` examples now use a `vX.Y.Z` placeholder
  instead of a pinned stale version.
- Gate before PR: cargo build/test/clippy -D warnings/fmt --all green.
- TAG IS DEFERRED: `v0.2.0` triggers `.github/workflows/release.yml`, so the tag
  is pushed only AFTER this PR merges to main and CI is green — Tom gives the
  go-ahead first.

## Pulse TUI migration to suite-ui (T1–T10) — pulse

2026-06-20. Worktree `.claude/worktrees/pulse-suite-ui-migration`, branch
`worktree-pulse-suite-ui-migration`, off main. Committed on the branch, **NOT
pushed, no PR** (awaiting Tom). Executed the rewrite plan
`docs/superpowers/plans/2026-06-20-pulse-tui-rewrite.md` (PULSE TUI REWRITE
PLAN). Pulse was a deliberately no-ratatui, hand-rolled ANSI-string renderer +
libc `termios` driver; it now renders entirely through the shared **suite-ui**
chrome (over ratatui + crossterm). Net ~1300 lines deleted.

T1 (b1682a8) — deps: added suite-ui + ratatui + crossterm (+ insta dev) to
pulse; suite-ui added to `[workspace.dependencies]`.
T2 (1db66ea) — terminal driven by `suite_ui::Tui` (hide_cursor + require_tty),
behind a temporary monochrome Paragraph bridge so the app kept running; input
moved to crossterm (`tui::read_event` → unchanged `Key` enum); cockpit hand-off
to `Tui::suspended` (flattening the io::Result<io::Result<()>>).
T3 (9efb29a) — crossterm adapter confirmed as the sole production input; tested
the KeyCode→Key mapping incl. search-box literal letters; `read_key` marked
legacy/test-only.
T4 (66e044d) — resolve `suite_ui::Theme`; `--theme cyan|amber` + `--color
auto|always|never` parsed (invalid → exit 2); NO_COLOR now routes through
suite-ui's gate.
T5 (189d9de) — new `crate::view` draw layer; the default verdict screen drawn in
real ratatui (KeyHints footer, Theme colour, display-width truncation), faithful
geometry; 3 insta snapshots.
T6 (79dfec4) — all drill-downs ported: Attention (`SeverityBadge`/`EmptyState`),
Feeds (`HealthStrip`), Details, Help (`HelpSheet` overlay), Search (`SearchBar`);
a `suite_severity` shim maps domain Severity → suite_ui::Severity at the draw
boundary; sample fixture moved to `verdict::sample_readings`; 5 snapshots.
T7 (e922533) — transient status overlay covered from the draw side (bottom-row
placement across views, width truncation).
T8–T10 (5c42874) — headless `--dump-view`/`--state` render through ratatui
(`view::render_to_string` via TestBackend; `App::from_verdict`/`Readings::empty`
back the one-shot); then DELETED the entire legacy renderer (main.rs
render/Style/Line/clip_ansi/…; app.rs frame/view_*/panel) and the termios driver
(tui.rs RawMode/paint/install_panic_guard/read_key + extern "C" block, 490→108
lines). Docs refreshed.

State: 66 pulse tests pass (navigation state machine intact + ~15 ratatui draw
tests incl. 8 insta snapshots); `cargo test/clippy -D warnings/fmt --all` green
across the workspace; no sibling crate regressed. PULSE_DESIGN.md gained an
"Implementation note" (design intent unchanged). T11 (reusable loop template for
the next tool) deliberately SKIPPED as YAGNI — tool #2 isn't being built. **Not
yet done:** manual smoke in a real terminal (live resize, cockpit round-trip, 3
themes/NO_COLOR) — can't be unit-tested; recommended before merge.
Intentional restyles shipped: `[CRIT]` badge, glyph health strip, Help-as-overlay,
`•`-separated KeyHints footer.

## suite-ui design review fixes (R1–R4) — thomas-tui + suite-ui

2026-06-20. Worktree `.claude/worktrees/suite-ui-design-doc`, branch
`worktree-suite-ui-design-doc`, off main at 06ca26a. Committed on the branch,
**NOT pushed, no PR** (awaiting Tom). Implemented the design review in
`docs/design/suite-ui/SUITE_UI_DESIGN.md` per the plan
`docs/superpowers/plans/2026-06-20-suite-ui-fixes.md`.

R1 [bug] — `thomas-tui` `truncate_path`/`truncate_desc` counted `char`s, not
display columns, so wide CJK/emoji (1 char = 2 cols) overflowed the cell budget
and corrupted table layout while the doc-comment claimed "Unicode-safe". Fixed to
measure UAX#11 width over grapheme clusters (`unicode-width` +
`unicode-segmentation` — the crates ratatui uses internally); post-condition is
now ≤ max columns; added CJK/emoji/combining-mark/ZWJ tests. (892ad9f)

R2 — widget API was inconsistent and nothing implemented `ratatui::Widget`, so the
chrome didn't compose into the ecosystem. Added a public `ThemedLine` trait + one
blanket `impl Widget for &Themed<W>` so every one-line widget gets `.themed(theme)`.
Blanket-over-local-trait was required: a direct `impl Widget for &Themed<OurWidget>`
in suite-ui trips the orphan rule (E0117); implementing the local `ThemedLine` on
suite-ui's own types is legal. suite-ui's StatusBar/AttentionFlag/HealthStrip opt
in. Documented the span/line/render-by-shape contract in both crate-docs.
(abc14be, e41d116, b1630d8)

R3 — added `insta` geometry snapshots: thomas-tui (pane + a CJK-in-narrow-pane
shot that is the R1 regression net — the right border stays intact) and suite-ui
(status footer). Note: ratatui 0.29 `Buffer` has NO `Display`, so snapshots render
the cell grid to a string first. `assert_buffer_eq!` was never used (don't-regress
note only). (3ecb154, 3ccdbc3)

R4 — done as the **additive** variant Tom chose (not the breaking enum collapse):
added `JobState::outcome() -> Option<Outcome>` and routed `line()` through it so the
`JobState→Outcome` mapping lives once; `Done{ok}`/`Cancelled` variants unchanged →
no consumer migration. The full `Done{ok}`→`Finished{outcome}` collapse stays
recorded in design-doc §7 as the eventual cleanup. (b8af919)

Also: deps added to the workspace (d8ac1fa); two-layer (`thomas-tui` toolkit /
`suite-ui` shim) split now headlines §2 and both crate-docs; design doc marks
R1–R4 fixed (7e54115). Gate: `cargo build --workspace` green, thomas-tui 92+2+13
and suite-ui 29+1+5 tests green (142 total, +38 over baseline), clippy `-D warnings`
clean, fmt clean, gallery renders in all 3 themes. R5/R6 are documentation-only
(no code action). color_eyre "restore-first" panic note is consumer-side guidance
in design-doc §8.

## new-tools review — LOW findings (tripwire/portman/pulse)

2026-06-20. Worktree `.claude/worktrees/fix-low-findings`, branch
`worktree-fix-low-findings`, off main at d3db6e1 (after the HIGH+MEDIUM merge).
NOT committed yet. The deferred LOW findings from the new-tools review:

L2 — tripwire `changed_fields` reported NO content drift on a content-tracking
toggle (`baseline.rs`). When one run had a hash and the other didn't (content=
true→false) with no readability flip, the (Some,None) pair hit `_=>{}` and was
silent. Fix: collapse the not-both-hashed arms to fall back to Size drift, so a
hash↔no-hash transition with a changed size now surfaces.

L3 — tripwire watch.conf parser truncated paths containing `#` (`watch.rs`). It
stripped from the first `#` anywhere on the line, but `#` is legal in a filename
(`/var/data#v2.log`) → wrong file watched. Fix: a `#` is a comment only when it
starts the (trimmed) line; inline `#` is kept verbatim.

L4 — portman + tripwire `json_string` produced invalid JSON for control chars
(`main.rs` ×2). The one-line baseline confirmation envelope escaped only " \ \n,
leaving \r \t and other U+0000–U+001F raw (RFC 8259 violation). Fix: add \r \t
\b \f and a \u00xx fallback for all control chars; identical helper in both. Both
binaries gained their first `#[cfg(test)] mod tests`.

L6 — pulse silently mis-ranked unknown severities (`sources.rs`). The rexops
path did `unwrap_or(Severity::Low)` and the bulwark path `?`-dropped unknowns, so
a novel/misspelled severity from a producer sank below the High threshold or
vanished. Fix: unknown severity now escalates to High in both paths (surface, not
bury), and Severity::parse is case-insensitive ("CRITICAL" now parses).

L5 (pulse Esc blocking read, `tui.rs`) — DEFERRED, not fixed. read_key takes
`&mut impl Read` with no access to the tty fd, and the lone-Esc fix requires
toggling termios VTIME around the Esc peek; threading the fd through breaks the
in-memory test seam. Low-frequency (only high-latency terminals) and the proper
fix is disproportionate/untestable through the current seam — left as-is.

5 new regression tests (content_toggle_with_size_change_is_not_silent,
hash_in_path_is_kept_but_leading_hash_is_a_comment, json_string_escapes_control_
chars_to_valid_json ×2, unknown_or_uppercase_severity_escalates_not_drops).
Verification: `cargo test --workspace` 593 passed / 0 failed; clippy 0 warnings;
fmt clean. All review findings now addressed except the deferred L5.

---

## new-tools review — M1–M6 MEDIUM fixes (conductor/tripwire/portman/pulse/rewind)

2026-06-20. Same worktree `.claude/worktrees/fix-top2-portman-rexdoctor`
(branch `worktree-fix-top2-portman-rexdoctor`). NOT committed yet. Continues the
HIGH fixes below with the six MEDIUM findings:

M1 — conductor confirm modal could advertise a different argv than it spawns
(`plan/rules.rs`, `run.rs`, `plan/mod.rs`). A finding `what` / job `title` with
a space was `format!`'d raw into the command string, then `argv_of` split it on
whitespace → e.g. modal shows `proto show Nightly Backup` but spawns
`[proto,show,Nightly,Backup]`. Fix: added `plan::quote_arg` (single-quotes a
value with whitespace; no-op for the common single-word case) used at the two
interpolation sites, and made `argv_of` a quote-aware splitter that is its exact
inverse — the displayed string and the spawned argv are now provably the same
list. Not shell injection (arg0 still validated vs SUITE_BINARIES, no shell).
Kept `command: Option<String>` so the `--dump-view` JSON schema is unchanged.

M2 — `Baseline::save` truncated a valid baseline to empty on a serialize error
(`tripwire/baseline.rs:50`, `portman/baseline.rs:45`). Both did
`to_string_pretty(self).unwrap_or_default()` → `""` → `fs::write`. Fix:
serialize first and map a serde error to `SaveFailed{source: io::Error::other}`
before touching the file. (rewind `store.rs` already used `.map_err`, no change.)

M3 — conductor swallowed a terminal suspend/re-enter failure (`tui/mod.rs:142`
`let _ = spawn`; `run.rs:85` `Err(_) => Ran(false)`). A broken terminal showed
as "that step failed (exited non-zero)" or nothing. Fix: new
`RunOutcome::SpawnError(String)`; `run_current` flags the step Failed with a
"terminal may need a redraw" notice; the `r` rexops handoff now surfaces its
Err. NOTE: the pulse half of this finding was already correct — `RawMode::suspend`
returns Err and `cockpit::open` maps it to `LaunchOutcome::Failed` (visible
status line), so pulse was left as-is.

M4 — pulse misreported scriptvault as Missing when rexops is down
(`verdict.rs`). The `vault`→`scriptvault` name map was applied to the rexops
lookup but NOT the binary fallback (`find(|b| b.name == name)`), so an installed
scriptvault read as Missing instead of Stale. Fix: compute `key` once up front
and use it for both lookups; corrected the test fixtures that used the wrong
`vault` binary name.

M5 — docs/comment drift, NOT a behavior bug (`plan/rules.rs`, `plan/mod.rs`).
Comments/test name implied capture fires only when a Ring-2 step follows, but
investigate/review steps are Ring-1 and the real trigger is "work exists"
(finding or failed job). Changing behavior to match the comment would disable
capture entirely (worse), so aligned the docs to the actual, correct trigger and
renamed the misleading test.

M6 — rewind capture slurped whole files into one Vec (`scan/mod.rs:151`), OOM
risk on a misconfigured huge path. No streaming `hash_file`/`put_file` actually
exists (the doc that promised it was aspirational) and the capture set is small
suite JSON by design, so the proportionate fix is a 64 MiB cap: a larger file is
recorded `unreadable` (size still noted) instead of read into memory. A full
streaming store rewrite was judged over-engineering for intentionally-small
inputs.

Verification: `cargo test --workspace` all green (conductor 88, rewind 98,
pulse 55, portman 13, suite-core 37, tripwire 86+27, …) incl. 6 new regression
tests confirmed by name (quoted_id_with_spaces…, argv_of_round_trips…,
spawn_failure_is_its_own…, vault_binary_present…, oversize_file_is_marked…,
real_work_present_forces…); `cargo clippy --workspace --all-targets` clean.
Remaining: LOW findings only (to_string_lossy collisions, tripwire content-toggle
edge, '#'-in-path config split, control-char JSON escaping, pulse Esc blocking
read). See memory `suite-newtools-review-2026-06-20`.

---

## new-tools review — top-2 HIGH fixes (portman owner + rex-doctor is_readonly)

2026-06-20. Worktree `.claude/worktrees/fix-top2-portman-rexdoctor`, branch
`worktree-fix-top2-portman-rexdoctor`, off `main` at b8de4d7. NOT committed yet
(awaiting Tom's review). From the 7-crate new-tools review, fixed the two HIGH
findings:

H1 — portman socket-owner misattribution (`crates/portman/src/scan/owner.rs`).
`/proc` is walked in inode order, not pid order, so for a socket-activated
listener (systemd pid 1 holds the fd and passes it to the real daemon) pid 1
often landed first and `or_insert` froze it as the owner → port 22 showed
`systemd(1)` instead of `sshd`. Added pure `should_replace_owner(existing,
candidate)` = `existing==1 && candidate!=1`; a real service now supersedes a
recorded pid 1, a recorded non-1 pid is never displaced, first-writer still
wins for ordinary pids. Unit test covers all four cases. **Verified live**:
`portman` now prints `0.0.0.0:22 … sshd(1305) ssh.service`.

H2 — rex-doctor `env.writable` false PASS (`crates/rex-doctor/src/checks/
env.rs`). `is_readonly` tested `mode & 0o200` (owner-write bit) ignoring *who*
the owner is, so a root-owned `0755 ~/.local/bin` (the sudo-install case it
claims to catch) reported as writable for a non-root user. Added
`suite_core::path::is_writable_dir(dir)` — asks the kernel via `access(2)`
W_OK (extern "C", no libc-crate dep, matching the existing isatty/geteuid
pattern) — and rewrote `is_readonly` to `dir.exists() && !is_writable_dir(dir)`.
Test in suite-core asserts /usr/bin is non-writable to a non-root user (skipped
under root) and a tempdir is writable.

Verification: `cargo test -p suite-core -p rex-doctor -p portman` → 37/27/15
pass incl. both new tests by name; `cargo clippy … --all-targets` clean (0
warnings). Remaining review findings (M1 conductor argv split, M2 baseline
save-empties ×3, M3 TUI raw-mode err, M4 pulse vault name, M5/M6, LOWs) NOT
touched — see memory `suite-newtools-review-2026-06-20`.

---

## suite-core — new shared foundation crate + 7-crate migration

2026-06-20. Branch `worktree-suite-core` (worktree
`.claude/worktrees/suite-core`), off `main` at 98af8dc. Committed, NOT pushed
(awaiting review). 8 commits; +629/-432 across 28 files; net source shrinks.

Created `crates/suite-core/` — the suite's first shared NON-UI library
(peer to thomas-tui/suite-ui), ZERO third-party deps (std + isatty/geteuid
externs only). Four modules consolidate helpers that were copy-pasted
(mostly byte-identical) across the tool crates:
  env  : stdout_is_tty (isatty), is_root (geteuid), home_dir ($HOME)
  path : is_executable_file (mode & 0o111), resolve_on_path, which
  xdg  : data_dir(tool), config_dir(tool), expand_tilde
  fmt  : human_size (1024-based, "B/KB/MB/GB/TB")

Migrated 7 consumers, each keeping its public fn names (util.rs became thin
re-export shims; call sites untouched): rewind, tripwire, portman,
rex-doctor, rex-check, conductor, pulse. Crate-specific bits stayed local
(rewind store_dir/config_path; tripwire baseline_path; portman baseline_path;
rex-doctor SUITE_BINS + which_all + dir_on_path; rex-check command_exists +
stdin_is_tty/is_tty; conductor data_root — the bare no-suffix XDG root it
needs to read OTHER tools' subtrees, rebuilt on suite_core::env::home_dir).

TWO intentional, approved behavior changes:
  1. pulse `cockpit.rs::resolve_on_path` BUG FIX — it matched on is_file()
     only (no exec bit), so a non-executable file shadowing `rexops` on
     $PATH could be picked for launch; now delegates to suite-core which
     always checks the exec bit.
  2. rex-check `human_size` units standardized from "K/M/G" (no space) to
     the suite-wide "KB/MB/GB" (with space); its test expectations updated.

NOT extracted (deliberately, this pass): the 251-line hand-rolled Sha256
(byte-identical in rewind+tripwire) and the per-tool error.rs enums.

Design doc: docs/superpowers/specs/2026-06-20-suite-core-design.md.
Verify (whole workspace): cargo build --workspace clean; cargo test
--workspace = 581 passed / 0 failed; clippy --workspace --all-targets -D
warnings clean; fmt --all --check clean.

Footnote: 3 parallel subagents (rex-check/conductor/pulse) were dispatched
but each correctly refused — `worktree` isolation branched them from main,
which predates suite-core, so the crate wasn't in their workspace. Did all 3
in-place here instead. (They returned two useful fixes, both applied above:
rex-check's PATH helper is `command_exists`, and is_tty is shared with
stdin_is_tty so it can't be deleted.)

## Conductor Phase 3 — the driver (Ring-2 + confirm modal + orchestrate)

2026-06-19. Branch `conductor-phase3` (worktree
`.claude/worktrees/conductor-design`), off `main` at 602c992. Built on Phase 2;
all prior tests still pass.

Bare `conductor` and the new `conductor orchestrate` verb now DRIVE the plan:
Enter on a changes-state step opens a confirm modal showing the literal command;
it spawns the sibling only on `y` (a stray Enter can never fire a state change),
with `s` skip / `q` back. `run.rs` no longer refuses Ring-2 (the gate moved to
the TUI); every other guard is intact (known-binary-only, $PATH check, fixed
argv, NO shell). A step that runs and exits non-zero is marked Failed (new
StepStatus variant, ✗). The guided run returns a RunReport mapped to exit codes:
0 clean/all-done/nothing-to-conduct, 1 a step failed, 2 quit with pending/skipped
(failure outranks unfinished), 3 can't-run — for BOTH bare and orchestrate.
Non-TTY / --json still falls back to `status`. Conductor still writes zero live
files with its own code. No new dependency; rules + JSON envelope unchanged.
Tests: the full confirm-gate matrix + RunReport mapping + `--dump-view confirm`,
all green (85 lib + 13 cli); clippy -D warnings + fmt + `cargo build --workspace`
clean.

## Conductor Phase 2 — interactive TUI + Ring-1 spawning (crates/conductor)

2026-06-19. Branch `conductor-phase2` (worktree
`.claude/worktrees/conductor-design`). Built on top of Phase 1's read-only
foundation; all Phase 1 tests still pass.

Phase 2 delivered: **interactive TUI** (hand-rolled, no new third-party deps —
the same raw-terminal discipline as `pulse`): bare `conductor` on a real TTY
opens the plan view; stdout piped or non-TTY falls back to `status` (scripts
and CI keep working unchanged). The TUI is the
`tui/{mod,term,frame,style,run}.rs` stack built in prior tasks, now wired into
`main.rs`.

**Ring-1 read-only spawning:** `enter` on a read-only step suspends raw mode,
hands the terminal to the sibling binary (direct `execvp`-style spawn, no shell,
`$PATH` probed), and marks the step `✓` on return. The `SuspendSpawner` RAII
pattern restores raw mode unconditionally (even on panic).

**Ring-2 no-op with note:** `enter` on a changes-state step prints "this step
changes state — needs Phase 3, not run" and stays put. No state-changing command
is ever run in Phase 2. Zero writes, zero Ring-2 executions — the invariant
holds.

**`--dump-view <VIEW>`:** hidden flag for deterministic snapshot tests. Builds
the real plan, renders exactly one frame (`plan` | `healthy` | `compact` |
`help`), monochrome, and exits 0. Unknown view → stderr + exit 3.

**Tests:** 3 new integration tests in `crates/conductor/tests/cli.rs`, reusing
the existing `TempRoot` + `run()` harness (stub-bin-dir pattern): (1)
`--dump-view plan` on a stale-feed state shows "the plan" + "workstate
snapshot" + "changes state"; (2) `--dump-view healthy` with all bins stubbed
and empty data dir shows "nothing to conduct"; (3) bare invocation with captured
stdout (non-TTY) falls back to status text. All 10 integration tests + all unit
tests green.

Exit codes: 0 ok / 3 can't-run only — 1/2 still reserved for Phase 3.
`#[allow(dead_code)]` on `run` and `should_run_interactive` in `tui/mod.rs`
removed (main.rs now calls both). No new dependencies added.

All gates green: `cargo test -p conductor` (all tests pass incl. 3 new),
`cargo clippy -p conductor --all-targets -D warnings` (zero warnings),
`cargo fmt -p conductor -- --check` (clean), `cargo build --workspace` (clean).
NOT pushed — awaiting human approval per the hard rule.

---

## New tool: Conductor — suite guided operator (crates/conductor), Phase 1

2026-06-19. Designed and built Phase 1 of Conductor, the suite's GUIDED
OPERATOR / "brain": it reads the suite's own state and turns it into a short,
deterministic ORDERED RUNBOOK ("do these things, in this order"), delegating
every action to the tool that owns it. It fills the gap between Pulse (the calm
read-only *verdict*) and RexOps (the free *launcher*): Conductor is the
opinionated *sequence* — what to do, in what order — and is the one tool that
correlates ACROSS tools (signature move: a tripwire drift on the same file as a
bulwark finding gets lifted to the top, annotated "start here").

Design-first (Tom's process): wrote `CONDUCTOR_DESIGN.md` at the repo root
(mirrors PULSE/REWIND/TRIPWIRE_DESIGN.md), got sign-off, then a 10-task TDD plan
at `docs/superpowers/plans/2026-06-19-conductor-phase-1.md`. Tom locked the two
forks: built-in rules from state (no config language in v1) and "spawn, never
write itself".

Safety model — THREE RINGS, the heart of the tool: Ring 0 = Conductor's own code,
always read-only (reads contracts, probes $PATH, builds + renders the plan); Ring
1 = spawn a read-only sibling; Ring 2 = spawn a state-changing sibling ONLY after
a deliberate confirm. The rule: **Conductor never mutates state with its own
code** — the most it ever does is type a command a human could have. No shell
(direct argv), no --yes-to-all, no unattended runs.

Phase 1 shipped (this branch, 8 commits): the entire READ-ONLY foundation, 100%
Ring 0 — `conductor status` (situation + ordered plan), `conductor health`
(per-feed/per-tool readiness), `conductor plan` (steps only), all with `--json`,
`--no-color`, `--data-dir`, `-v`. NO subprocess, NO TUI, NO writes (those are
Phases 2–3). Bare `conductor` prints status (scriptable + RexOps-launchable).
Exit 0 ok / 3 can't-run; 1/2 reserved for the Phase 3 `orchestrate` driver.

Built to the suite house style: thin `main` (clap, rewind's shape) → library
does the work → renderers derive from the model. Modules each have ONE job:
`sources.rs` reads contracts fault-tolerantly (lifts pulse's discipline:
missing/malformed ⇒ "unavailable", never panics), `state.rs` holds normalized
facts (no I/O), `plan/rules.rs` is the pure `&SuiteState -> Plan` rule engine
(the brain, densest tests), `report.rs` renders human + the suite JSON envelope.
The 7 v1 rules in priority order: refresh stale feeds → wiring-gap fix commands →
safety-capture (only when real work follows) → investigate findings (worst-first,
drift-correlated lifted + annotated) → review failed jobs → else "nothing to
conduct". Tom's upgrade: stable kebab `step.id` + deterministic `plan_id`
(FNV-1a) in the JSON envelope, for the future Phase 3 driver.

Tests (44 unit + 7 integration, all green; clippy + fmt clean; full workspace
builds): the rule engine's full synthetic matrix passes incl the signature
drift×finding correlation; readers covered by temp-dir failure-mode tests; the
CLI covered end-to-end. ONE debugging catch worth noting: the integration tests'
"empty suite ⇒ nothing to conduct" cases failed on this dev box because 5 of the
8 suite binaries are actually installed (so the wiring-gap rule fired) — fixed
honestly by stubbing ALL 8 suite binaries in the test sandbox and pointing PATH
there, so the probe is deterministic across machines (not by special-casing).

Registered as a workspace member (`crates/conductor` in root Cargo.toml) and in
the installer (`conductor:conductor` in WORKSPACE_TOOLS — one line; the
build/verify/uninstall loops already iterate it). Bare binary on PATH, NO
r-conductor wrapper, NO alias (per Tom's standing rule).

Phase 2 (the TUI + Ring 1 read-only spawns) and Phase 3 (the `orchestrate`
driver + Ring 2 confirm — the only writing path, heaviest gate) are separate
plans, not started.

## Rewind Phase 2 — `show`, `diff`, capture selectors + timeline marker

2026-06-19. `crates/rewind/`. Added the read/compare half of Rewind on top of
Phase 1's storage layer:

- **Selectors** (`lib.rs::resolve_selector`, pure): `latest`, `latest-good`
  (newest capture whose snapshot is a valid `workstate` envelope), `~N` relative
  index (digits-only — `~+1`/`~ 1`/`~x` miss), full id, and unique id prefix.
  Every miss/ambiguity → existing `RewindError::UnknownCapture` (exit 3). Labels
  are never selectors.
- **`rewind show <capture>`** — one capture's manifest; `-v` adds mode/owner/hash
  prefix/mtime. Per-entry note word (unreadable > symlink target > envelope).
- **`rewind diff <a> [<b>]`** — capture-vs-capture, or capture-vs-**live** when
  `<b>` omitted. Identity is the content hash, never mtime/mode/owner (byte-equal
  + new mtime = unchanged); two unreadable files = `changed` (honest, not a false
  "unchanged"). Exit 1 on any difference, 0 when clean (policy in `main.rs` only).
- **Capture-vs-live re-walks the set** (read-only `scan::live_scan`, no store
  write) so a NEW/DELETED file under a captured recursive dir counts as drift —
  the honest cron "did anything change?" answer (user's chosen semantics).
- **Diff footer is zero-suppressed** (`1 changed · 1 added · 1 unchanged`;
  clean → `no differences`) — user's chosen style. JSON always carries all four.
- **Timeline** gained a `latest` marker column on the newest row (JSON unchanged).

Key refactor: `scan::entry_for` now takes `Option<&Store>` — `Some` persists the
blob (capture), `None` hashes in memory via the existing `hash::hex_of` (diff);
both produce identical entries (hash + sniffed envelope parity, tested).

New module `diff.rs` (Change/ChangeKind/Diff, kind-aware classifier). No new
deps; `error.rs`/`model.rs`/`store.rs`/`Cargo.toml` unchanged. Gate green:
fmt --check, clippy -D warnings (default AND --all-features), 95 rewind unit
tests + full workspace all pass. E2E smoke-tested the binary (show/-v, diff
capture/clean/vs-live, JSON, exit 0/1/3, latest marker). Planned via a
multi-angle design workflow + adversarial critique before coding.

---

## rex-check: itemize umbrella crates into the totals table (incl. rewind)

2026-06-19. `crates/rex-check/src/main.rs`. The crates inside the umbrella
(`linux-ops-suite/crates/*`) are auto-discovered at runtime (`discover_crates`,
any subdir with a `Cargo.toml`, sorted) and itemized as indented `·` sub-rows
under the `linux-ops-suite` row in the SINGLE totals table. The umbrella's own
row is shown net of the crates (`linux-ops-suite (root)`), so the grand TOTAL is
identical to a plain whole-repo tokei (no double-count) but every crate —
rewind, pulse, portman, tripwire, … — is now a visible line item. Footer total
reads `TOTAL (7 repos + N crates)`. `count_rs_files()` is the no-tokei fallback.
No hardcoded crate list; new crates appear automatically. Widened the name
column to 24 for alignment. 12 tests pass (added discover_crates ×2 +
count_rs_files), clippy clean, fmt applied.

---

## New tool: Rewind — suite history + safe rollback (crates/rewind), Phase 1

2026-06-19. Designed and started Rewind, the suite's TIME AXIS / black-box
recorder: it records the suite's own state files (the compiled Workstate
snapshot, the producer feeds, tripwire's baseline) into a content-addressed
store, lists the timeline, and — in later phases — diffs any two points and
RESTORES under a hard safety gate. It is the one suite tool that breaks
"read-only always," narrowly: restore is dry-run-by-default, only touches
Rewind's own captures, and auto-takes a pre-restore safety capture first.

Design first: wrote REWIND_DESIGN.md at the repo root (mirrors
TRIPWIRE/PULSE_DESIGN.md). Tom confirmed two key forks — content-addressed
object store + manifests (git-blob idea, no git, auto-dedup), and
auto-capture-current-state-first as the restore safety mechanism. Doc committed
on this worktree branch (787fa5c).

Phase 1 shipped (this commit): the storage layer + `rewind capture`, the
timeline view (default / `rewind log`), and `rewind sources`. Deferred to later
phases: `show`, `diff`, and the guarded `restore` (the one writing path — gets
the heaviest test coverage when it lands).

Built to tripwire's exact house style — thin `main`, library does the work,
renderers derive from the model, `Style` resolver, JSON envelope shape
(`schema_version`+`source_tool`), exit codes 0 ok / 3 can't-run (1 diff-drift
and 2 partial-restore reserved for later phases). LIFTED tripwire's proven
primitives verbatim (hash.rs streaming SHA-256, scan/walk.rs, scan/meta.rs) per
the copy-for-v1 decision in the design doc; extract a shared suite-fs crate only
if a third consumer appears.

Storage: `$XDG_DATA_HOME/linux-ops-suite/rewind/` with `objects/<aa>/<sha256>`
(deduped blobs), `captures/<ts>-<id>.json` (one manifest per capture), `HEAD`.
Each readable file's bytes are hashed and stored once; two captures of identical
content share one object (verified e2e: 2 captures of the same content = 2
unique objects total, not 4; store stayed at 68 B). Atomic temp+rename writes;
manifest written last. Capture id is content-derived (SHA-256 over timestamp +
label + each entry's path/hash/mode/owner; mtime deliberately excluded so a
touched-but-identical file doesn't change identity). Envelope sniffing on
capture records each blob's `source_tool`/`schema_version`, driving the
timeline's `good`/`snapshot invalid` NOTE and (later) `--latest-good`.

Default capture set = the three suite-state targets (Tom's call): compiled
snapshot + producer-feeds dir + tripwire baseline, existing-only. Precedence
cli > config > builtin (line-based capture.conf, no TOML dep), source always
recorded and shown by `rewind sources`. Graceful degradation: an unreadable file
is metadata-only (no blob, not restorable), a missing configured path is simply
absent, a symlink is recorded as a symlink (never followed by default), a
newer-schema manifest is rejected loudly.

Deps: clap + serde + serde_json + chrono (chrono is the only addition over
tripwire's set, for the capture timestamp; it's already a workspace dep and
Tom approved). No sha2/git2/walkdir/compression/network/async.

Gate GREEN: cargo fmt --check (workspace), clippy -p rewind --all-targets
-D warnings (default AND --all-features, both clean), 58 rewind tests + full
workspace test (all crates, 0 failures). Manual e2e verified: sources/capture/
timeline incl --json, dedup, invalid-snapshot NOTE, no-store exit 3, empty-set
exit 3, builtin set resolution. Registered crates/rewind in the workspace
Cargo.toml, added it to the suite README tool table + a crate README in
tripwire's shape.

NOT done (left for Tom's call): not committed/pushed beyond the design doc —
awaiting approval per the hard rule. No PR opened yet. Phases 2+ (show / diff /
guarded restore / prune) not started. No JSON schema under contracts/ +
examples/ yet (tripwire/portman/pulse also ship without one; can add a
rewind.capture/timeline schema pair if wanted). Work is on worktree branch
`worktree-rewind-design`.

## New tool: Tripwire — file-integrity baseline + drift diff (crates/tripwire)

2026-06-18. Designed and implemented Tripwire, the suite's filesystem-surface
lens (the on-disk counterpart to portman's network lens). Read-only: records a
baseline (SHA-256 content hash + metadata — kind/mode/uid/gid/size/mtime) of a
watched set of files/dirs, then diffs the live filesystem against it, reporting
added / removed / modified (content, mode, owner, size, type, readability).
Built to portman's exact house style — thin `main`, library does the work,
renderers derive from the model, `Style` resolver, JSON envelope shape
(`schema_version`+`source_tool`), exit codes 0 clean / 1 drift / 3 can't-run.

Design first: wrote TRIPWIRE_DESIGN.md (mirrors PULSE_DESIGN.md), Tom approved
4 forks — ship a built-in default watch set (~14 system files + dotfiles),
baseline records state only (no config writing), keep the cron-quiet `verify`
subcommand, line-based `watch.conf` (no TOML dep).

Commands: `tripwire` (view), `watch` (resolved set + source), `baseline`,
`diff` (exit 1 on drift), `verify` (diff but silent on clean). Watch-set
precedence: `--path` flags > `--config`/default watch.conf > built-in set;
source is always recorded. Per-path opts: recursive, follow_symlinks (default
OFF — symlinks recorded as symlinks, not followed), content (hash on/off),
exclude globs.

Lean like the rest: deps are clap + serde + serde_json only (dev: tempfile).
SHA-256 is HAND-ROLLED (hash.rs, streamed in 64KiB chunks, validated against
FIPS 180-4 known-answer vectors incl. the million-'a' case); the recursive
directory walk is hand-rolled too (walk.rs, iterative + depth guard, prunes
excludes, honors symlink policy) — no sha2/walkdir/notify/network/async.

Graceful degradation throughout: an unreadable file (e.g. /etc/shadow as
non-root) is recorded as metadata with `unreadable:true` and no hash, never an
error; mtime is NEVER part of identity or the change decision (touched-but-
identical ≠ drift); readable↔unreadable flip is its own change; type change
short-circuits; mode/owner changes get a `[PERM]`/`[OWNER]` security tag (the
analogue of portman's `[PUBLIC]`). Versioned baseline rejects a newer schema
loudly; NoBaseline vs BadBaseline split like portman.

Caught + fixed a real footgun in manual e2e testing: a baseline file living
INSIDE a watched dir would self-report as drift. Fixed by threading an `ignore`
path (the resolved baseline file, canonicalized) through scan() — verified the
baseline-inside-dir case now diffs clean and a real tamper still trips exit 1.

Gate GREEN: cargo fmt --check, clippy --all-targets -D warnings (default AND
--all-features), 57 tripwire tests + full workspace test (all crates, 0
failures), workspace build all clean. Registered crates/tripwire in the
workspace Cargo.toml and added it to the suite README tool table + a crate
README in portman's shape.

NOT done (left for Tom's call): not committed/pushed — awaiting approval per the
hard rule. No GitHub Release/installer entry yet (tripwire is an in-repo crate
like rex-doctor/portman/pulse, not a sibling-repo release asset). No JSON schema
under contracts/ + examples/ yet (portman/pulse also ship without one; can add a
tripwire.scan/diff schema pair if wanted). Work is on worktree branch
`worktree-tripwire`.

## Deep umbrella review → fix all HIGH/CRITICAL items (3 PRs MERGED)

2026-06-18. Ran a deep code review of the whole umbrella (5 crates + the
contracts/CI/docs surface) with parallel specialist agents, established a green
baseline (fmt, clippy -D warnings default+all-features, full test, all 9
schema/example pairs validated), then fixed every HIGH/CRITICAL finding in
priority order. Each fix got a new test; the full gate + `rex-check` ran before
every commit. Landed as normal merge commits across 3 PRs:

**umbrella PR #25 (MERGED) — installer + rex-check security.**
`linux-ops-install`: a release that PASSES sha256 could still escape, so —
tar gained `--no-absolute-filenames` (both -xJf/-xzf), unzip gained `-j`
(zip-slip), `find_binary` now stats with `symlink_metadata` and SKIPS symlinks
(no redirect/escape), `collect_assets` rejects untrusted asset names that aren't
a plain single-segment filename (no `/ \ ..`, leading-dot, NUL) and requires an
https URL, curl got `--proto =https` on both calls, and the prereq probe checks
the `--version` exit status. `rex-check`: `commit_one` now refuses (unless
confirmed) on main/master/detached-HEAD/rebase|merge (`commit_hazard`) and warns
that `git add -A` stages untracked files; `command_exists` dropped the
`sh -c "command -v"` injection vector for a direct PATH walk.

**umbrella PR #26 (MERGED) — thomas-tui/suite-ui/toolbox-bridge.**
Five own-crate matches on `#[non_exhaustive]` status enums had no fallback arm
(adding a variant would break the defining crate) — added neutral fallbacks
(theme.rs health/severity → plain Style; status_bar Outcome → ("? ", dim),
JobState → "…"; toast ToastKind → dim text), all `#[allow(unreachable_patterns)]`.
The toast inner outcome match was made explicit + `unreachable!()` (the old
`_ => Cancelled` was a silent-misrender trap). `centered_rect` now clamps pct to
<=100 (was a reachable u16 underflow: panic in debug / wrap in release).
toolbox-bridge `source_generated_at` no longer emits "" — a blank upstream stamp
normalizes to an `"unknown"` sentinel (+ operator warning), the field was added
to the output schema's `required`, and the stale "v3 snapshot" test doc was fixed.

**workstate PR #7 (MERGED) — snapshot/schema, in the sibling repo.**
`Finding` gained `#[serde(deny_unknown_fields)]` so the v4 allowlist
(additionalProperties:false — keeps Bulwark secrets/PII out) is enforced on the
DESERIALIZE path, not just serialize. `FeedStatus` gained an `Unknown(Value)`
catch-all + custom Deserialize so a status written by a NEWER Workstate degrades
to "unknown health" instead of hard-failing the entire snapshot for a pinned
consumer. Also fixed a PRE-EXISTING stale CI pin discovered when its PR went red:
`HUB_SCHEMA_REF` pointed at v3 commit 3f0d2da while the crate emits v4 → bumped
to v4 commit f89b1be (the job now passes).

Final `rex-check`: all 7 repos on main, clean; all three touched mains
(linux-ops-suite, workstate) green in CI.

NOT done (out of scope / blocked): the 6 sibling-repo deep reviews
(rexops/bulwark/scriptvault/proto/toolfoundry) were dispatched but ABORTED by an
Anthropic session limit — only workstate completed. **proto still has NO ci.yml**
(only release.yml) — a confirmed open HIGH to revisit. Lower-severity umbrella
items (MED/LOW: god-file splits, doc drift in README/AGENT/ARCHITECTURE, MSRV
1.85-vs-1.96, the unpopulated workstate findings example) were left for a later pass.

## Cleanup: retire all bump-v0.1.1 branches across the 7 suite repos

2026-06-16. Removed the leftover `bump-v0.1.1` branch from every repo. The task was
framed as "merge bump into main, push, delete", but inspection showed the bump's
*content was already on `origin/main` in all 7 repos* (verified by matching
`git patch-id`): proto/rexops/scriptvault landed it via squash-PRs #5/#26/#18 plus
their release.yml, bulwark/workstate had it fully contained in main, and
linux-ops-suite/toolfoundry had it superseded by `origin/main` PRs #24/#4 with newer
work on top. So no merge/push was needed (a merge would have been a no-op at best, or
regressed main — e.g. re-adding the `.gitignore` line 029eff0 deleted, or reverting
toolfoundry's hardening cleanup 6278868). Actions taken: FF'd local main to origin/main
where behind (proto/rexops/scriptvault), removed the stale
`.claude/worktrees/suite-fix-top5` worktree (clean) that pinned linux-ops-suite's bump
branch, then deleted all 7 bump branches with `git branch -D`. Final `rex-check`:
all 7 repos on `main`, clean, 0 dirty, in sync with origin. (Other stale worktrees
remain under `.claude/worktrees/` — left untouched, out of scope.)

## linux-ops-install code-review fixes (CORRECTNESS / CLARITY / NITS, all items)

2026-06-16. Branch `worktree-installer-review-fixes` (worktree under
`.claude/worktrees/installer-review-fixes`, cut from origin/main). Applied
**every** item from the in-session critical review of
`crates/linux-ops-install` — all 5 CORRECTNESS, all 6 CLARITY/STYLE, all 3
NITS. Not pushed; awaiting Tom's call on PR.

CORRECTNESS:
1. TempDir now created with O_EXCL semantics + mode 0700 (`DirBuilder::mode().create()`),
   fails if path exists — no symlink/pre-create reuse in shared /tmp.
2. `read_expected_sha256`: a bare (filename-less) digest is trusted ONLY when it
   is the sole digest line; multi-entry manifests with a bare line now error
   instead of guessing.
3. `checksum_for` manifest match tightened to exact `sha256sums`/`sha256sums.txt`
   (dropped loose `ends_with`), so a different asset's `*.sha256sums` can't be
   mispaired.
4. `download_asset` gained `--max-redirs 10` + `--max-filesize 512MiB` caps.
5. `fetch_http` reads HTTP status from curl `-w '%{http_code}'` to a temp body
   file instead of a `__HTTP_STATUS__` body sentinel (no body-collision risk).

CLARITY/STYLE:
6. Split the 1493-line `main.rs` god-file into focused modules: `error.rs`,
   `platform.rs`, `release.rs`, `verify.rs`, `fs_ops.rs`, `net.rs`, `ui.rs`;
   `main.rs` is now a 101-line orchestrator. Tests moved into their modules.
7. `ReleaseAsset` derives `Clone`; `select_asset` uses `.cloned()`.
8. `print_mode` now echoes the verify posture (no_verify / allow_unverified /
   default fail-closed) in the banner.
9. `NoLatestRelease` Display names its unused fields (`binary:_`,`new_release_url:_`)
   instead of hiding them behind `..`.
10. Single `is_signature_or_checksum()` helper + `SIGNATURE_OR_CHECKSUM_EXTENSIONS`
    const, shared (no denylist drift).
11. `find_binary` is now BFS shallowest-match with sorted (deterministic) ties.

NITS:
- `tar`/`unzip` get friendly `MissingPrerequisite` remediation at point-of-use
  (new error variant; `check_command` for curl/sha256sum routed through it too).
- `summarize_http_body` truncates on a UTF-8 char boundary (was a real
  mid-char slice panic).
- TempDir `unwrap_or(0)` nanos documented as intentional.

Added 7 tests (multi-entry bare-digest rejection, sole-bare-digest accept,
loose-manifest ignore, UTF-8 truncation, shallowest find_binary). Verified:
`cargo fmt --check`, `clippy --workspace --all-targets -D warnings`, and
`cargo test --workspace` all green; installer crate 25/25 tests pass.

## Top-5 review fixes: green main, release/installer hardening, LICENSE+MSRV

2026-06-16. Branch `worktree-suite-fix-top5` (worktree under
`.claude/worktrees/suite-fix-top5`, cut from origin/main @de32e00). Knocked out
the 5 prioritized items from the 2026-06-16 review, in order. The uncommitted
installer rewrite + `release.yml` that existed only in the primary checkout's
working tree were carried into this branch via `git stash -u` (so they finally
get committed, not lost).

**#1 — `main` was RED, now green.** `rex-check` had a `clippy::trim_split_whitespace`
error (`crates/rex-check/src/main.rs:387`, redundant `.trim_start()` before
`split_whitespace()` — removed) and `cargo fmt --check` failed at 9 sites in the
installer (ran `cargo fmt`). Now `fmt --check`, `clippy -D warnings`, and
`cargo test --workspace` all exit 0 (175 tests pass).

**#2 — Release+installer pipeline landed.** Committed the umbrella's
`linux-ops-install` rewrite (SHA256-verifying release installer) + `.github/workflows/release.yml`.
Cross-repo: `workstate` and `proto` release.yml were missing the explicit
`-p <crate>` build flag (and proto had a double-space typo) — both fixed to
`-p workstate` / `-p proto` (single root-bin repos, so the built binary is
identical; the flag just future-proofs against a second bin). All 6 repos'
release.yml verified consistent: tag `v*` → x86_64+aarch64 `.tar.gz` + `.sha256`,
archive/binary names match the installer registry.

**#3 — toolfoundry.** Its release.yml already existed and was already correct
(`-p toolfoundry`); the earlier "missing" finding was stale. Just committed.

**#4 — Checksum policy now fails closed.** `verify_download` previously installed
unverified on a *missing* checksum (warn + proceed). Since every suite release
publishes a `.sha256`, a missing one means a broken/tampered release — flipped the
default to hard-fail. Renamed `--require-checksums` → inverted `--allow-unverified`
(opt-in downgrade to warn); `--no-verify` unchanged. A checksum *mismatch* already
failed and still does. Added 3 tests: `missing_checksum_fails_closed_by_default`,
`missing_checksum_allowed_with_flag`, `no_verify_skips_missing_checksum`. README +
error text + doc comments updated.

**#5 — LICENSE + MSRV + docs.** Added top-level `LICENSE` (MIT, backs the
`license = "MIT"` in every manifest), `rust-toolchain.toml` (channel `1.96.0`, the
fix for #1's surprise: CI no longer rides floating `stable`), and
`rust-version = "1.85"` (MSRV) in `[workspace.package]`. Refreshed PROJECT-STATUS.md
(new installer path, release pipeline, toolchain pin, + `linux-ops-install` &
`rex-check` in the crate table).

Umbrella diff: rex-check, linux-ops-install (rewrite + flag rename + 3 tests),
Cargo.toml, README.md, PROJECT-STATUS.md, + new LICENSE / rust-toolchain.toml /
.github/workflows/release.yml. Sibling repos (workstate, proto, + all 6 release.yml)
committed in place on their own `main` — NOT pushed yet (awaiting go-ahead).
NEXT: PR the umbrella branch; push sibling release.yml commits; tag ONE repo `v0.1.0`
to exercise the pipeline + installer end-to-end (never run yet — no releases exist).

---

## Fix 3 Important review items: CI example-validation + 2 doc fixes

2026-06-14. Repo: linux-ops-suite (umbrella) only. Branch: `fix-ci-and-docs`
(worktree under `.claude/worktrees/fix-ci-and-docs`, based on origin/main @1668fcb
— NOTE: the local primary checkout was 2 commits behind origin/main at the time;
this branch was cut from the remote, not the stale local main). Implemented the
3 "Important" findings from the full 2026-06-14 umbrella review:

1. **CI now validates examples against schemas** (`.github/workflows/ci.yml`,
   `json:` job, new 3rd step "Validate examples against their schemas"). Uses the
   same `check-jsonschema` already installed; an explicit example→schema pair list
   (5 pairs) because `proto.workstate-feed.example.json` omits the `.v1` infix its
   schema carries. Verified locally with check-jsonschema 0.37.2: all 5 pairs pass,
   AND confirmed teeth — a deliberately corrupted proto.session copy is correctly
   REJECTED (schema_version is an integer const = 1). This closes the #1 gap: the
   contract↔example relationship was previously unenforced anywhere.
2. **docs/AGENT.md:24** crate-ownership table row updated `suite-ui, toolbox-bridge`
   → `thomas-tui, suite-ui, toolbox-bridge` with role text reflecting the toolkit/
   chrome split. (AGENT.md was the last shipping doc still omitting thomas-tui;
   README + ARCHITECTURE + PROJECT-STATUS already had it via PR #15/#16.)
3. **README.md:91** fixed the "RexOps is the only consumer" contradiction →
   "front door and top-level consumer … (ScriptVault is a secondary consumer: it
   reads the Toolbox-Bridge sidecar feed)". Matches ARCHITECTURE's "only suite-
   level consumer" framing and the sidecar flow described 3 lines below.

Diff: 3 files, +20/−2 (ci.yml +18, README -1/+1, AGENT.md -1/+1). No code touched;
crates unchanged. NEXT: merge to umbrella main via PR. (Left unaddressed by design:
the Minor/Nice-to-have items — non_exhaustive on the 2 error enums, the proto
example rename, workstate.snapshot still lacks an example, ROADMAP lag, LAST_WORK
location.)

ALSO: the user's local ~/projects/linux-ops-suite main checkout is behind
origin/main — they were told to `git pull --ff-only`.

---

## PROJECT-STATUS.md accuracy cleanup (umbrella, docs-only)

2026-06-14. Repo: linux-ops-suite (umbrella) only. Branch:
`worktree-project-status-cleanup`. Docs-only edit to PROJECT-STATUS.md — no code,
no consumer/sibling repos touched. Fixed five drift items the 2026-06-14 review
flagged, plus one diagram error found in passing:

1. Broken footer link to non-existent `INSTALLER-STATUS.md` → now points to
   `install.sh` + `docs/ARCHITECTURE.md`/`INTEGRATION_MAP.md` (all verified to
   exist).
2. Stale suite-ui pin `cf97f07` → actual rev `71a4fe5` (the rev all three
   consumers really pin: bulwark/rexops/scriptvault Cargo.toml,
   71a4fe5484abb75b494c010b89033dbc7e0faace).
3. Removed all "pending push / unpushed commit / Commits pending push" language
   and dropped "Major remaining work #1 (push the conversion)" — the git-dep
   conversion is landed AND pushed.
4. Added `thomas-tui` everywhere it was missing: the shared-code-exception bullet
   (now both TUI crates), a new in-workspace-crates table (thomas-tui ~3.2k /
   suite-ui ~1.6k / toolbox-bridge ~1.1k), and the "Done since last snapshot"
   list. Moved Toolbox-Bridge out of the sibling-tools table into that crates
   table → tools table is now the 6 real sibling tools ("All six tools").
5. Schema count: kept "9 schemas" (correct — 9 files in contracts/) but made it
   precise — listed all 9 by name and noted examples/ has only 5 sample payloads,
   not schema-validated in CI (only well-formedness is). Added remaining-work item
   to validate examples vs schemas.

Also fixed the data-flow diagram: ScriptVault was wrongly shown as a Workstate
feed producer; real feed producers into Workstate are ToolFoundry/Bulwark/Proto
(per docs/INTEGRATION_MAP.md). ScriptVault export + Bulwark scan are read by
RexOps directly; noted that.

Diff: PROJECT-STATUS.md only, +49/−32. NEXT: merge to umbrella main via PR.

---

## thomas-tui + suite-ui: #[non_exhaustive] enums + test hardening (PR pending merge)

2026-06-14 ~00:30 UTC. Repo: linux-ops-suite (umbrella) only — no consumer/
sibling repos touched. Branch: `worktree-non-exhaustive-enums` (worktree under
`.claude/worktrees/non-exhaustive-enums`). Followed a 5-agent deep-dive review
of thomas-tui + suite-ui; implemented the highest-value findings.

Forward-compat (API): added `#[non_exhaustive]` to all 7 public enums —
  thomas-tui: ThemeChoice, ColorChoice, Health, Severity (theme.rs)
  suite-ui:   Outcome, JobState (status_bar.rs), ToastKind (overlays/toast.rs)
The attribute FORCED two cross-crate matches in suite-ui to gain a fallback arm
(`_ => "?"`): badge.rs (Severity→abbr) and health_strip.rs (Health→glyph) —
without them suite-ui itself stops compiling. A future enum variant now shows a
neutral `?` rather than breaking consumers or masquerading as an existing level.

Test hardening (review items #1/#2/#4; #3/#5/#6 deferred per Tom):
  - widgets.rs: pane border now asserted to carry the DIM modifier (not just the
    corner glyph); padding asserted (border x=0, pad x=1, body x=2); pane_blank
    border checked dim; +tiny-area (1×1…3×3) no-panic guard.
  - palette.rs: MAX_ROWS truncation test (15 items → 0–11 render, 12+ dropped,
    no "(no match)"); out-of-range `selected: Some(99)` highlights nothing / no
    panic.
  - layout.rs: zero-size + 1×1 parents into centered_rect/centered_fixed stay
    in-bounds, no panic/underflow.
  - (freshness bucket-boundary tests the review flagged were ALREADY present —
    not duplicated.)

Counts now: thomas-tui 85 unit (+7) + 13 doctest; suite-ui 27 unit + 5 doctest.
Verified GREEN on default AND `--features clap`: `cargo build`, `cargo test`,
`cargo clippy --all-targets -- -D warnings`. Diff: 8 files, +181/−12.

API impact on consumers (rexops/bulwark/scriptvault): they pin a suite-ui rev
and only construct/read these enums (no exhaustive matches they own), so this
forces NO rev-bump — they move only if deliberately bumped per
suite-ui-ci-sibling-checkout-ordering.

NEXT: merging to umbrella main via PR now (per the "PR, never direct push" rule).

---

## thomas-tui + suite-ui review fixes (worktree, NOT yet merged)

Focused review of the new `thomas-tui` crate + updated `suite-ui`, then fixed
all findings. Branch: `worktree-tui-review-fixes` (worktree under
`.claude/worktrees/tui-review-fixes`). Not pushed / no PR yet.

Two CONFIRMED rendering bugs fixed (reproduced before/after in the gallery):
  - ConfirmModal clipped a title longer than its message — width now folds in
    `title.chars().count() + 2` (confirm.rs). Verified: full long title fits.
  - PaletteFrame chopped long descriptions/labels at the border with no marker —
    now truncates both via the crate's own `truncate_desc` to the computed inner
    width (palette.rs). Verified: descriptions end in `…`.
Plus:
  - PaletteFrame `selected: None` used to highlight row 0 (`unwrap_or(0)`); now
    `self.selected == Some(i)` so None highlights nothing. Doc clarified.
  - centered_rect lost a row/col on ODD percentages (`(100-pct)/2` twice = 99%);
    trailing margin now absorbs the remainder → band is exactly `pct` (layout.rs).
  - keys::key_hint() kept as a literal but added a drift-guard test asserting it
    names QUIT/HELP/^P, so it can't silently disagree with the binding consts.
  - Removed the redundant `suite-ui/src/app/mod.rs` pass-through (only lib.rs used
    it); `Tui`/`TuiError`/`TuiOptions` now re-exported straight from thomas_tui.
  - Kept the `theme` shim module (it's the single internal import seam for 5
    suite-ui widgets) — tightened the doc to say why it stays.

Tests ADDED to the 3 previously-untested overlay files (confirm/help/palette).
Counts now: thomas-tui 78 unit (+13) + 13 doctest; suite-ui 27 unit + 5 doctest.
Verified: `cargo clippy --all-targets --all-features -- -D warnings` clean;
`cargo test --all-features` all green; `cargo build --workspace` clean (no
consumer/sibling breakage). Public API unchanged (consumers need no rev-bump).

NEXT: review the diff, then merge to umbrella main as a normal PR. No consumer
rev-bump dance needed unless you want them on the fixed suite-ui rev.

---

## thomas-tui extraction: MERGED to umbrella main (PR #11)

The full `thomas-tui` extraction is MERGED to umbrella main.
  PR #11: https://github.com/tom2025b/linux-ops-suite/pull/11 (merged, not squashed)
  merge commit: 71a4fe5 ; CI green before merge.
  Feature branch + worktree deleted; local main fast-forwarded.

NEW crate layout (workspace members: suite-ui, thomas-tui, toolbox-bridge):
  - thomas-tui = the general TUI toolkit (guard, Theme(+Severity/Health), text,
    layout/centering, panes, keys, SearchBar, KeyHints, EmptyState, Counted,
    FilterChips, StatusStrip, Freshness, Confirm/Help/Palette overlays). Deps:
    ratatui + crossterm (+ optional clap for the Theme/Color ValueEnum derives).
  - suite-ui = domain core only (attention_flag, badge, health_strip, status_bar,
    overlays/toast). Depends on thomas-tui via PATH dep; re-exports everything
    moved; its clap feature forwards to thomas-tui/clap. Public API unchanged.

FOLLOW-UP (per [[suite-ui-ci-sibling-checkout-ordering]]) — COMPLETE & MERGED:
bumped the pinned suite-ui git rev to 71a4fe5 in all 3 consumers, each its own
PR (off its repo's main), CI green, MERGED, branch deleted:
  - RexOps:      PR #20 merged → origin/main 7d4f72c   (118 tests)
  - Bulwark:     PR #7  merged → origin/main 4fd2d74   (tui-feature path)
  - ScriptVault: PR #9  merged → origin/main aef7bb6   (270 tests; clap path)
All 3 consumers' main now pin suite-ui @ 71a4fe5 and BUILD CLEAN post-merge.
Each Cargo.lock resolves thomas-tui transitively from the same umbrella rev (path
dep via suite-ui — no separate consumer dep). Validated the
suite-ui/clap -> thomas-tui/clap forwarding via ScriptVault (only clap consumer).

=> The thomas-tui extraction is fully rolled out across the whole suite.

REMAINING (deferred, design work — not straight moves): the 5 Tier-C suite-ui
widgets (badge/attention_flag/health_strip/status_bar/toast) could be generalized
into reusable primitives (generic Badge<T>, a generic status segment) that
suite-ui then specializes.

---

## thomas-tui: eighth extraction — the whole easy Tier-B set (8 files)

Drained the rest of the straight-move tier in one batch, same verbatim-rename +
re-export pattern. All 8 tracked as git renames (R091–R099 — doc-only deltas):

- `keys.rs`        → thomas-tui `pub mod keys` (PURE, crossterm-only). suite-ui
                     re-exports the whole module: `pub use thomas_tui::keys;` so
                     `suite_ui::keys::QUIT` etc. are unchanged. Generalized the
                     module doc (dropped "both suite TUIs"/tool names).
- `filter_chips.rs`→ theme-only; dropped a broken `crate::StatusBar` doc link.
- `status_strip.rs`→ theme-only (StatusStrip + STATUS_SEP const).
- `freshness.rs`   → theme-only pure formatter (uses truncate_path, already moved).
- `overlays/{confirm,help,palette}.rs` → new `thomas-tui/src/overlays/` module.
                     These used `crate::widgets::centered_*`; repointed to
                     `crate::centered_*` (already in thomas-tui). suite-ui's
                     `overlays/mod.rs` keeps only `toast` (domain-coupled) and
                     re-exports the 3 generic ones from thomas_tui.
- `widgets.rs`     → PARTIAL: only pane/pane_titled/pane_blank moved (the
                     centering helpers went in extraction #2). suite-ui's
                     widgets.rs is gone; lib.rs re-exports pane*/centered_* from
                     thomas_tui.

**Verified:** test count lossless — suite-ui unit 27 + thomas-tui unit 65 = 92
conserved; doctests suite-ui 5 + thomas-tui 13. clippy -D warnings clean both
crates (default AND --features clap); fmt clean; gallery builds.

**thomas-tui now owns (the whole general toolkit):** Theme(+Severity/Health),
Tui guard, text, layout (centering), widgets (panes), keys, SearchBar, KeyHints,
EmptyState, Counted, FilterChips, StatusStrip, Freshness, overlays
(Confirm/Help/Palette).

**suite-ui is now down to its DOMAIN core (Tier-C only):** attention_flag,
badge (SeverityBadge), health_strip, status_bar (JobState/Outcome), overlays/toast
(ToastKind) — each welded to Severity/Health/JobState/Outcome. These are NOT
straight moves; generalizing them is design work (a generic Badge<T>, a generic
status segment), deferred pending a decision.

---

## thomas-tui: seventh extraction — Counted span helper

Moved `counted.rs` into thomas-tui (git `R090` — near-pure rename). Doc-only
delta: generalized the module-doc opener (dropped "in the suite" / "the rest of
the crate") and pointed the doctest at `use thomas_tui::`. No logic changed.

Counted only coupled to `Theme` (via `accent_bar()`/`dim()`), so
`use crate::theme::Theme;` resolves verbatim in thomas-tui.

Note: suite-ui's `widgets.rs` `pane_titled` doctest does
`# use suite_ui::{pane_titled, Counted, Theme};` — still valid because Counted is
re-exported at `suite_ui::Counted` (that suite-ui doctest count held at 12).

**Wiring (suite-ui API identical):**
- thomas-tui: `mod counted` + `pub use counted::Counted`.
- suite-ui: dropped `mod counted`, now `pub use thomas_tui::Counted` — so
  `suite_ui::Counted` and gallery's use are unchanged.

**Verified:** test count lossless — suite-ui unit 55→50, thomas-tui unit 37→42
(the 5 Counted tests moved); doctests suite-ui 13→12, thomas-tui 5→6. 92
conserved. clippy -D warnings clean both crates; fmt clean; gallery builds.

**thomas-tui now owns:** Theme (+Severity/Health), Tui guard, text truncation,
centering helpers, SearchBar, KeyHints, EmptyState, Counted.

**Remaining theme-only Tier-B widget:** FilterChips (last on the easy path).

---

## thomas-tui: sixth extraction — EmptyState widget

Moved `empty_state.rs` into thomas-tui (git `R099` — cleanest rename yet, the
only delta is the doctest `use suite_ui::` → `use thomas_tui::`). No logic
changed; the `[Theme]` doc link stays valid (Theme is in thomas-tui).

EmptyState only coupled to `Theme` (via `dim()`), so `use crate::theme::Theme;`
resolves verbatim in thomas-tui.

**Wiring (suite-ui API identical):**
- thomas-tui: `mod empty_state` + `pub use empty_state::EmptyState`.
- suite-ui: dropped `mod empty_state`, now `pub use thomas_tui::EmptyState` — so
  `suite_ui::EmptyState` and gallery's use are unchanged.

**Verified:** test count lossless — suite-ui unit 59→55, thomas-tui unit 33→37
(the 4 EmptyState tests moved); doctests suite-ui 14→13, thomas-tui 4→5. 92
conserved. clippy -D warnings clean both crates; fmt clean; gallery builds.

**thomas-tui now owns:** Theme (+Severity/Health), Tui guard, text truncation,
centering helpers, SearchBar, KeyHints, EmptyState.

---

## thomas-tui: fifth extraction — KeyHints widget

Moved `key_hints.rs` into thomas-tui (git `R085` — near-pure rename). Doc-only
delta: rewrote the module-doc opener to drop a now-broken `crate::HelpSheet`
intra-doc link (HelpSheet stays in suite-ui) and pointed the doctest at
`use thomas_tui::`. The `[Theme::prompt](crate::Theme)` link stays valid (Theme
lives in thomas-tui). No logic changed.

Like SearchBar, KeyHints only coupled to `Theme` (via `prompt()`/`dim()`), so
`use crate::theme::Theme;` resolves verbatim in thomas-tui.

**Wiring (suite-ui API identical):**
- thomas-tui: `mod key_hints` + `pub use key_hints::KeyHints`.
- suite-ui: dropped `mod key_hints`, now `pub use thomas_tui::KeyHints` — so
  `suite_ui::KeyHints` and gallery's use are unchanged.

**Verified:** test count lossless — suite-ui unit 64→59, thomas-tui unit 28→33
(the 5 KeyHints tests moved); doctests suite-ui 15→14, thomas-tui 3→4. 92
conserved. clippy -D warnings clean both crates; fmt clean; gallery builds.

**thomas-tui now owns:** Theme (+Severity/Health), Tui guard, text truncation,
centering helpers, SearchBar, KeyHints.

---

## thomas-tui: fourth extraction — SearchBar widget

Moved `search_bar.rs` into thomas-tui (git `R093` — near-pure rename). The 7%
delta is 3 doc-only edits: generalized the module-doc opener, removed two now-
broken intra-doc links (`crate::StatusBar`/`crate::Toast` — those stay in
suite-ui), and pointed the doctest at `use thomas_tui::`. No logic changed.

SearchBar only coupled to `Theme` (via `prompt()`/`dim()`/`match_text()`), which
now lives in thomas-tui — so `use crate::theme::Theme;` resolves verbatim inside
thomas-tui (it has its own `mod theme`). First Tier-B widget unblocked by the
Theme move.

**Wiring (suite-ui API identical):**
- thomas-tui: `mod search_bar` + `pub use search_bar::SearchBar`.
- suite-ui: dropped `mod search_bar`, now `pub use thomas_tui::SearchBar` at the
  crate root — so `suite_ui::SearchBar` and gallery's use are unchanged.

**Verified:** test count lossless — suite-ui unit 68→64, thomas-tui unit 24→28
(the 4 SearchBar tests moved); doctests suite-ui 16→15, thomas-tui 2→3. 92
conserved. clippy -D warnings clean both crates; fmt clean; gallery builds.

**thomas-tui now owns:** Theme (+Severity/Health), Tui guard, text truncation,
centering helpers, SearchBar.

---

## thomas-tui: third extraction — the whole Theme (simplest move)

Moved `theme.rs` **verbatim** into thomas-tui — git tracked it as `R100` (a
100% pure rename, ZERO content change). The full file went: the `Theme` struct,
the `NO_COLOR` gate, `ThemeChoice`/`ColorChoice`, the `Severity`/`Health` enums,
and ALL the inherent style methods (incl. `severity()`/`health()`).

NOTE: an earlier attempt over-engineered this (split pure styling from a
`Severity`/`Health` extension trait). Tom course-corrected: "keep it simple, do
the simplest possible extraction that keeps the API unchanged." Reset and did the
plain whole-file move instead. `Severity`/`Health` rode along into thomas-tui —
they're generic enough, and `theme.severity(s)` stays an inherent method (no
trait, no call-site churn).

**Wiring (suite-ui API identical):**
- thomas-tui exposes `mod theme` → re-exports Theme/ThemeChoice/ColorChoice/
  Severity/Health. Added an optional `clap` dep + `clap` feature so the existing
  `#[cfg_attr(feature="clap", derive(ValueEnum))]` lines compile.
- suite-ui keeps a one-line shim `mod theme { pub use thomas_tui::{...}; }` so all
  17 files importing `crate::theme::{...}` and `lib.rs`'s `pub use theme::{...}`
  resolve UNCHANGED — no edits to any consuming file.
- suite-ui's `clap` feature now forwards to `thomas-tui/clap` (its own clap dep
  dropped — the only clap use was those theme derives, which moved).

**Verified:** test count lossless — thomas-tui unit 15→24, suite-ui unit 77→68
(the 9 theme tests moved); 92 conserved. Builds clean with AND without
`--features clap` (forwarding works); clippy -D warnings clean in both feature
states; fmt clean; gallery builds.

**thomas-tui now owns:** Theme (+Severity/Health), Tui guard, text truncation,
centering helpers. suite-ui is now mostly domain widgets layered on top.

---

## thomas-tui: second extraction — text truncation + centering helpers

Pulled two more zero-coupling pieces from suite-ui into `thomas-tui`, same
move-and-re-export pattern as the Tui guard:

- **`text.rs`** (`truncate_path` / `truncate_desc`) — `git mv`'d whole into
  `thomas-tui/src/text.rs` (rename preserved history; only doc phrasing + the 2
  doctests changed `use suite_ui::` → `use thomas_tui::`).
- **`centered_rect` / `centered_fixed`** — extracted from `suite-ui`'s
  `widgets.rs` into a new `thomas-tui/src/layout.rs` (with their 3 tests). The
  pane helpers (`pane`/`pane_titled`/`pane_blank`) use `Theme`, so they STAYED in
  suite-ui's widgets.rs — this was a partial split, not a whole-file move.

**Re-export wiring (suite-ui API unchanged):**
- suite-ui `lib.rs` re-exports `truncate_*` from `thomas_tui`; `mod text;` gone.
- suite-ui `widgets.rs` re-exports `centered_*` from `thomas_tui`, so the 3
  overlays (confirm/help/palette) that import `crate::widgets::centered_*` and
  `lib.rs`'s `pub use widgets::{centered_*, ...}` all keep resolving untouched.

**Verified:** test count lossless — suite-ui unit 86→77, thomas-tui unit 6→15
(the 6 text + 3 centering tests moved); doctests suite-ui 18→16, thomas-tui 0→2.
77+15=92 conserved. clippy -D warnings clean on both; fmt clean; gallery builds.

**thomas-tui now owns:** Tui guard, text truncation, centering helpers.

---

## thomas-tui: new general-purpose TUI crate — first extraction (Tui guard)

Started a new, project-agnostic terminal-UI library `crates/thomas-tui`, separate
from `suite-ui` (which stays suite-specific). First component extracted: the
`Tui` RAII terminal scope guard.

**Why the Tui guard first:** it was the only suite-ui component with zero coupling
— no `Theme`, no suite vocabulary (`Severity`/`Health`/`JobState`), deps are just
`crossterm` + `ratatui`. It's also the highest-repeat boilerplate every TUI app
needs (panic-safe raw-mode/alt-screen teardown, `suspended()` for child editors,
post-child input drain) and was already fully tested.

**What changed:**
- New crate `crates/thomas-tui` (Cargo.toml + lib.rs), added to workspace members.
- `git mv crates/suite-ui/src/app/tui.rs → crates/thomas-tui/src/tui.rs` (rename
  preserved history; only 2 doc-comments generalized — no logic changed).
- `suite-ui` now depends on `thomas-tui` (path dep) and **re-exports**
  `Tui/TuiError/TuiOptions` from `app/mod.rs`. suite-ui's public API is unchanged:
  consumers still `use suite_ui::Tui`. Single source of truth, no duplication.

**Verified:** `cargo test -p thomas-tui -p suite-ui` green. Test count is lossless
— baseline suite-ui 92 → now suite-ui 86 + thomas-tui 6 (the relocated guard
tests). Gallery example builds; clippy + fmt clean on the new crate.

**Branch:** `worktree-thomas-tui-extract` (committed in worktree, not yet merged
to main, not pushed). NOTE: suite-ui changes normally land via PR to umbrella main
— but this commit touches suite-ui only by swapping its `Tui` impl for a re-export
(API-identical), alongside the new crate.

**Next candidates (per the extraction plan):**
1. `text.rs` (truncate_path/desc) + the two `centered_*` helpers — also zero-coupling.
2. **Split `Theme`** — pure styling (accent + NO_COLOR gate + prompt/title/dim/
   selection/...) down into thomas-tui; leave `Severity`/`Health` in suite-ui as a
   domain layer. This is the gate that unlocks the Tier-B widgets (SearchBar,
   KeyHints, EmptyState, Counted, FilterChips, ...).
3. Re-derive Tier-C domain widgets (SeverityBadge, StatusBar/JobState, AttentionFlag,
   HealthStrip, ToastKind) as GENERIC primitives in thomas-tui; suite-ui specializes.
