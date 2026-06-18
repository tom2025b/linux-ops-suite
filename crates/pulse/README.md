# pulse

A calm, read-only status instrument for the Linux Ops Suite. It opens to a single verdict on a near-empty screen and answers one question first: *is the suite healthy right now?*

Pulse is not RexOps with another skin. RexOps is the launcher and orchestrator; pulse is the quiet overview — the place to see whether anything deserves attention before deciding where to work. It only ever reads the suite's file contracts; it never mutates feeds or repairs state.

## Usage

```bash
# Live verdict. In a real terminal this opens the interactive view.
pulse

# Force a demo state (no live data needed): healthy | attention | incomplete.
pulse --state attention

# Read feeds from a specific directory instead of the default data dir.
pulse --data-dir ./feeds          # same as setting $PULSE_DATA_DIR

# Render one view once and exit — no event loop, greppable / CI-friendly.
pulse --dump-view attention
pulse --dump-view search aws

# Print a frame without clearing the screen first (useful when piping).
pulse --no-clear
```

### Keys (interactive)

```text
Enter  details      a  attention      f  feeds
/      search       ?  help           q / Esc  quit
```

## Key features

- **One verdict, three states.** `all clear` (healthy), `NEEDS ATTENTION`, or `INCOMPLETE` — the focal point on every screen.
- **Calm by design.** Healthy is the emptiest screen in the suite: just the verdict and a dim timestamp. Detail fills in from the center outward only when something needs it; the verdict never changes position between states.
- **Source confidence.** A `● current  ◐ stale  ○ missing` strip shows which producers are fresh, so a verdict built on stale or absent feeds is marked `INCOMPLETE` rather than falsely "clear."
- **Legible without color.** State is carried by the verdict word and marker shape, not color alone — honors `NO_COLOR` and non-TTY output.
- **Zero dependencies.** Renders its own ANSI and reads terminal size via tiny `libc` calls; std is all it needs.

## How it fits into the suite

Pulse is a passive reader at the end of the contract pipeline. It loads the suite's file contracts under `$XDG_DATA_HOME` — the Workstate snapshot plus the Bulwark / Proto / ToolFoundry feeds — and rolls them into a single suite verdict. Where RexOps consumes the snapshot to *act* (launch tools, run a refresh), pulse consumes the same data to *observe*: a glanceable health check that respects the suite's one-way, read-only-by-default data flow.
