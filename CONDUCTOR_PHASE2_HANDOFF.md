# Conductor — Phase 2 Handoff (for the next AI / engineer)

You are picking up **Conductor**, the Linux Ops Suite's *guided operator*.
**Phase 1 (Ring-0, read-only) is DONE and merged.** Your job is **Phase 2: the
interactive TUI + Ring-1 (read-only) step spawning.** Read this whole file, then
`CONDUCTOR_DESIGN.md` at the repo root before writing any code.

---

## 0. Orientation — read these first (in order)

1. `CONDUCTOR_DESIGN.md` (repo root) — the authoritative design. The **"The TUI"**
   and **"Safety Philosophy"** sections are your spec for Phase 2.
2. `docs/superpowers/plans/2026-06-19-conductor-phase-1.md` — the Phase 1 plan;
   shows the exact TDD cadence and house style to mirror.
3. `crates/conductor/src/**` — the Phase 1 code you build on. It is small; read
   all of it. `crates/pulse/src/{app.rs,tui.rs,main.rs}` is the suite's existing
   TUI you should imitate.
4. `LAST_WORK.md` (top entry) — narrative of what Phase 1 delivered.

**House rules (non-negotiable, from the user + suite):**
- Read-only by default is sacred. **Conductor never mutates state with its own
  code.** Phase 2 may *spawn* a Ring-1 (read-only) sibling, nothing more.
- Keep it simple. Thin `main` → library does the work → renderers derive from the
  model. No new third-party dependency without a strong reason (suite-ui /
  thomas-tui + std is the bar; pulse hand-rolls ANSI + libc).
- NO `r-conductor` wrapper, NO shell alias. Bare binary on `$PATH`.
- NO_COLOR + non-TTY ⇒ monochrome; state always carried by word + glyph, never
  color alone. Compact (<80×24) fallback that cannot clip.
- TDD: failing test → minimal impl → green → commit. Per-task commits. Run
  `cargo test -p conductor` + `cargo clippy -p conductor --all-targets -- -D
  warnings` + `cargo fmt -p conductor -- --check` before every commit.

---

## 1. Where the work lives (git state)

- Repo: `~/projects/linux-ops-suite` (this is the umbrella/contract repo + cargo
  workspace; siblings live elsewhere — see the suite-repo-topology note).
- Phase 1 was built on branch **`worktree-conductor-design`** inside a git
  worktree at `.claude/worktrees/conductor-design/`, then merged `--no-ff` into
  **local `main`** (commit `e061b0c`). It is VERIFIED green on main.
- **Caveat:** at handoff time, `git push origin main` was rejected because
  `origin/main` had diverging commits. Resolving that (pull/rebase on shared
  main) is a human decision — confirm the remote state before you push anything.
- **Do not push, commit to main, or force anything without explicit human
  approval.** The user requires per-action approval for commit/push/PR/merge.

To work on Phase 2: branch from the up-to-date `main` (e.g. a new worktree
`conductor-phase2`), don't reuse the Phase 1 branch.

---

## 2. What Phase 1 gives you (the foundation you build on)

Crate: `crates/conductor/`. All Ring-0, 100% read-only, no subprocess, no TUI.

```text
src/
  main.rs        thin clap CLI: status (default) | health | plan
  lib.rs         load_state(&DataDir) -> SuiteState   ← your TUI calls this
  error.rs       ConductorError::NoDataDir (exit 3)
  util.rs        stdout_is_tty(), data_root() (XDG, fallback ~/.local/share)
  sources.rs     fault-tolerant readers + DataDir{new,from_env,*paths*}
                 read_feeds / read_findings / read_failed_jobs / read_drift /
                 read_binaries ; SUITE_BINARIES (the 8 probed bins)
  state.rs       SuiteState (the normalized facts) + Freshness/Severity/Finding/
                 FeedStatus/BinaryStatus/DriftedPath/FailedJob
  plan/
    mod.rs       Plan, Step{id,title,command,ring,annotation,status},
                 Ring{ReadOnly,ChangesState,Info}, StepStatus{Pending,Done,Skipped},
                 slug(), Plan::plan_id() (FNV-1a), build(&SuiteState)->Plan
    rules.rs     the 7 v1 rules (pure). DO NOT change rule semantics in Phase 2.
  report.rs      Style{resolve(force_off)}, print_status/print_plan/print_health,
                 status_json/health_json (envelope: schema_version=1,
                 source_tool="conductor", plan_id, step.id)
tests/cli.rs     end-to-end CLI tests (note the stub-bin-dir trick, see §5)
```

