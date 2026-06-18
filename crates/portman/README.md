# portman

Answers one question: *what is listening on this machine, and why?*

portman enumerates every listening socket and resolves the full ownership chain behind it — socket → PID → process → systemd unit → package — then lets you save that picture as a baseline and diff against it later. Read-only; it never opens, closes, or changes a socket.

## Usage

```bash
# Current view: every listening socket and who owns it.
portman

# Add the exe + package columns.
portman --verbose

# Record the current listeners as a baseline to compare against.
portman baseline

# Show what changed since the recorded baseline.
portman diff

# Machine-readable output (works on any command).
portman --json

# Use a specific baseline file instead of the default XDG path.
portman --baseline-file ./ports.json diff
```

Owners of other users' sockets show as `?` without root; `sudo portman` fills in the rest.

## Key features

- **Full ownership chain.** Not just `pid/program` like `ss` — portman walks socket → PID → process → systemd unit → package, so you can tell *why* a port is open.
- **Baseline + diff.** Snapshot the expected listeners once, then `portman diff` flags anything new, missing, or re-owned.
- **Tripwire-friendly.** `portman diff` exits `1` when anything changed (and `0` when clean), so it drops straight into cron or a CI check. Exit `3` means portman itself couldn't run.
- **Read-only and rootless.** Works without privileges (unknown owners show as `?`); `sudo` completes the picture.
- **JSON envelope** on every command for scripting and the suite's file contracts.

## How it fits into the suite

portman is the suite's network-surface lens, sitting alongside Bulwark (filesystem/script risk) under the same "read-only, single-purpose, file-contract" philosophy. Its baseline/diff model makes the listening surface auditable over time, and its JSON output is shaped to feed the suite's snapshot pipeline the same way the other producers do.
