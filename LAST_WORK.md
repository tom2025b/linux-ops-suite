# Last Work

## Rewind Phase 3 — guarded `restore` + `prune` (store maintenance)

2026-06-19. `crates/rewind/`. Completed Rewind's write half on top of Phase 2,
under a hard safety contract. Branch `feat/rewind-phase3` (not yet merged).

- **`rewind restore <capture>`** — the one guarded write. Dry-run by default
  (`restore.rs::plan`, write-free, reuses `diff::diff_entries` as its classifier,
  then overrides the cases a diff can't see — unreadable-in-capture, missing
  object). `--apply` (`restore.rs::apply`) first takes a `pre-restore:<id>` safety
  capture of the live state, then atomic-writes each file (temp-in-parent +
  rename), restoring captured mode and — best-effort — uid/gid via a hand-rolled
  `extern lchown` (`util::set_owner`; can't-set-owner warns, continues). Schema
  *downgrade* (older envelope under a newer live consumer) is flagged. R1–R6
  contract documented at the top of `restore.rs`.
- **`rewind restore --latest-good`** — restores the newest valid-snapshot capture
  (clap `required_unless_present`, so `<capture>` is then optional).
- **`rewind prune`** — `--keep-last N` / `--older-than <Ns|m|h|d>` (hand-parsed) /
  `--gc` mark-and-sweep. Immediate (no dry-run), nothing auto-pruned. Removing a
  capture deletes only its manifest; `--gc` then removes objects no surviving
  capture references (`prune.rs::sweep`). Bad `--older-than` → new
  `RewindError::BadDuration` (exit 3).
- **Exit-code policy** (in `main.rs` only): restore dry-run → 0; `--apply` with a
  per-path failure → **2** (R6, never partial-silent); prune → 0.

Store gained `delete_manifest` (idempotent) + `iter_object_hashes` +
`remove_object` (returns freed bytes). JSON: typed envelopes for restore-plan,
restore-outcome, prune (`skip_serializing_if` so absent label/safety-capture/
false flags are omitted, never null). No new deps. Gate green: fmt --check,
clippy -D warnings (default AND --all-features), 132 rewind unit tests + full
workspace. E2E smoke-tested the binary: capture → drift → dry-run (writes
nothing, exit 0) → `--apply` (writes + pre-restore safety capture in timeline) →
re-diff clean → `prune --keep-last 1 --gc` (reclaims orphan) → bad-duration exit
3 → missing-arg clap exit 2 → JSON envelopes. Fixed a renderer spacing bug
(path/size-transition collision) found during the smoke test.

---

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

---

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
