# Last Work

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