**Key API you will consume from the TUI:**
- `conductor::load_state(&DataDir) -> SuiteState`
- `conductor::plan::build(&SuiteState) -> Plan`
- `Plan { situation: Vec<String>, steps: Vec<Step> }`, `Plan::is_empty()`
- `Step { id, title, command: Option<String>, ring: Ring, annotation: Option<String>, status: StepStatus }`
- `Ring::tag()` → `"read-only" | "changes state" | "info"`

**Exit codes:** 0 ok / 3 can't-run. `1`/`2` are RESERVED for Phase 3
(`orchestrate`) — do not emit them in Phase 2.

---

## 3. Phase 2 scope — what to build

Goal: bare `conductor` opens an interactive TUI that renders the plan and lets the
operator **run the read-only (Ring 1) steps**, walking down the list. It STILL
cannot change state — Ring-2 steps render (with their `changes state` tag +
command) but selecting one is a **no-op-with-note** until Phase 3.

Deliverables:
1. `src/tui/mod.rs` — the interactive app (event loop, the plan screen, the
   healthy "nothing to conduct" screen, compact <80×24 fallback, navigation).
   Use **thomas-tui's RAII terminal scope guard** (panic-safe setup/teardown,
   suspend-for-child) — see how pulse uses it. Reuse **suite-ui** theme.
2. `src/tui/style.rs` — Style resolver mirroring pulse's (color iff TTY &&
   !NO_COLOR), ring/severity/step-glyph styling.
3. `src/run.rs` — **Ring-1 path ONLY**: foreground spawn of a read-only sibling
   (e.g. `pulse`, `bulwark show <id>`, `tripwire`, `rewind log`, `proto show
   <id>`) on Enter, after a `$PATH` availability check. **Direct argv, NO shell**
   (so a finding id can never become a shell metacharacter — a finding's id is
   one argv element). Suspend the TUI, hand over the real terminal, resume on
   child exit, mark the step `Done`. Abstract the actual spawn behind a tiny
   trait so tests assert "would spawn X with argv […]" without launching.
4. Wire bare `conductor` (no subcommand) to launch the TUI when stdout is a TTY;
   keep `status`/`health`/`plan` as the non-interactive verbs. Mirror pulse's
   `should_run_interactive(clear, tty, live)` gate. A non-TTY bare invocation
   must stay scriptable (fall back to `print_status`).

