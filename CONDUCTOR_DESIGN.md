# Conductor Design

Conductor is the Linux Ops Suite's **guided operator**: it reads the suite's
own state, turns that state into a short, ordered runbook — *do these things, in
this order* — and walks the operator through it, delegating each step to the
tool that owns it. It is the single command you run when something needs
attention and you want to be told what to do next, in what order, without having
to remember which tool does what.

Conductor is the calm conductor of an orchestra it does not play. It sequences
and cues; the specialist tools perform. It never writes a live file itself.

## Core Direction

Conductor answers one question:

> Given the suite's current state, what should I do — and in what order?

Pulse answers *is the suite healthy right now?* and stops there: a calm verdict
on a near-empty screen. RexOps is the launcher/cockpit — pick a tool, run it.
Conductor sits between them and fills the gap neither covers: it reads **every**
tool's signal at once, decides what matters most, and produces a **ranked,
ordered plan of action** — then drives that plan step by step, handing each step
to the right specialist.

The shape of the product is the runbook. Where Pulse's true north is the empty
healthy screen, Conductor's true north is a **short, correctly-ordered list of
the right next actions** — and, when nothing is wrong, the honest absence of one.

### Conductor is not Pulse, RexOps, or rex-doctor

This boundary is the whole design, so it is stated explicitly:

| Tool | One-line job | Conductor's relationship |
|---|---|---|
| **Pulse** | Calm read-only *verdict*: is the suite healthy? | Conductor goes one step further: from "what's wrong" to "what to do about it, in order." Conductor may quote Pulse's verdict; it does not replace the glance. |
| **RexOps** | Launcher / cockpit: pick a tool and run it. | Conductor is opinionated *sequence*, not a free catalog. It decides the order; RexOps lets you pick freely. Conductor hands off *up* to RexOps (`r`) exactly as Pulse does. |
| **rex-doctor** | Is the suite *installed/wired* correctly? PASS/WARN/FAIL/SKIP. | Conductor consumes the same kind of environment probe as one input among many, then *acts on it* (a failing wiring becomes a step in the plan). |
| **Tripwire / Bulwark / Rewind / Portman** | Each owns one surface (files / findings / history / network). | Conductor reads their outputs, correlates, and **delegates back to them** for every action. It owns no surface of its own. |

The test for any proposed Conductor feature: *does it produce or drive the
ordered plan?* If not, it belongs in one of the tools above, and Conductor
should delegate to it.

## Research Basis

This design is grounded in the current Linux Ops Suite architecture, the
contract hub, and the established house style of Pulse, Rewind, and Tripwire.

### Repository Role

`linux-ops-suite` is the contract and index headquarters for the suite: shared
architecture docs, the integration map, contract rules, JSON schemas, example
fixtures, and the shared `suite-ui` / `thomas-tui` TUI chrome. It is **not** a
monorepo — most tool logic lives in sibling repos. Conductor lives in this
repo's workspace (`crates/conductor/`) because, like Pulse and Rewind, it is a
*consumer of the suite's published contracts*, not a tool with its own domain.

That role has a hard implication, identical to Pulse's: **Conductor consumes
published artifacts and file contracts; it never reaches into another tool's
internals.** Its only ways to touch another tool are (1) reading that tool's
contract files off disk, and (2) spawning that tool's binary as a foreground
subprocess, exactly the way a human would at the shell.

### Data-Flow Model

The suite is built on one-way, file-based contracts:

```text
Bulwark ----------- workstate-feed JSON ----> Workstate
ToolFoundry ------- workstate-feed JSON ----> Workstate
Proto ------------- workstate-feed JSON ----> Workstate
Workstate --------- snapshot JSON ----------> suite consumers
Toolbox-Bridge ---- sidecar feed -----------> ScriptVault
RexOps ------------ snapshot/report --------> self/report
Tripwire ---------- baseline + drift --------> (local state)
Rewind ------------ captures ----------------> (local store)
```

