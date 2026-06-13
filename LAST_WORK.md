# Last Work

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
