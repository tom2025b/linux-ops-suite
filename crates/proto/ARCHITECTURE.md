# Proto — Architecture

Proto is the **guided protocol / checklist runner** for the Linux Ops Suite. It
loads human-authored protocols (YAML), walks an operator through their steps one
at a time, and records the outcome as a session (JSON). It guides and records.
Command steps are read-only by default — Proto displays the command and the
operator reports the result — but auto-run is available as an **explicit,
confirm-gated opt-in**: Proto shows the command and only executes it (via
`sh -c`, output inherited so the operator watches it live) after a per-command
`y`, then derives the step outcome from the exit code. Proto never runs anything
without that explicit confirmation, and never on its own behalf.

## Crate shape

- **Library + thin binary.** All real logic — models, loading, validation, the
  run engine — lives in `src/lib.rs` (and its modules). `src/main.rs` is a thin
  clap shell that parses arguments and dispatches into the library. This keeps the
  logic unit-testable and reusable; tests link the library directly instead of
  spawning a process. Cargo auto-detects both targets from the standard file
  layout, so there are no explicit `[lib]` / `[[bin]]` sections.
- **Single crate, not a workspace.** Proto does one job and ships its session
  output to RexOps/Workstate as a plain JSON file. There is no in-process linking
  with another tool that would justify splitting into sub-crates. (Contrast:
  Bulwark is a workspace because it has a separable core + TUI.)
- **Edition 2024**, matching the rest of the suite.

## Suite philosophy: file-based contracts, not shared code

Proto never imports another suite tool. It reads YAML it owns (`protocols/*.yaml`)
and writes JSON others can read (sessions, and a workstate feed). Producers and
consumers stay decoupled through files whose shapes are pinned by the suite's
JSON Schemas — this is the whole-suite contract rule.

## Data formats

One set of `serde`-derived models powers two wire formats:

- **Protocols (YAML)** are authored by humans — YAML is the friendliest
  hand-editable format (comments, block strings, no trailing-comma traps).
- **Sessions (JSON)** are machine state for other suite tools (a RexOps panel, a
  Workstate feed) to read — JSON is the suite's lingua franca for contracts.

Timestamps (`started_at`, per-step answers, `finished_at`) use `chrono` with the
`serde` feature, serialized as RFC 3339 strings to match the suite timestamp
contract.

## Error handling

A deliberate division of labour, not redundancy:

- **`thiserror` in the library** — typed error enums a caller may want to
  distinguish (file-not-found vs. malformed YAML vs. failed validation).
- **`anyhow` in the binary (`main.rs`) only** — the CLI doesn't match on error
  variants; it reports a friendly message and exits non-zero.

## Check execution

There is **one** execution engine — `core::executor` — with two output modes, so
the auto-check flow and a confirmed `command:` step can't drift apart in their
timeout or process-group behaviour:

- **Captured** (`execute_check`/`execute_profile`, used by the auto-check flow):
  built-in check commands are split with `shlex` for predictable shell-like
  quoting (e.g. `cargo test -- --foo`) and spawned directly (no per-check shell);
  stdout/stderr are captured on reader threads for the batch summary.
- **Streaming** (`run_streaming`, used by an interactive `command:` step): run via
  `sh -c` so the authored string keeps full shell semantics (pipes, `&&`,
  quoting), with stdout/stderr inherited so the operator watches it live.

Both share the same wait loop: a deadline-based timeout (10 min batch, 5 min for
an interactive step the operator is watching) and, on Unix, `libc::kill` on the
**negated process-group id** so a timed-out command's `sh -c` grandchildren don't
survive. The exit code maps to pass/fail; a spawn failure or timeout is an error.

## Trust model

A protocol YAML is **executable content**: a `command:` field is a string Proto
will run (after a per-command `y`). Treat the protocols directory like a
`Makefile` — only run protocols you trust, and only point `--dir` at a directory
you control. Proto never runs a command without explicit per-command
confirmation, and never runs anything on its own behalf, but it does not sandbox
or allow-list what an authored command may do.

## Tests

Core unit tests use a hand-rolled temp dir to stay dependency-light (mirroring
Workstate). The black-box CLI tests spawn the real `proto` binary against a
throwaway protocols directory via `tempfile` (a dev-dependency, compiled only for
tests), mirroring Bulwark's workstate-feed contract test.