Conductor is a **reader across all of these**. Crucially, it adds no new
producer contract and no new file other tools must consume: it reads what
already exists and renders a plan. The suite does not have to know Conductor
exists for the rest of it to work — pure additive consumer, the same property
Pulse has.

### Existing Producer Contracts Conductor Reads

From `docs/INTEGRATION_MAP.md` and `contracts/`, the signals available today:

- **Workstate snapshot** (`…/rexops/feeds/workstate.snapshot.json`): per-section
  freshness (`Fresh` / `Stale` / `UnsupportedVersion`), build time.
- **RexOps snapshot** (`…/rexops/snapshot.json`): pre-aggregated `sources`
  (present?) and `attention` items (tool, id, reason, severity) — the richest
  single input when present.
- **Bulwark feed** (`…/workstate/feeds/bulwark.json`): findings with severity.
- **Proto sessions** (`…/proto/sessions/*.json`): job outcomes (passed / failed
  / running).
- **Tripwire**: drift state (added / removed / modified / re-permissioned) via
  its exit code and `--json`.
- **Binary presence on `$PATH`**: the in-process `which`-style probe Pulse
  already does (no `rex-doctor` producer file exists, so Conductor probes
  locally too).

Every reader is **fault-tolerant by contract** (`docs/CONTRACT_RULES.md`): a
missing, empty, malformed, or wrong-major file resolves to "unavailable" and
never panics. serde ignores unknown fields, so additive contract bumps need no
change. This is lifted verbatim from Pulse's `sources.rs` discipline.

### Shared UI Language

Conductor reuses the suite's visual vocabulary from `suite-ui` / `thomas-tui`:
the cyan/amber theme with `NO_COLOR` handling, the health and severity styling,
the rounded panes and key-hint strip. Like Pulse, it borrows the **theme, not
the heavy chrome** — a runbook is a short ordered list, not a dense dashboard.
State is always carried by word and marker shape, never by color alone, so the
interface is fully legible with color off.

### UX Constraints From Prior TUI Work

Carried straight from the suite's accumulated TUI lessons (see PULSE_DESIGN.md):

- Real terminal testing matters; 80×24 needs special care and a compact
  fallback that cannot clip.
- Confirmation and close flows must not rely on Escape alone (operator may be on
  an iPad-style keyboard). Every confirm has an explicit non-Esc path.
- Read-only / unavailable entries must not look interactive.
- A step that *changes* state must be unmistakable and require a deliberate
  keypress — never fire on a stray Enter.

## Safety Philosophy

This is the heart of Conductor, because it is the one suite tool whose explicit
job is to *drive action*. The suite's prime directive is **read-only by
default**; Conductor honors it through one rule:

> **Conductor never mutates state with its own code.**

It has no file-writing path, no restore logic, no feed regeneration. The most
"dangerous" thing Conductor's own code does is **spawn a known sibling binary in
the foreground**, after an explicit operator confirmation. That binary owns the
write and its own safety gate (Rewind's dry-run-by-default restore, Workstate's
snapshot, etc.). Conductor is to those tools what a human at the shell is: it
types the command, the tool decides what's safe.

Concretely, three rings of authority:

```text
 RING 0 — Conductor's own code (always read-only)
   reads contract files, probes $PATH, derives the plan, renders.
   Touches NO live file. Ever. This is 100% of the default screen,
   `status`, `health`, and the rendered plan.

 RING 1 — Delegated, read-only steps (spawn, no confirm needed)
   steps that run a sibling in a read-only mode (e.g. `pulse`,
   `tripwire` check, `bulwark show <id>`, `rewind log`). These only
   read, so Conductor may spawn them on a single Enter.

 RING 2 — Delegated, state-changing steps (spawn ONLY after confirm)
   steps that run a sibling which writes (e.g. `rewind capture`,
   `rewind restore <id>`, `workstate snapshot`). Conductor shows the
   EXACT command, labels it as changing state, and requires a
   deliberate confirm keypress. The sibling's own gate still applies
   on top (defence in depth: Conductor confirms, then e.g. rewind
   restore is itself dry-run-by-default).
```

