# Changelog

All notable changes to the Linux Ops Suite umbrella are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
(pre-1.0: the MINOR slot carries both new features and breaking changes).

All crates in the workspace share one version (`[workspace.package] version`), so
a single entry covers the whole suite.

## [Unreleased]

## [0.2.0] - 2026-06-20

A large release: the shared TUI library landed and Pulse became its first full
adopter, several new tools graduated in, and a dependency-free foundation crate
was extracted and rolled across the suite.

### Added
- **thomas-tui** — a domain-free, general-purpose ratatui toolkit: `Theme` behind
  a single `NO_COLOR` gate, the rounded `pane`/`pane_titled`/`pane_blank` chrome,
  `centered_rect`/`centered_fixed`, display-width-aware `truncate_path`/
  `truncate_desc`, the `Tui` RAII terminal guard (panic-safe restore + `suspended`
  child hand-off), the one-line widgets (`SearchBar`, `KeyHints`, `StatusStrip`,
  `Counted`, `FilterChips`, `Freshness`, `EmptyState`) and the `HelpSheet`/
  `ConfirmModal`/`PaletteFrame` overlays. Includes the `Themed<W>`/`ThemedLine`
  opt-in `ratatui::Widget` surface.
- **suite-ui** — the suite-flavoured chrome over thomas-tui: `StatusBar`/
  `JobState`/`Outcome`, `SeverityBadge`, `HealthStrip`, `AttentionFlag`, and the
  job-lifecycle `Toast` kinds. Re-exports the whole toolkit so consumers import
  everything as `suite_ui::*`.
- **suite-core** — a dependency-free shared foundation crate (env / path / xdg /
  fmt helpers), the suite's one sanctioned non-TUI shared library.
- **conductor** — a new read-only suite driver: `SuiteState` model, the v1 rule
  engine, `status`/`health`/`plan` CLI, the Ring-1/Ring-2 spawn choke point with a
  confirm gate, and a TUI.
- **rewind** — content-addressed capture store with `capture`/`list`/`show`/`diff`
  (capture & live).
- **tripwire** — read-only file-integrity baseline + drift diff.
- **portman** — "what is listening, and why": socket/owner inventory.
- **rex-doctor** — suite diagnostics crate with `env.*` and `bin.*` check groups.
- **Pulse** — `--theme cyan|amber` and `--color auto|always|never` flags.

### Changed
- **Pulse migrated to suite-ui.** Its hand-rolled ANSI-string renderer and libc
  `termios` driver were replaced by ratatui + crossterm via suite-ui (~1300 net
  lines removed). Visible refinements: a `[CRIT]` severity badge, a glyph health
  strip, and the Help screen as a centered overlay. Interactive and headless
  (`--dump-view`/`--state`) paths now render the same chrome.
- **8 crates refactored onto suite-core** for env/path/xdg/fmt (pulse, conductor,
  rex-check, rex-doctor, portman, tripwire, rewind, the installer).
- thomas-tui truncation is now display-width-correct over grapheme clusters
  (CJK/emoji safe), measuring UAX#11 columns instead of `char` counts.
- Public enums in suite-ui/thomas-tui are `#[non_exhaustive]` with neutral
  forward-compat fallback arms.

### Fixed
- **portman [H1]** — a real service supersedes systemd as a socket's owner.
- **rex-doctor [H2]** — the writable check honors ownership via `access(2)`.
- **conductor [M1/M3/M5]** — the confirm modal matches the spawned argv; spawn
  errors are surfaced.
- **tripwire/portman [M2]** — a valid baseline is never truncated on a serialize
  error.
- **pulse/rewind [M4/M6]** — correct ScriptVault status; capture file size capped.
- **portman/tripwire [L4]** — control characters are escaped in the JSON envelope.
- **tripwire [L2/L3]** — content-toggle drift is reported; `#` kept in watched paths.
- **pulse [L6]** — an unknown severity escalates to High rather than sinking to Low.
- **rex-check** — auto-discovers umbrella crates in the totals table; the commit
  flow keeps its hazard gate without an unconditional confirmation.
- **installer** — hardened archive extraction and untrusted-asset handling.

### Notes
- Pulse's manual real-terminal smoke (live resize, cockpit round-trip,
  themes/`NO_COLOR`) is recommended; it is not covered by CI.

## [0.1.2] - earlier
## [0.1.1] - earlier
## [0.1.0] - earlier

Initial tagged releases; see the git history for details
(`git log v0.1.0`, `v0.1.1`, `v0.1.2`).

[Unreleased]: https://github.com/tom2025b/linux-ops-suite/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/tom2025b/linux-ops-suite/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/tom2025b/linux-ops-suite/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/tom2025b/linux-ops-suite/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/tom2025b/linux-ops-suite/releases/tag/v0.1.0