Navigation (from CONDUCTOR_DESIGN.md "The TUI"):
- `Enter` run current step (Ring 1 spawns; Ring 2 = no-op + dim note "needs
  Phase 3"; Info = no spawn, it's just a fix hint).
- `s` skip · `a` advance focus without running · `r` hand off to `rexops tui`
  (foreground, like pulse's `r`; no-op + one dim line if rexops not on PATH) ·
  `?` help · `q` quit (always works). **Avoid Escape-only flows.**
- `▸` current · `○` pending · `✓` done · skipped greyed. Glyphs carry state.

Screens (ASCII mockups are in CONDUCTOR_DESIGN.md → "The plan" and "All clear").
The Ring-2 confirm modal shown in the design is **Phase 3**, not Phase 2 — in
Phase 2 a Ring-2 step simply won't run.

---

## 4. Out of scope for Phase 2 (do NOT build)

- Any Ring-2 execution, the `y`-confirm modal, or `conductor orchestrate` → Phase 3.
- Any file write / restore / feed regeneration by Conductor itself — ever.
- User config files / custom runbooks / rule overrides.
- Changing the Phase 1 rule semantics or the JSON envelope shape.
- `--auto-readonly` / unattended runs.

---

## 5. Testing strategy (mirror Phase 1)

- **Deterministic frame render:** add a `--dump-view <name>` style path (pulse
  has exactly this) so the plan screen, the healthy screen, and the compact
  fallback can be **snapshot-tested without a PTY**. This is the backbone of TUI
  testing here — do it early.
- **`run.rs`:** assert the `$PATH` check; assert a Ring-1 spawn builds the right
  **argv vector** (no shell); assert a Ring-2 step is **not** spawned. Spawning
  behind a trait → no real process in tests.
- **Width invariant:** no rendered line exceeds the viewport at 80×24 and a
  compact size (pulse asserts this — copy it).
- **GOTCHA you will hit (Phase 1 hit it):** the binary probe reads the real
  `$PATH`, and this dev box has 5 of 8 suite bins installed. Any test that
  expects "nothing to conduct" or a wiring-free plan MUST neutralize `$PATH`.
  Phase 1's `tests/cli.rs` does this by creating a `bin/` dir of **stub
  executables for all 8 SUITE_BINARIES** and pointing `PATH` there. Reuse that
  pattern. (Don't "fix" it by clearing PATH — that makes ALL 8 missing, the
  opposite failure.)

---

## 6. Suggested task breakdown (TDD, per-task commits)

```text
P2-T1  src/tui/style.rs — Style resolver + ring/glyph/severity colors (unit tests)
P2-T2  pure frame renderers for the TUI: plan screen, healthy screen, compact
       fallback — as String-returning fns (snapshot tests, width invariant).
       (Keep the event loop OUT of these so they're testable.)
P2-T3  src/run.rs — Ring-1 spawn behind a Spawner trait + $PATH check + argv
       construction (NO shell). Tests assert intent, no real process.
P2-T4  src/tui/mod.rs — the event loop + navigation wiring (Enter/s/a/r/?/q),
       step status transitions, Ring-2 no-op-with-note, thomas-tui guard.
P2-T5  main.rs — bare `conductor` launches the TUI on a TTY; --dump-view path;
       keep status/health/plan. Integration tests via --dump-view.
P2-T6  README + LAST_WORK.md update; full workspace clippy/build/fmt; self-review.
```

Acceptance: bare `conductor` opens the TUI; Ring-1 steps run and mark done; Ring-2
steps render but don't run; healthy screen + compact fallback render; all tests
green, clippy + fmt clean, workspace builds; still zero writes, zero Ring-2.

---

## 7. Gotchas & conventions (learned building Phase 1)

- `Style::resolve(force_off)` returns empty strings when color off, so call sites
  interpolate unconditionally. Keep that pattern.
- clippy `-D warnings` is strict: it flagged needless lifetimes (`fn f<'a>(…
  &'a Style) -> &'a str` where return is `&'static str`), `sort_by` →
  `sort_by_key(Reverse(..))`, `&PathBuf` params → `&Path`, and unused imports.
  Fix these as you go.
- `cargo fmt` will reformat multi-line `.unwrap()` chains and long `Step::new(…)`
  calls — run it before committing so the fmt-check gate is clean.
- The suite JSON envelope is `serde_json::to_string_pretty` of a struct whose
  first two fields are `schema_version: 1` then `source_tool: "conductor"`.
- Step `command` is shown VERBATIM in the UI — what you display is exactly what
  `run.rs` would spawn. Never assemble a command string from feed content beyond
  passing an id as a single argv element.

---

## 8. Definition of done for Phase 2

- [ ] Bare `conductor` (on a TTY) opens the TUI; non-TTY falls back to status.
- [ ] Plan screen, healthy screen, compact fallback render; width-safe; NO_COLOR
      legible.
- [ ] Ring-1 steps spawn (foreground, argv, no shell) and mark `Done`; Ring-2
      steps render with tag+command but are a no-op-with-note; Info steps don't
      spawn.
- [ ] `r` hands off to `rexops tui` (graceful no-op if absent); `q` always quits;
      no Escape-only flow.
- [ ] Tests: snapshot frames + run.rs intent tests + width invariant, all green;
      clippy `-D warnings` clean; `cargo fmt --check` clean; `cargo build
      --workspace` clean.
- [ ] README + LAST_WORK.md updated. No Ring-2, no writes, no orchestrate.
- [ ] Nothing committed/pushed to a shared branch without explicit human approval.

Good luck. Keep it calm, keep it read-only, and let the specialist tools play —
Conductor only conducts.