What Conductor is therefore **allowed** to do:
- Read every contract file and probe the environment (Ring 0).
- Compute and display an ordered plan (Ring 0).
- Spawn a read-only sibling on request (Ring 1).
- Spawn a state-changing sibling *after explicit confirmation*, surfacing the
  literal command first (Ring 2).

What Conductor is **never** allowed to do:
- Write, delete, restore, or regenerate any file in its own code.
- Run any state-changing command without a deliberate confirm.
- Auto-run a plan end-to-end unattended. There is no `--yes-to-all`; each Ring 2
  step is confirmed individually. (A later phase may add a scoped
  `--auto-readonly` that runs only Ring 0/1 steps; Ring 2 always stops.)
- Invent a command. Conductor only ever spawns a fixed, known binary with
  fixed arguments derived from the plan rules — never a string assembled from
  feed contents. (A finding's id is only ever passed as a single argv element to
  a known subcommand, never interpolated into a shell.) No shell is invoked;
  spawns are direct `execvp`-style with an argv vector, so feed content can never
  become a shell metacharacter.

Graceful degradation is part of safety: if a step's binary is not on `$PATH`,
that step renders as *unavailable* with the install hint (rex-doctor style) and
is skipped, never errored. A missing feed narrows the plan; it never crashes it.

## How Conductor Builds the Plan

v1 derives the runbook from a **small, fixed set of rules in code** — no config
file, no scripting language. The rules *are* the product: they encode the
suite's operational know-how ("refresh stale data before you trust it"; "capture
a safety point before you change anything"). They are deterministic and
predictable: the same state always yields the same plan.

The rule engine is a pure function: `state -> ordered Vec<Step>`. It is trivially
unit-testable (feed it a synthetic state, assert the steps and their order), and
it is where the heaviest test coverage lives.

### The v1 rules

Ordered by priority; each fires only when its precondition holds. Steps are
emitted in this order, then de-duplicated:

1. **Trust the data first.** Any *stale* or *unsupported* feed → a Ring 2 step to
   refresh it (`workstate snapshot`, etc.). Rationale: every later step reads
   these feeds; refresh before you trust them. If feeds are fresh, no step.
2. **Wiring gaps.** Any expected suite binary missing from `$PATH` → a Ring 0
   informational step with the rex-doctor-style fix command. (Informational
   because Conductor can't install for you; it tells you the one command.)
3. **Capture before you change.** If the plan will contain *any* Ring 2
   state-changing step beyond a refresh, prepend a `rewind capture
   --label pre-conductor` step (Ring 2, confirmed). Rationale: a safety point
   before guided changes, mirroring Rewind's own auto-capture-before-restore.
4. **Critical/high findings.** For each `critical`/`high` attention item
   (from RexOps aggregate, else Bulwark) → a Ring 1 *investigate* step
   (`bulwark show <id>`), highest severity first.
5. **Correlated drift.** If Tripwire reports drift on a path that *also* appears
   in a finding → raise that finding's investigate step to the top of group 4
   and annotate it ("same file as tripwire drift — start here"). This is the one
   genuinely cross-tool insight and Conductor's signature move.
6. **Failed jobs.** Any failed Proto session → a Ring 1 step to review it.
7. **All clear.** If none of the above fired → no plan; Conductor says so (see
   the healthy screen) and exits 0.

Each rule, its precondition, and its emitted step is documented next to the code
so the plan is never a black box. New rules are additive and independently
testable. (User-authored runbook files — a config-driven plan source — are a
deliberately deferred future phase, not v1; the rule engine's `state -> steps`
signature leaves room for it without rework.)

## Command Surface

Conductor follows the suite's thin-`main` + clap pattern (Rewind's `main.rs` is
the template). Bare `conductor` is the primary interface and drops the operator
into the TUI, because RexOps launches tools with no arguments and expects a
useful bare invocation (`docs/INTEGRATION_MAP.md`, "Launching from RexOps").

