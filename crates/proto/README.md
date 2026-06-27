# Proto

Guided protocol / checklist runner for the **Linux Ops Suite**. Proto loads a
human-authored checklist (a *protocol*, in YAML), walks you through it one step
at a time, and records the outcome as a session (JSON) you can keep as an audit
trail.

## What it does

- **Loads protocols** from `protocols/*.yaml` — ordered, human-editable checklists.
- **Validates** them strictly (slug ids that match the filename, unique step ids,
  non-empty titles, at least one step) — reporting *every* problem in a file at
  once — before they ever run.
- **Runs them interactively** — shows each step, asks yes / no / skip (or
  acknowledge, for info steps), and tallies a summary.
- **Records a session** as JSON (`sessions/<id>-<timestamp>.json`) with a
  contract header (`schema_version`, `source_tool`, `generated_at`) so other
  suite tools can read it later.
- **Feeds Workstate** — after each run it (re)writes a rolling
  `workstate/feeds/proto.json` so completed runs surface in RexOps automatically.
- **Manages history** — `search`, `export` (Markdown / JSON), and `delete` over
  saved sessions.

Proto is **read-only by default**: `command` steps are *displayed* and you run
them yourself and tell Proto the result. Auto-run is an **explicit opt-in** —
for a `command` step Proto offers to run it for you, and only on a per-command
`y` does it execute the command (via `sh -c`, output streamed live, under a
timeout that kills a hung command's whole process group) and record the outcome
from the exit code. Proto never executes anything without that confirmation;
decline and it falls back to the manual y/n/s self-report.

> A protocol's `command:` is executable content. Treat your protocols directory
> like a `Makefile`: only run protocols you trust, and only point `--dir` at a
> directory you control. Proto confirms every command but does not sandbox it.

## Install / run

Built with Cargo (Rust edition 2024):

```sh
cargo build --release        # compile
cargo run -- list            # list available protocols
cargo install --path .       # install `proto` to ~/.cargo/bin (on PATH)
```

Installing puts a real `proto` binary on your `$PATH`, which is what lets the
RexOps cockpit launch it (RexOps resolves tools with `which <id>`). Running bare
`proto` on a terminal opens an interactive **picker** — choose a protocol and go.

## Commands

```sh
proto list                       # list every valid protocol
proto validate                   # validate ALL protocols (non-zero exit if any fail)
proto validate <id>              # validate a single protocol
proto run <id>                   # walk through a protocol interactively
proto sessions                   # list past runs (newest first)
proto show <session-id>          # show one past run in detail
proto search <text>              # find sessions by protocol id/title or step note
proto export <session-id>        # export a session as Markdown (default) to stdout
proto export <session-id> --json # export the raw session JSON
proto export <session-id> --out run.md   # write the export to a file
proto delete <session-id>        # delete a session (confirms; --yes to skip)
proto feed                       # regenerate the Workstate feed from saved sessions
proto --dir <path> ...           # use a different protocols directory (default ./protocols)
proto --sessions-dir <path>      # use a different session store
proto --feed-dir <path>          # use a different Workstate feed directory
proto --no-feed run <id>         # run without (re)writing the Workstate feed
```

Example:

```sh
proto run rust-repo-review
proto sessions
proto show rust-repo-review-20260606T072436Z
proto export rust-repo-review-20260606T072436Z > review.md
```

## Sessions

Each `proto run` writes one session record (JSON) to the session store, which
defaults to **`$XDG_DATA_HOME/proto/sessions`** (falling back to
**`~/.proto/sessions`**). A session captures the protocol, timing, and every
step's outcome plus any note you attached — an audit trail you can revisit with
`proto sessions` / `proto show`, search with `proto search`, share with
`proto export`, or remove with `proto delete`. The format is pinned by the suite
contract
[`proto.session`](https://github.com/tom2025b/linux-ops-suite/blob/main/contracts/proto.session.schema.json)
so other suite tools can read it.

## Workstate feed

After each run (unless `--no-feed`), Proto also (re)writes a rolling **Workstate
feed** — a single `proto.json` summarizing your most recent runs — to
**`$XDG_DATA_HOME/workstate/feeds/`**, the same place Bulwark and ToolFoundry
drop their feeds. This is what makes completed Proto runs show up in RexOps with
no manual export step. Regenerate it any time with `proto feed`. The format is
pinned by the suite contract
[`proto.workstate-feed.v1`](https://github.com/tom2025b/linux-ops-suite/blob/main/contracts/proto.workstate-feed.v1.schema.json).

## Protocol format

A protocol is a YAML file with an `id`, `title`, optional `description`/`version`,
and an ordered list of `steps`. Each step has an `id`, `title`, optional
`detail`, and a `kind`:

| kind           | behaviour                                              |
|----------------|--------------------------------------------------------|
| `manual_check` | yes / no / skip confirmation (the default)             |
| `info`         | something to read and acknowledge (no pass/fail)       |
| `command`      | a suggested command shown in `detail` — *you* run it   |

See [`protocols/rust-repo-review.yaml`](protocols/rust-repo-review.yaml) for a
complete, commented example.

## Architecture

Single crate, library + thin binary (the suite's Workstate pattern):

```
src/
├── main.rs          # thin entry point: parse argv, dispatch, set exit code
├── lib.rs           # library root: Result alias + public re-exports
├── core/            # CLI-agnostic domain (no argument parsing here)
│   ├── error.rs     # ProtoError (thiserror)
│   ├── protocol.rs  # Protocol, Step, StepKind   (the recipe)
│   ├── session.rs   # Session, StepResult, StepStatus  (the run)
│   ├── loader.rs    # discover / load / validate / find
│   └── store.rs     # save / list / load sessions (the run's persistence)
└── cli/             # argument parsing (clap) + one file per command
    ├── mod.rs       # Cli/Command definitions + dispatch
    ├── list.rs
    ├── validate.rs
    ├── run.rs
    ├── sessions.rs
    └── show.rs
```

Data flows one way and through files: protocols are YAML *in*, sessions are JSON
*out*. Proto never imports another suite tool.

---

Part of the [Linux Ops Suite](https://github.com/tom2025b/linux-ops-suite).
Built for personal use. Keep it simple.
