# tripwire

Answers one question: *what changed on disk since I last looked?*

tripwire records a **baseline** — a SHA-256 content hash plus metadata (kind,
permissions, owner, size, mtime) for a watched set of files and directories —
and later **diffs** the live filesystem against it, reporting what was added,
removed, modified, or re-permissioned. Read-only; the only file it ever writes
is its own baseline.

It is the suite's filesystem-surface lens, the on-disk counterpart to
[portman](../portman)'s network-surface lens.

## Usage

```bash
# Current view: every watched path and its state now.
tripwire

# Show the resolved watch set and where it came from (cli / config / builtin).
tripwire watch

# Add the hash / owner / mtime columns.
tripwire --verbose

# Record the current state as a baseline to compare against.
tripwire baseline

# Show what changed since the recorded baseline.
tripwire diff

# Like diff, but silent when clean (for cron). Exits 1 on drift.
tripwire verify

# Machine-readable output (works on any command).
tripwire --json

# Watch specific paths instead of the config/built-in set (repeatable).
tripwire --path /etc/nginx --path ~/.ssh/authorized_keys diff

# Use a specific config or baseline file instead of the default XDG paths.
tripwire --config ./watch.conf --baseline-file ./tw.json diff
```

System files like `/etc/shadow` show as `unreadable` without root; `sudo
tripwire` records their content too.

## What it watches

With no `--path` and no config file, tripwire watches a built-in default set of
the files an operator most often wants to know changed — `/etc/passwd`,
`/etc/shadow`, `/etc/sudoers`, `/etc/ssh/sshd_config`, `/etc/crontab`,
`/etc/cron.d`, `~/.ssh/authorized_keys`, the login dotfiles, and a few more.
Run `tripwire watch` to see the exact set. Missing paths are skipped silently.

To customize, drop a `watch.conf` at
`$XDG_CONFIG_HOME/linux-ops-suite/tripwire/watch.conf` (or pass `--config`).
The format is one path per line, with optional `key=value` options:

```conf
# system
/etc/ssh/sshd_config
/etc/cron.d            recursive=true exclude=*.tmp
/var/log/app.log       content=false        # watch metadata only, don't hash
/srv/www               exclude=.git exclude=*.log
~/.ssh/authorized_keys
```

Options: `recursive` (dirs, default true), `follow_symlinks` (default false —
symlinks are recorded as symlinks, not followed), `content` (hash file
contents, default true), and repeatable `exclude` globs (`*`, `?`).

## Key features

- **Content + metadata drift.** A differing SHA-256 catches an edited file even
  if its mtime was reset; mode and owner changes are caught and flagged
  `[PERM]` / `[OWNER]` as security-relevant.
- **Baseline + diff.** Snapshot the expected state once, then `tripwire diff`
  flags anything added, removed, modified, or re-owned.
- **Cron-friendly.** `tripwire diff` (or the quieter `tripwire verify`) exits
  `1` when anything changed and `0` when clean, so it drops straight into cron
  or a CI check. Exit `3` means tripwire itself couldn't run.
- **Read-only and rootless.** Works without privileges (unreadable files are
  recorded as metadata-only); `sudo` completes the picture. A touched-but-
  identical file is never reported as drift.
- **Symlink-safe.** Symlinks are recorded as symlinks by default, so a swapped
  link target is caught as a change — never silently followed off the watch set.
- **JSON envelope** on every command for scripting and the suite's file
  contracts.

## How it fits into the suite

tripwire sits alongside portman (network surface) and Bulwark (script/risk)
under the same "read-only, single-purpose, file-contract" philosophy. Its
baseline/diff model makes the on-disk surface auditable over time, and its JSON
output is shaped like the other producers' so it can feed the suite's snapshot
pipeline. It is intentionally lean — `clap` + `serde` only; the SHA-256 and the
directory walk are hand-rolled (no `sha2`, `walkdir`, `notify`, or network).