```text
conductor                  Open the TUI: the plan, ready to walk. (bare = primary)
conductor status           One-shot, non-interactive: print the situation +
                           the ordered plan as text, then exit. Read-only,
                           greppable, CI-friendly. (Ring 0 only — never runs a step.)
conductor health           One-shot: the suite's readiness as Conductor sees it
                           — per-tool present/fresh/degraded — exit code reflects
                           worst state (cron-friendly). Read-only.
conductor plan             Alias surface for `status` focused on just the steps
                           (the bare ordered list, no situation prose). Read-only.
conductor orchestrate      Interactive guided run: walk the plan step by step,
                           confirming each Ring 2 step. The "drive it" verb.
                           (In the TUI this is the Enter/▸ flow; `orchestrate`
                           is the CLI entry to the same driver.)
conductor doctor           Pass-through convenience: show the wiring/install
                           findings (Ring 0) with fix commands. Distinct from
                           rex-doctor only in that it's Conductor's own view of
                           the same probe; delegates to rex-doctor if present.

Global flags (suite-standard, mirror rewind):
  --json                   Emit the JSON envelope instead of human output
                           (schema_version + source_tool="conductor"). Implies
                           non-interactive. Available on status/health/plan.
  --no-color               Force monochrome (also auto-off when not a TTY).
  --data-dir <DIR>         Read suite contracts from DIR instead of the XDG
                           default (mirrors PULSE_DATA_DIR; for tests/power use).
  -v, --verbose            Show rule provenance: why each step is in the plan.
  -h, --help               Help.
```

Exit codes (suite convention, extending Pulse/Rewind/Tripwire):

```text
  0  ok — read cleanly; `health`/`status` ran; a guided run finished or the
         operator quit cleanly; "all clear" with no plan.
  3  conductor itself could not run (no data dir resolvable, etc.).
  1  reserved: a delegated step a guided run depended on failed.
  2  reserved: a guided run ended with steps still pending/skipped.
```

(1 and 2 are reserved now and wired when `orchestrate` lands, exactly as Rewind
reserved 1/2 for diff/restore.)

### Why these verbs

- `status` / `plan` / `health` are the **read-only** triplet — the safe,
  scriptable surface. They never run a step. These ship first.
- `orchestrate` is the **only** verb that can spawn a Ring 2 step, and only
  interactively with per-step confirms. Isolating the one acting verb keeps the
  safety story legible: if you never type `orchestrate` (or press Enter on a step
  in the TUI), Conductor cannot change anything.
- `doctor` exists because wiring problems are the most common "why is the suite
  lying to me" cause, and surfacing them with the fix is high-value and 100% Ring
  0.

## The TUI

Conductor's TUI is calmer than RexOps and busier than Pulse — it is a **short
ordered list with one focused step**, never a multi-panel cockpit. It reuses the
`suite-ui` theme but stays restrained: the plan is the screen.

### All clear (the healthy screen)

When no rule fires, Conductor has nothing to conduct. It says so plainly and
gets out of the way — close kin to Pulse's empty screen, but it speaks in
Conductor's voice (*nothing to do*, not *all clear* — that's Pulse's word).

```text



                             nothing to conduct

                          the suite is healthy and
                           every feed is current

                                                              checked 1m ago
```

No plan, no steps, no chrome. One line, a soft reason, a dim timestamp. The
absence of a runbook is the signal: there is genuinely nothing to do.

### The plan (something needs attention)

```text
 conductor                                                    19 Jun 14:22

   the situation

   workstate snapshot is 4h stale — refresh before trusting feeds
   1 critical finding correlates with a tripwire drift

   the plan                                                   4 steps

   ▸ 1  refresh stale data                              changes state
         workstate snapshot
   ○ 2  capture a safety point                          changes state
         rewind capture --label pre-conductor
   ○ 3  investigate deploy-prod.sh   ← same file as tripwire drift
         bulwark show deploy-prod.sh                        read-only
   ○ 4  review failed job: nightly-backup
         proto show nightly-backup                          read-only

 ────────────────────────────────────────────────────────────────────────
 enter  run step      s  skip      a  advance      r  rexops      ?  help
```

Reading rules for the plan screen:
- **`the situation`** is one or two plain sentences naming *why* there's a plan —
  the human summary above the mechanical list. Omitted when the plan is a single
  obvious step.
- **`▸`** marks the current step; **`○`** a pending step; a completed step gets a
  dim **`✓`** and its command greys out.
- The right-edge tag is the **ring**: `changes state` (amber, Ring 2) vs
  `read-only` (dim, Ring 1). It is always present so the operator always knows
  what a step will do before pressing Enter. Color is a bonus; the words carry it.
- The **`←` annotation** is rule 5's correlation note — Conductor's signature
  cross-tool insight, shown inline on the step it elevates.
- The literal command is shown under every step. No hidden actions: what you see
  is exactly what gets spawned.

### Running a step

`Enter` on the current step runs it. A Ring 1 (read-only) step spawns
immediately, hands over the real terminal, and returns to the plan with the step
checked. A **Ring 2 (changes state)** step first drops a confirm:

```text
   ▸ 1  refresh stale data                              changes state

        this will run:  workstate snapshot
        it changes suite state (regenerates the snapshot).

        y  run it        s  skip        q  back to plan
```

The confirm requires `y` (not Enter — a stray Enter cannot fire a state change),
and offers an explicit non-Esc back path (`q`), per the suite's keyboard
constraint. On `y`, Conductor spawns the bare known binary in the foreground,
shows its output, marks the step `✓`, and advances. The spawned tool's own
safety gate still applies (e.g. `rewind restore` is itself dry-run-first).

### Navigation

```text
 enter  run step      s  skip      a  advance      r  rexops      ?  help
```

- `Enter`: run the current step (Ring 2 routes through the confirm).
- `s`: skip the current step (advance without running). Skipped steps are noted,
  not lost.
- `a`: advance focus without running (read down the plan).
- `r`: hand off *up* to the RexOps cockpit (`rexops tui`), the same foreground
  handoff Pulse offers — for when the operator wants the free launcher instead of
  the guided sequence. No-op with one dim line if `rexops` isn't on `$PATH`.
- `?`: help; reveal the hint strip.
- `q`: quit (always works; omitted from the strip to keep it narrow).

Layout invariants follow Pulse: a constant vertical structure, a compact
fallback for terminals below ~80×24 that renders the plan as a plain unpadded
list (never clips), and color that only ever carries state.

## Color Rules

Same palette and discipline as Pulse — color is rare and always means something:

- **Amber**: `changes state` (Ring 2) tags and the stale/degraded reasons.
- **Red**: a `critical` severity in the situation/steps.
- **Cyan**: the current-step `▸` focus marker only — never decoration.
- **Green**: the healthy "nothing to conduct" line and a completed `✓`.
- **Dim gray**: commands, `read-only` tags, timestamps, secondary prose.

Fully legible with color off: ring is also a word (`changes state` /
`read-only`), step state is also a glyph (`▸ ○ ✓`), severity is also a word.

## Folder / File Structure

`crates/conductor/`, modeled exactly on Rewind/Tripwire (thin `main`, library
does the work, renderers derive from a model, hand-rolled terminal bits with no
extra dependency where Pulse already proved it):

```text
crates/conductor/
  Cargo.toml          # workspace deps only: clap, serde, serde_json, chrono,
                      #   thiserror, suite-ui, thomas-tui. No new 3rd-party dep.
  README.md           # mirrors rewind/README.md: what it is, the rings, usage.
  src/
    main.rs           # thin CLI: parse flags, dispatch subcommand, render
                      #   human/JSON, structured exit code. (rewind/main.rs shape)
    lib.rs            # public surface: load_state(), build_plan(), the orchestrator
                      #   entry. Library does the work; main only chooses + prints.
    error.rs          # ConductorError (thiserror), e.g. NoDataDir. Read errors are
                      #   absorbed into "unavailable", not surfaced as errors.
    sources.rs        # fault-tolerant readers for every contract Conductor reads
                      #   (Workstate snapshot, RexOps snapshot, Bulwark feed, Proto
                      #   sessions, $PATH probe). Lifts Pulse's sources.rs discipline
                      #   verbatim: missing/malformed => unavailable, never panic.
    state.rs          # the SuiteState model: the normalized snapshot of everything
                      #   Conductor read, with no rendering or rule logic. The single
                      #   input to the rule engine.
    plan/
      mod.rs          # Plan + Step types (id, title, command, Ring, annotation,
                      #   status) and the public `build(&SuiteState) -> Plan`.
      rules.rs        # the v1 rules as pure functions state -> steps, in priority
                      #   order. The product's brain; the heaviest unit tests.
    run.rs            # the delegated-spawn layer: foreground spawn of a known
                      #   binary with a fixed argv (NO shell), Ring-2 confirm gate,
                      #   $PATH availability check, result capture. The only module
                      #   that touches a subprocess.
    report.rs         # one-shot human + JSON renderers for status/health/plan
                      #   (the envelope: schema_version + source_tool="conductor").
                      #   Pure: state/plan in, text out — snapshot-testable.
    tui/
      mod.rs          # the interactive app: event loop, the plan screen, the
                      #   Ring-2 confirm modal, the healthy screen, compact fallback,
                      #   navigation. Uses thomas-tui's RAII terminal guard.
      style.rs        # Style resolver (color on iff TTY && !NO_COLOR), ring/severity
                      #   /step-glyph styling. Same shape as pulse's Style.
```

Module-boundary intent (so each unit has one clear job):
- `sources.rs` only *reads files* → returns raw, tolerant values.
- `state.rs` only *holds the normalized facts* → no I/O, no rules, no rendering.
- `plan/rules.rs` only *decides steps from facts* → pure, deterministic, tested.
- `run.rs` only *spawns a sibling safely* → the single subprocess choke point.
- `report.rs` / `tui/` only *render* → derive from state + plan, no logic.

A change in how a step is rendered can't affect what steps exist; a new rule
can't change how spawning works. That separation is the testability story.

## Testing Strategy

Mirrors the suite's proven approach (Pulse's `sources.rs` temp-dir readers,
Rewind's roundtrip tests, Pulse's render snapshot tests):

- **`sources.rs`**: temp-dir fixtures per contract — missing / empty / malformed
  / wrong-major / valid — assert every failure mode resolves to "unavailable"
  and never panics. (Directly ported from Pulse's tests.)
- **`plan/rules.rs`** (the core): feed synthetic `SuiteState` values, assert the
  exact ordered steps and their rings. Cover each rule in isolation and key
  interactions — especially rule 5 (drift × finding correlation lifts the step)
  and rule 3 (a Ring 2 step in the plan prepends the safety capture). This is the
  densest test module because it is the product.
- **`report.rs`**: snapshot-test the `status`/`health` text and the JSON
  envelope for healthy, single-step, and multi-step states, at 80×24 and a
  compact size; assert no line exceeds the viewport (Pulse's width invariant).
- **`run.rs`**: assert the `$PATH` availability check, that a Ring 2 spawn is
  *not* attempted without the confirm, and that the argv is a fixed vector with
  no shell (a finding id is one argv element, never interpolated). Spawning is
  abstracted behind a tiny trait so tests assert "would spawn `X` with argv
  `[…]`" without launching a real process.
- **TUI**: a deterministic `--dump-view`-style frame render (as Pulse has) so the
  plan screen, the confirm modal, and the healthy screen can be snapshot-tested
  without a PTY.
- **Exit codes**: assert 0 vs 3 paths; 1/2 are reserved until `orchestrate`.

## Workspace & Installer Integration

Two registrations, both following the existing pattern exactly:

1. **Workspace** — add `"crates/conductor"` to `members` in the root
   `Cargo.toml` (alongside `crates/rewind`).
2. **Installer** — `crates/conductor` is an in-repo workspace crate, so it joins
   `WORKSPACE_TOOLS` in `install.sh`:

   ```sh
   WORKSPACE_TOOLS=(
     "toolbox-bridge:toolbox-bridge"
     "rex-check:rex-check"
     "conductor:conductor"        # ← added
   )
   ```

   That single line is enough: the installer already builds each
   `WORKSPACE_TOOLS` crate with `cargo build --release -p <crate>` and installs
   the `<binary>` onto `PATH`, and the verification/uninstall loops iterate the
   same array. The binary must be named `conductor` and be a real file on `$PATH`
   (not a shell alias) so RexOps's `which conductor` launcher resolves it — and
   so each plan step that spawns a sibling can resolve *those* binaries the same
   way (`docs/INTEGRATION_MAP.md`, "Launching from RexOps").

   **No `r-conductor` wrapper and no alias** are part of this design — the suite
   convention here is a bare binary on `PATH`, per the project's standing rule.

## Phasing

> **Status (v0.3.0): all three phases have shipped.** This section is the original
> roadmap, kept for the design rationale. As of the 0.3.0 release Conductor is the
> full tool described above: the read-only triplet (`status`/`health`/`plan`/`--json`),
> the interactive TUI (bare `conductor`, Phase 2), and the `orchestrate` driver with
> per-step confirmation (Phase 3) are all implemented in `crates/conductor`. Conductor
> reads the suite's state through the canonical Workstate snapshot via the
> `workstate-schema` crate (the single source of truth) and owns no snapshot model of
> its own. Read the phases below as *how it was built*, not as outstanding work.

Tight v1, with the highest-risk surface deferred behind the read-only triplet —
the same staging Rewind used (ship reads first; the one writing path lands last
with the heaviest gate).

```text
 Phase 1  (foundation, all Ring 0 — ship first)
   sources.rs + state.rs + plan/rules.rs + report.rs.
   Surface: `conductor status`, `conductor health`, `conductor plan`, `--json`.
   100% read-only. No subprocess, no TUI yet. This alone is a useful tool:
   "tell me the situation and the ordered plan as text."

 Phase 2  (the TUI, Ring 0/1)
   tui/ : the plan screen, the healthy screen, compact fallback, navigation,
   and Ring 1 (read-only) step spawning. Bare `conductor` opens here.
   Still cannot change state — Ring 2 steps render with their tag and command
   but selecting one is a no-op-with-note until Phase 3.

 Phase 3  (the driver, Ring 2 — heaviest gate, lands last)
   run.rs Ring 2 path + the confirm modal + `conductor orchestrate`.
   The ONLY phase that can spawn a state-changing command, and only after a
   per-step `y` confirm. Wires exit codes 1/2. Gets the most test scrutiny.

 Future (explicitly not v1)
   User-authored runbook files (config-driven plan source); a scoped
   `--auto-readonly` that runs Ring 0/1 steps unattended (Ring 2 always stops).
```

## Design Principles

- One job: produce and drive the **ordered plan**. Anything that isn't that
  belongs in a tool Conductor delegates to.
- Conductor conducts; it never plays. **No state-changing code of its own** —
  every change is a confirmed spawn of the tool that owns it.
- Read-only by default is sacred: the entire default screen, `status`, `health`,
  and `plan` touch no live file.
- The plan is honest: every step shows its literal command and whether it changes
  state, before it runs. No hidden actions.
- A state-changing step always takes a deliberate, distinct keypress (`y`), never
  a stray Enter; always an explicit non-Esc back path.
- When there's nothing to do, say so and leave — the absent runbook is the
  signal, kin to Pulse's empty screen.
- Graceful degradation everywhere: a missing feed narrows the plan; a missing
  binary greys out a step with its fix; nothing ever panics.
- Determinism: the same suite state always yields the same plan. The rules are in
  code, documented, and unit-tested — never a black box.
- Borrow the suite's theme, not its heavy chrome. A runbook is a short list, not
  a cockpit.
