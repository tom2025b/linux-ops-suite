# suite-ui Review Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the defects found in the suite-ui design review — the truncation
display-width bug (R1), the inconsistent widget API (R2), test gaps (R3), the
JobState/Outcome redundancy (R4) — and refresh the docs, all while keeping the
crate strictly stateless chrome.

**Architecture:** Two crates in one workspace. `thomas-tui` is the domain-free
toolkit (theme, panes, layout, truncation, the `Tui` guard, the one-line/overlay
widgets). `suite-ui` is the thin suite-flavoured shim that re-exports all of
`thomas-tui` and adds the suite-semantic widgets (`StatusBar`/`Outcome`,
`SeverityBadge`, `AttentionFlag`, `HealthStrip`, the job `Toast` kinds). Every
widget is a struct of *borrowed* display values; none owns state, captures input,
or reads the environment. We do not change that contract — we make it consistent,
correct, and documented.

**Tech Stack:** Rust (edition 2021), `ratatui 0.29`, `crossterm 0.28`,
`unicode-width`, `unicode-segmentation`, `insta` (dev-dep), `clap` (optional).

## Global Constraints

- **Edition `2021`, `rust-version = "1.85"`, workspace `version = "0.1.2"`** — copied
  from the root `Cargo.toml`; do not bump.
- **Stateless contract is inviolable:** no widget gains owned state, input capture,
  async, or an env read. New types borrow; they own nothing.
- **`NO_COLOR` guarantee:** every style still routes through `Theme`; colour-off must
  drop *all* foreground colour and keep only attributes/glyphs. Never assert a
  foreground under `NO_COLOR`.
- **`#[non_exhaustive]` stays** on every public enum; keep the
  `#[allow(unreachable_patterns)]` neutral fallback arms.
- **Workspace dep hygiene:** add shared deps to `[workspace.dependencies]` in the
  root `Cargo.toml` and reference them as `{ workspace = true }` from crate manifests
  (matches the existing `ratatui`/`crossterm` pattern). `unicode-width` and
  `unicode-segmentation` are already in `Cargo.lock` (transitive via ratatui), so no
  new network fetch.
- **Green gate every task:** `cargo test -p thomas-tui -p suite-ui`,
  `cargo clippy -p thomas-tui -p suite-ui --all-targets -- -D warnings`, and
  `cargo fmt --check` must pass before each commit. The example gallery
  (`cargo run -p suite-ui --example gallery`) must still run.
- **Baseline (pre-work, verified 2026-06-20):** thomas-tui 86 tests, suite-ui 5 + 13
  tests — all green. Do not regress the count.
- **API-break policy:** none. Per approval, R4 is the **additive** variant — it adds
  `JobState::outcome()` and keeps `Done{ok}`/`Cancelled` unchanged. Every task in this
  plan is additive or behaviour-preserving; no consumer migration is required.
- **No wrappers / no new CLI tool:** this is a library change only — no `~/bin/r-*`
  scripts, no aliases, no binaries.

---

## File Structure

Files created or modified, by responsibility:

- `Cargo.toml` (root) — add `unicode-width`, `unicode-segmentation`, `insta` to
  `[workspace.dependencies]`.
- `crates/thomas-tui/Cargo.toml` — depend on the two unicode crates; add `insta` as
  a dev-dependency.
- `crates/thomas-tui/src/text.rs` — **R1**: rewrite `truncate_path`/`truncate_desc`
  to measure display columns; rewrite docs and tests.
- `crates/thomas-tui/src/widget.rs` — **R2 (new file)**: the `ChromeWidget` marker +
  `Themed<W>` wrapper + the `Widget for &Themed<W>` impls and the `.themed(theme)`
  ext-trait. One focused file; the widgets themselves are not moved.
- `crates/thomas-tui/src/lib.rs` — **R2/docs**: export `Themed`/`Themed` ext-trait;
  add the "Widget API contract" doc section; headline the two-layer architecture.
- `crates/suite-ui/Cargo.toml` — add `insta` dev-dep (for suite-ui widget snapshots).
- `crates/suite-ui/src/widget.rs` — **R2 (new file)**: `Widget for &Themed<W>` impls
  for the suite-ui widgets (`StatusBar`, `AttentionFlag`, `HealthStrip`).
- `crates/suite-ui/src/status_bar.rs` — **R4**: collapse `JobState::Done{ok}` +
  `Cancelled` into `Finished{outcome}`; update `line()` and tests.
- `crates/suite-ui/src/lib.rs` — **R2/R4/docs**: re-export `Themed`; update the
  crate-doc component list and the two-layer note.
- `crates/suite-ui/examples/gallery.rs` — **R4**: update `JobState` call-sites.
- `crates/thomas-tui/tests/snapshots.rs` + `crates/suite-ui/tests/snapshots.rs` —
  **R3 (new files)**: `insta` geometry snapshots. Snapshot fixtures land under
  `crates/*/tests/snapshots/`.
- `docs/design/suite-ui/SUITE_UI_DESIGN.md` — **docs**: flip the resolved items from
  "proposed" to "done"; keep §10 Future Evolution as-is.

---

## Task 1: Add the unicode + insta dependencies

**Files:**
- Modify: `Cargo.toml` (root, `[workspace.dependencies]`)
- Modify: `crates/thomas-tui/Cargo.toml`
- Modify: `crates/suite-ui/Cargo.toml`

**Interfaces:**
- Produces: `unicode_width::{UnicodeWidthStr, UnicodeWidthChar}`,
  `unicode_segmentation::UnicodeSegmentation`, and `insta` (dev) available to both
  crates. No code yet — this is the dependency seam Tasks 2/3/6 consume.

- [ ] **Step 1: Add the shared deps to the workspace**

In root `Cargo.toml`, under `[workspace.dependencies]`, add:

```toml
unicode-width = "0.2"
unicode-segmentation = "1"
insta = "1"
```

- [ ] **Step 2: Wire them into thomas-tui**

In `crates/thomas-tui/Cargo.toml`, under `[dependencies]`, add:

```toml
unicode-width = { workspace = true }
unicode-segmentation = { workspace = true }
```

and add a dev-deps section (or extend it):

```toml
[dev-dependencies]
insta = { workspace = true }
```

- [ ] **Step 3: Wire insta into suite-ui**

In `crates/suite-ui/Cargo.toml`, add:

```toml
[dev-dependencies]
insta = { workspace = true }
```

- [ ] **Step 4: Verify it resolves without a new fetch**

Run: `cargo build -p thomas-tui -p suite-ui`
Expected: builds; `unicode-width`/`unicode-segmentation` resolve from the existing
lock (already transitive via ratatui), `insta` is fetched once.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/thomas-tui/Cargo.toml crates/suite-ui/Cargo.toml
git commit -m "build(suite-ui): add unicode-width, unicode-segmentation, insta deps"
```

**Acceptance:** Both crates build; the three deps are declared workspace-wide and
referenced via `{ workspace = true }`; no other behaviour changed.

---

## Task 2: R1 — make truncation display-width-correct (failing tests first)

**Files:**
- Test: `crates/thomas-tui/src/text.rs` (the `#[cfg(test)] mod tests`)
- Modify: `crates/thomas-tui/src/text.rs`

**Interfaces:**
- Consumes: `unicode_width::UnicodeWidthStr`, `unicode_segmentation::UnicodeSegmentation`.
- Produces: unchanged signatures `pub fn truncate_path(s: &str, max: usize) -> String`
  and `pub fn truncate_desc(s: &str, max: usize) -> String`, with the post-condition
  changed from "exactly `max` **chars**" to "**≤ `max` display columns**". Later tasks
  and consumers call these unchanged.

- [ ] **Step 1: Write the failing wide-character tests**

Add to `crates/thomas-tui/src/text.rs` tests module. These FAIL against the current
char-counting implementation:

```rust
use unicode_width::UnicodeWidthStr;

#[test]
fn truncate_path_respects_display_columns_not_char_count() {
    // 日本語 = 3 chars but 6 columns. A 10-column budget must NOT emit a string
    // wider than 10 columns (char-counting would wrongly keep ~9 wide chars).
    let wide = "/srv/日本語/データ/script.sh";
    let out = truncate_path(wide, 10);
    assert!(out.width() <= 10, "got {:?} = {} cols", out, out.width());
    assert!(out.starts_with('…'));
}

#[test]
fn truncate_desc_respects_display_columns_for_emoji() {
    // Each 🚀 is 1 char but 2 columns. 8-column budget → at most 8 columns.
    let out = truncate_desc("🚀🚀🚀🚀🚀🚀 launch", 8);
    assert!(out.width() <= 8, "got {:?} = {} cols", out, out.width());
    assert!(out.ends_with('…'));
}

#[test]
fn truncate_keeps_combining_marks_with_their_base() {
    // "é" as e + U+0301 is 2 chars but 1 column. A grapheme-aware truncation must
    // not slice between the base and the mark, and must measure it as 1 column.
    let s = "cafe\u{0301} test description here that overflows";
    let out = truncate_desc(s, 6);
    assert!(out.width() <= 6);
    // The combining mark must never be orphaned at the cut.
    assert!(!out.starts_with('\u{0301}') && !out.ends_with('\u{0301}'));
}

#[test]
fn truncate_path_never_exceeds_budget_for_zwj_sequence() {
    // A ZWJ family emoji is several chars and 2 columns; must be kept or dropped
    // whole, never split, and never overflow the column budget.
    let s = "/x/👨‍👩‍👧/deeply/nested/file.sh";
    let out = truncate_path(s, 12);
    assert!(out.width() <= 12, "got {:?} = {} cols", out, out.width());
}
```

- [ ] **Step 2: Run them to confirm they fail**

Run: `cargo test -p thomas-tui text:: -- --nocapture`
Expected: the four new tests FAIL (the current impl keeps `max` *chars*, overflowing
columns for wide input).

- [ ] **Step 3: Rewrite `truncate_path` to measure columns**

Replace `truncate_path` in `crates/thomas-tui/src/text.rs`:

```rust
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Truncate `s` to at most `max` display COLUMNS, keeping the **end** and marking
/// the cut with a leading ellipsis. For paths, where the filename (the tail) is
/// the part worth keeping.
///
/// Display-width-aware (UAX#11) and grapheme-safe: a 2-column CJK ideograph or
/// emoji counts as 2, a zero-width combining mark as 0, and a multi-codepoint
/// grapheme (ZWJ emoji, base+combining) is kept or dropped whole, never split.
/// Input that already fits in `max` columns is returned unchanged; a longer input
/// yields the ellipsis plus a tail of at most `max - 1` columns (so the result is
/// `≤ max` columns — it may be one short when a 2-column glyph straddles the edge).
/// `max == 0` yields an empty string.
///
/// ```
/// use thomas_tui::truncate_path;
/// assert_eq!(truncate_path("/tmp/tool.sh", 48), "/tmp/tool.sh");
/// let out = truncate_path("/very/deeply/nested/dir/backup-tool.sh", 20);
/// assert!(out.starts_with('…'));
/// assert!(out.ends_with("backup-tool.sh"));
/// ```
pub fn truncate_path(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let budget = max - 1; // reserve one column for the leading ellipsis
    let mut kept: Vec<&str> = Vec::new();
    let mut used = 0usize;
    for g in s.graphemes(true).rev() {
        let w = g.width();
        if used + w > budget {
            break;
        }
        used += w;
        kept.push(g);
    }
    kept.reverse();
    format!("{ELLIPSIS}{}", kept.concat())
}
```

- [ ] **Step 4: Rewrite `truncate_desc` to measure columns**

Replace `truncate_desc`:

```rust
/// Truncate `s` to at most `max` display COLUMNS, keeping the **start** and marking
/// the cut with a trailing ellipsis. For descriptions and other read-left-to-right
/// text. Leading/trailing whitespace is trimmed first.
///
/// Display-width-aware and grapheme-safe, same as [`truncate_path`]. Input that
/// already fits in `max` columns (after trimming) is returned as-is; a longer input
/// is a head of at most `max - 1` columns plus the ellipsis (`≤ max` columns total).
/// `max == 0` yields an empty string.
///
/// ```
/// use thomas_tui::truncate_desc;
/// assert_eq!(truncate_desc("  backs up the NAS  ", 40), "backs up the NAS");
/// let out = truncate_desc("this description is definitely too long to fit", 20);
/// assert!(out.ends_with('…'));
/// ```
pub fn truncate_desc(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.width() <= max {
        return trimmed.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let budget = max - 1; // reserve one column for the trailing ellipsis
    let mut kept: Vec<&str> = Vec::new();
    let mut used = 0usize;
    for g in trimmed.graphemes(true) {
        let w = g.width();
        if used + w > budget {
            break;
        }
        used += w;
        kept.push(g);
    }
    format!("{}{ELLIPSIS}", kept.concat())
}
```

- [ ] **Step 5: Fix the existing tests' post-condition (chars → columns)**

The legacy tests assert `out.chars().count() == 20` / `== 18` / `== 10`. Those
post-conditions are wrong for the new contract. Update them to width assertions:

In `short_input_is_returned_unchanged` — unchanged (still correct).
In `truncate_path_keeps_the_tail_and_hits_max_exactly` — rename and relax:

```rust
#[test]
fn truncate_path_keeps_the_tail_within_the_column_budget() {
    let long = "/very/deeply/nested/directory/structure/backup-tool.sh";
    let out = truncate_path(long, 20);
    assert!(out.starts_with('…'), "leading ellipsis marks the cut");
    assert!(out.ends_with("backup-tool.sh"), "the filename tail is kept");
    assert!(out.width() <= 20, "stays within the column budget");
}
```

In `truncate_desc_keeps_the_head_trims_and_hits_max_exactly` — same treatment:

```rust
#[test]
fn truncate_desc_keeps_the_head_trims_within_the_column_budget() {
    let out = truncate_desc("this description is definitely too long to fit", 20);
    assert!(out.ends_with('…'), "trailing ellipsis marks the cut");
    assert!(out.starts_with("this description"), "the head is kept");
    assert!(out.width() <= 20, "stays within the column budget");
    assert_eq!(truncate_desc("   hello   ", 40), "hello");
}
```

In `unicode_boundaries_are_never_split` — replace the `chars().count()` asserts with
`width()` asserts (the ASCII-ellipsis check and short-input checks stay):

```rust
#[test]
fn unicode_boundaries_are_never_split() {
    let path = "/tmp/café-résumé/être/naïve/tool.sh";
    let out = truncate_path(path, 18);
    assert!(out.width() <= 18);
    assert!(out.starts_with('…'));
    let desc = "ééééééééééééééééééééééééé";
    let out = truncate_desc(desc, 10);
    assert!(out.width() <= 10);
    assert!(out.ends_with('…'));
    assert_eq!(truncate_desc("café ☕", 40), "café ☕");
    assert_eq!(truncate_path("/tmp/é", 48), "/tmp/é");
}
```

In `tiny_widths_saturate_and_never_panic` — `max == 1` now yields just the ellipsis
(width 1), which still holds; keep it. The doctest assertions in Steps 3/4 dropped the
`chars().count()` lines, so no doctest fixups remain.

- [ ] **Step 6: Run the full text module + doctests**

Run: `cargo test -p thomas-tui text::`
Then: `cargo test -p thomas-tui --doc`
Expected: PASS — the four new wide-char tests, the updated legacy tests, and the
doctests all green.

- [ ] **Step 7: Clippy + fmt + commit**

Run: `cargo clippy -p thomas-tui --all-targets -- -D warnings && cargo fmt`

```bash
git add crates/thomas-tui/src/text.rs
git commit -m "fix(thomas-tui): truncate by display columns, not char count [R1]

Wide CJK/emoji are 1 char but 2 columns; the old char-counted truncation
overflowed the cell budget and corrupted table layout. Measure UAX#11 display
width over grapheme clusters (unicode-width + unicode-segmentation), the same
crates ratatui uses internally. Post-condition is now <= max columns."
```

**Acceptance:** `truncate_*` never return a string wider than `max` columns for any
input (CJK, emoji, ZWJ, combining marks); graphemes are never split; the module-doc
says "columns" and no longer claims a misleading "Unicode-safe / char count"
guarantee; all thomas-tui tests + doctests green.

---

## Task 3: R2a — the `Themed<W>` wrapper + `ChromeWidget` contract (thomas-tui)

**Files:**
- Create: `crates/thomas-tui/src/widget.rs`
- Modify: `crates/thomas-tui/src/lib.rs` (add `mod widget;`, re-exports, doc section)
- Test: `crates/thomas-tui/src/widget.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: the existing thomas-tui one-line widgets that already expose
  `fn line(&self, theme: Theme) -> Line<'static>`: `SearchBar`, `StatusStrip`,
  `FilterChips`, `KeyHints`, `Freshness`. (`Counted`/`SeverityBadge` are span-only
  by design and are out of scope for the line-rendering wrapper.)
- Produces:
  - `pub trait Themable: Sized { fn themed(self, theme: Theme) -> Themed<Self>; }`
    with a blanket impl, so every wrapped widget gets `.themed(theme)`.
  - `pub struct Themed<W> { pub widget: W, pub theme: Theme }`.
  - `impl ratatui::widgets::Widget for &Themed<W>` for each line-widget `W`, rendering
    `Paragraph::new(self.widget.line(self.theme))` into the area.
  These let `frame.render_widget(&bar.themed(theme), area)` and nesting inside
  ecosystem `Widget` containers, with zero added state.

- [ ] **Step 1: Write the failing test (the wrapper renders identically to `.render`)**

Create `crates/thomas-tui/src/widget.rs`:

```rust
//! `Themed<W>`: bind a stateless chrome widget to a `Theme` so it implements
//! `ratatui::Widget` (render-by-reference). This is the *opt-in* ecosystem surface:
//! the existing inherent `.line(theme)` / `.render(frame, area, theme)` methods stay.
//!
//! The fixed `Widget::render(self, Rect, &mut Buffer)` signature has no `theme`
//! parameter, so the theme rides inside `Themed`. The wrapper borrows and owns no
//! application state — the contract is unchanged.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};

use crate::theme::Theme;

/// A chrome widget bound to a theme, so it can render as a `ratatui::Widget`.
/// Construct via [`Themable::themed`].
pub struct Themed<W> {
    pub widget: W,
    pub theme: Theme,
}

/// Bind any line-producing chrome widget to a theme with `.themed(theme)`.
pub trait Themable: Sized {
    /// Wrap `self` with `theme` so `&Themed<Self>` is a `ratatui::Widget`.
    fn themed(self, theme: Theme) -> Themed<Self> {
        Themed { widget: self, theme }
    }
}

impl<W> Themable for W {}

/// Internal: render any widget whose theme-bound `Line` we can produce.
fn render_line(line: Line<'static>, area: Rect, buf: &mut Buffer) {
    Paragraph::new(line).render(area, buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SearchBar;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// Flatten row 0 of a buffer to a string.
    fn row0(buf: &Buffer) -> String {
        (0..buf.area.width)
            .map(|x| buf.cell((x, 0)).unwrap().symbol().to_string())
            .collect()
    }

    #[test]
    fn themed_searchbar_renders_via_the_widget_trait() {
        let theme = Theme::with_color(true);
        let bar = SearchBar { query: "bul", placeholder: "ph", match_count: Some(2) };
        let mut term = Terminal::new(TestBackend::new(30, 1)).unwrap();
        term.draw(|f| f.render_widget(&bar.themed(theme), f.area())).unwrap();
        let got = row0(term.backend().buffer());
        assert!(got.starts_with("/ "), "prompt glyph rendered: {got:?}");
        assert!(got.contains("bul"), "query rendered: {got:?}");
    }

    #[test]
    fn themed_matches_inherent_render() {
        // The Widget impl must produce the same buffer as calling .render() directly.
        let theme = Theme::with_color(false);
        let bar = SearchBar { query: "q", placeholder: "ph", match_count: None };

        let mut a = Terminal::new(TestBackend::new(20, 1)).unwrap();
        a.draw(|f| f.render_widget(&bar.themed(theme), f.area())).unwrap();

        let bar2 = SearchBar { query: "q", placeholder: "ph", match_count: None };
        let mut b = Terminal::new(TestBackend::new(20, 1)).unwrap();
        b.draw(|f| bar2.render(f, f.area(), theme)).unwrap();

        assert_eq!(a.backend().buffer(), b.backend().buffer());
    }
}
```

- [ ] **Step 2: Run it — fails to compile (no `Widget for &Themed`)**

Run: `cargo test -p thomas-tui widget::`
Expected: FAIL — `&Themed<SearchBar>` does not implement `Widget` yet.

- [ ] **Step 3: Add the `Widget for &Themed<W>` impls**

In `crates/thomas-tui/src/widget.rs`, above the tests, add one impl per line-widget.
Each delegates to the widget's existing `.line(theme)`:

```rust
use crate::{FilterChips, Freshness, KeyHints, SearchBar, StatusStrip};

impl Widget for &Themed<SearchBar<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_line(self.widget.line(self.theme), area, buf);
    }
}
impl Widget for &Themed<StatusStrip<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_line(self.widget.line(self.theme), area, buf);
    }
}
impl Widget for &Themed<FilterChips<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_line(self.widget.line(self.theme), area, buf);
    }
}
impl Widget for &Themed<KeyHints<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_line(self.widget.line(self.theme), area, buf);
    }
}
impl Widget for &Themed<Freshness> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        render_line(self.widget.line(self.theme), area, buf);
    }
}
```

- [ ] **Step 4: Export the wrapper from the crate**

In `crates/thomas-tui/src/lib.rs`, add `mod widget;` near the other module decls and
extend the public re-export block:

```rust
pub use widget::{Themable, Themed};
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p thomas-tui widget::`
Expected: PASS — both tests; the `themed_matches_inherent_render` buffer equality
proves the wrapper is behaviour-preserving.

- [ ] **Step 6: Clippy + fmt + commit**

Run: `cargo clippy -p thomas-tui --all-targets -- -D warnings && cargo fmt`

```bash
git add crates/thomas-tui/src/widget.rs crates/thomas-tui/src/lib.rs
git commit -m "feat(thomas-tui): add Themed<W> ratatui::Widget surface [R2]

Opt-in: line-producing chrome widgets gain .themed(theme) -> Themed<W>, and
&Themed<W> implements ratatui::Widget, so they compose with frame.render_widget
and ecosystem Widget containers. Existing inherent .line/.render methods stay.
No state added — Themed borrows and owns nothing."
```

**Acceptance:** `frame.render_widget(&w.themed(theme), area)` works for `SearchBar`,
`StatusStrip`, `FilterChips`, `KeyHints`, `Freshness`; the wrapper's output is
byte-identical to the inherent `.render`; `Themable`/`Themed` are public; no state
introduced.

---

## Task 4: R2b — `Widget for &Themed<W>` for the suite-ui widgets

**Files:**
- Create: `crates/suite-ui/src/widget.rs`
- Modify: `crates/suite-ui/src/lib.rs` (add `mod widget;`, re-export `Themed`/`Themable`)
- Test: `crates/suite-ui/src/widget.rs`

**Interfaces:**
- Consumes: `thomas_tui::{Themed, Themable}` (Task 3); the suite-ui line-widgets
  `StatusBar`, `AttentionFlag`, `HealthStrip` (all expose `fn line(&self, Theme) -> Line`).
- Produces: `impl Widget for &Themed<StatusBar<'_>>` (and `AttentionFlag`,
  `HealthStrip`); `Themed`/`Themable` re-exported as `suite_ui::{Themed, Themable}`.

- [ ] **Step 1: Write the failing test**

Create `crates/suite-ui/src/widget.rs`:

```rust
//! `Widget for &Themed<W>` impls for the suite-flavoured line widgets, so they
//! compose the same way the thomas-tui widgets do. `Themed`/`Themable` themselves
//! come from thomas-tui; this file only adds the suite-ui widget impls.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Widget};
use thomas_tui::Themed;

use crate::{AttentionFlag, HealthStrip, StatusBar};

impl Widget for &Themed<StatusBar<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.widget.line(self.theme)).render(area, buf);
    }
}
impl Widget for &Themed<AttentionFlag<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.widget.line(self.theme)).render(area, buf);
    }
}
impl Widget for &Themed<HealthStrip<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.widget.line(self.theme)).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobState, Theme};
    use thomas_tui::Themable;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn themed_status_bar_renders_via_widget_trait() {
        let theme = Theme::with_color(false);
        let bar = StatusBar { job: JobState::Running { name: "backup" } };
        let mut term = Terminal::new(TestBackend::new(30, 1)).unwrap();
        term.draw(|f| f.render_widget(&bar.themed(theme), f.area())).unwrap();
        let row: String = (0..30)
            .map(|x| term.backend().buffer().cell((x, 0)).unwrap().symbol().to_string())
            .collect();
        assert!(row.contains("running backup"), "got {row:?}");
    }
}
```

> Note: this test's `JobState::Running` call-site survives the Task 8 refactor
> unchanged (only `Done`/`Cancelled` change), so the tasks don't collide.

- [ ] **Step 2: Run it — fails to compile (no `mod widget`)**

Run: `cargo test -p suite-ui widget::`
Expected: FAIL — module not declared / impls not in scope.

- [ ] **Step 3: Declare the module and re-export the wrapper**

In `crates/suite-ui/src/lib.rs`, add `mod widget;` and extend the re-exports so
consumers can write `suite_ui::Themed` / `suite_ui::Themable`:

```rust
pub use thomas_tui::{Themable, Themed};
```

(Add to the existing `pub use thomas_tui::{ ... }` block.)

- [ ] **Step 4: Run the test**

Run: `cargo test -p suite-ui widget::`
Expected: PASS.

- [ ] **Step 5: Clippy + fmt + commit**

Run: `cargo clippy -p suite-ui --all-targets -- -D warnings && cargo fmt`

```bash
git add crates/suite-ui/src/widget.rs crates/suite-ui/src/lib.rs
git commit -m "feat(suite-ui): Themed<W> Widget impls for StatusBar/AttentionFlag/HealthStrip [R2]"
```

**Acceptance:** the three suite-ui line widgets render via
`frame.render_widget(&w.themed(theme), area)`; `suite_ui::{Themed, Themable}` are
public; no state added.

---

## Task 5: R2c — document the Widget API contract

**Files:**
- Modify: `crates/thomas-tui/src/lib.rs` (crate-doc)
- Modify: `crates/suite-ui/src/lib.rs` (crate-doc)

**Interfaces:**
- Consumes: nothing (docs only). Produces: a written contract every future widget
  must follow.

- [ ] **Step 1: Add the contract section to thomas-tui's crate-doc**

In `crates/thomas-tui/src/lib.rs`, add to the top-level `//!` docs:

```rust
//! ## Widget API contract
//!
//! Every widget is a struct of **borrowed display values**. None captures input,
//! owns application state, or reads the environment. The render surface follows one
//! rule by shape:
//!
//! - **Single span** (a count, a badge): `fn span(self, theme: Theme) -> Span` and
//!   `fn line(self, theme: Theme) -> Line` — e.g. `Counted`.
//! - **Single line** (a strip, a bar, a hint row): `fn line(&self, theme: Theme) ->
//!   Line` **and** `fn render(&self, frame, area, theme)`, and it composes as a
//!   `ratatui::Widget` via `.themed(theme)` (see [`Themed`]).
//! - **A framed/multi-line region** (an overlay, a placeholder): `fn render(&self,
//!   frame, area, theme)` only — it owns its layout, so there is no single `Line`.
//!
//! New widgets MUST pick the matching shape; do not invent a fourth.
```

- [ ] **Step 2: Cross-reference it from suite-ui's crate-doc**

In `crates/suite-ui/src/lib.rs`, under the existing "Scope: chrome, not logic"
section, add:

```rust
//! suite-ui's widgets follow the same Widget API contract as `thomas-tui` (see its
//! crate docs): borrowed values only, `span`/`line`/`render` by shape, and
//! `.themed(theme)` for the `ratatui::Widget` surface.
```

- [ ] **Step 3: Verify docs build**

Run: `cargo doc -p thomas-tui -p suite-ui --no-deps`
Expected: builds with no broken-intra-doc-link warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/thomas-tui/src/lib.rs crates/suite-ui/src/lib.rs
git commit -m "docs(suite-ui): document the Widget API contract (span/line/render by shape) [R2]"
```

**Acceptance:** both crate-docs state the contract; `cargo doc` is clean; the
`[`Themed`]` intra-doc link resolves.

---

## Task 6: R3 — insta geometry snapshots (thomas-tui)

**Files:**
- Create: `crates/thomas-tui/tests/snapshots.rs`
- Create (generated): `crates/thomas-tui/tests/snapshots/*.snap`

**Interfaces:**
- Consumes: `truncate_path`/`truncate_desc` (Task 2), `pane`/`pane_titled`,
  `centered_rect`, `TestBackend`. Produces: committed `.snap` fixtures that lock
  geometry/layout so a regression in column math or pane framing fails loudly.

- [ ] **Step 1: Write the snapshot test (renders into TestBackend, snapshots the buffer)**

Create `crates/thomas-tui/tests/snapshots.rs`:

```rust
//! insta geometry snapshots: render chrome into a fixed TestBackend and snapshot the
//! glyph grid. Snapshots capture LAYOUT ONLY (insta buffer Display ignores colour);
//! NO_COLOR/accent guarantees stay covered by the in-crate style-assertion tests.

use ratatui::backend::TestBackend;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use thomas_tui::{pane, truncate_path, Theme};

/// Render a closure into a `w`×`h` TestBackend and return the buffer for snapshotting.
fn render<F: FnOnce(&mut ratatui::Frame)>(w: u16, h: u16, f: F) -> ratatui::buffer::Buffer {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(f).unwrap();
    term.backend().buffer().clone()
}

#[test]
fn snapshot_pane_frames_a_title() {
    let theme = Theme::with_color(true);
    let buf = render(24, 4, |f| f.render_widget(pane("adapters", theme), f.area()));
    insta::assert_snapshot!(buf);
}

#[test]
fn snapshot_truncated_cjk_path_fits_a_narrow_pane() {
    // The R1 regression net: a wide-CJK path truncated to the pane's inner width
    // must not bleed past the right border. Snapshot the whole framed result.
    let theme = Theme::with_color(false);
    let buf = render(16, 3, |f| {
        let block = pane("p", theme);
        let inner = block.inner(f.area());
        f.render_widget(block, f.area());
        let text = truncate_path("/srv/日本語/データ/script.sh", inner.width as usize);
        f.render_widget(Paragraph::new(text), inner);
    });
    insta::assert_snapshot!(buf);
}
```

- [ ] **Step 2: Run with snapshot acceptance**

Run: `INSTA_UPDATE=always cargo test -p thomas-tui --test snapshots`
Expected: PASS, writing `crates/thomas-tui/tests/snapshots/*.snap`. Open the
`snapshot_truncated_cjk_path_fits_a_narrow_pane.snap` and eyeball it: the right
border `│` column must be intact, the CJK text clipped with a leading `…`.

- [ ] **Step 3: Re-run without update to confirm determinism**

Run: `cargo test -p thomas-tui --test snapshots`
Expected: PASS against the committed snapshots (no diff).

- [ ] **Step 4: Commit the test + fixtures**

Run: `cargo fmt`

```bash
git add crates/thomas-tui/tests/snapshots.rs crates/thomas-tui/tests/snapshots/
git commit -m "test(thomas-tui): insta geometry snapshots for pane + CJK truncation [R3]"
```

**Acceptance:** snapshots committed and deterministic; the CJK-in-pane snapshot shows
an intact right border (proving the R1 fix at the layout level); re-running without
`INSTA_UPDATE` is green.

---

## Task 7: R3 — insta snapshot for a suite-ui composite

**Files:**
- Create: `crates/suite-ui/tests/snapshots.rs`
- Create (generated): `crates/suite-ui/tests/snapshots/*.snap`

**Interfaces:**
- Consumes: `StatusBar`/`JobState` (existing `Done{ok}` shape — unchanged by Task 8),
  `TestBackend`. Produces: a committed composite snapshot. No dependency on Task 8.

- [ ] **Step 1: Write the composite snapshot test**

Create `crates/suite-ui/tests/snapshots.rs`:

```rust
//! insta geometry snapshot for a suite-ui status footer. Layout only (colour is not
//! captured); the per-state style/glyph guarantees live in status_bar.rs unit tests.

use ratatui::backend::TestBackend;
use ratatui::Terminal;
use suite_ui::{JobState, StatusBar, Theme};

#[test]
fn snapshot_status_bar_done_ok() {
    let theme = Theme::with_color(false);
    let bar = StatusBar {
        job: JobState::Done { name: "backup", ok: true },
    };
    let mut term = Terminal::new(TestBackend::new(30, 1)).unwrap();
    term.draw(|f| bar.render(f, f.area(), theme)).unwrap();
    insta::assert_snapshot!(term.backend().buffer().clone());
}
```

- [ ] **Step 2: Accept the snapshot**

Run: `INSTA_UPDATE=always cargo test -p suite-ui --test snapshots`
Expected: PASS; `.snap` written showing `✓ backup — done` (glyph + text, no colour).

- [ ] **Step 3: Re-run deterministically**

Run: `cargo test -p suite-ui --test snapshots`
Expected: PASS, no diff.

- [ ] **Step 4: Commit**

```bash
git add crates/suite-ui/tests/snapshots.rs crates/suite-ui/tests/snapshots/
git commit -m "test(suite-ui): insta snapshot for the status footer [R3]"
```

**Acceptance:** committed, deterministic; uses the unchanged `JobState::Done` API.

---

## Task 8: R4 — add `JobState::outcome()` (additive, non-breaking)

> **CHANGED per approval:** keep `JobState::Done{ok}` and `JobState::Cancelled`
> exactly as they are. Do **not** collapse the enum. Add a single derived accessor
> that names the `JobState → Outcome` mapping so it isn't re-inlined, giving callers
> the shared `Outcome` vocabulary without an API break. `line()` is refactored to use
> the new accessor internally (behaviour-preserving), not rewritten.

**Files:**
- Modify: `crates/suite-ui/src/status_bar.rs` (add `outcome()`, refactor `line()`, add tests)

**Interfaces:**
- Consumes: the existing `Outcome` enum (`Success`/`Failure`/`Cancelled`) and the
  existing `JobState` (`Idle`/`Running{name}`/`Cancelled{name}`/`Done{name,ok}`).
- Produces:
  ```rust
  impl JobState<'_> {
      /// The terminal [`Outcome`] for a finished state, or `None` while `Idle`/`Running`.
      pub fn outcome(&self) -> Option<Outcome>;
  }
  ```
  No enum change — fully additive. Task 7's snapshot uses the unchanged `Done`/
  `Cancelled` variants; no other task depends on this one.

- [ ] **Step 1: Write the failing test for `outcome()`**

Add to `crates/suite-ui/src/status_bar.rs` tests:

```rust
#[test]
fn outcome_maps_finished_states_and_is_none_otherwise() {
    assert_eq!(JobState::Idle.outcome(), None);
    assert_eq!(JobState::Running { name: "j" }.outcome(), None);
    assert_eq!(
        JobState::Done { name: "j", ok: true }.outcome(),
        Some(Outcome::Success)
    );
    assert_eq!(
        JobState::Done { name: "j", ok: false }.outcome(),
        Some(Outcome::Failure)
    );
    assert_eq!(
        JobState::Cancelled { name: "j" }.outcome(),
        Some(Outcome::Cancelled)
    );
}
```

- [ ] **Step 2: Run it — fails (no `outcome` method)**

Run: `cargo test -p suite-ui status_bar::outcome_maps`
Expected: FAIL to compile — `no method named outcome`.

- [ ] **Step 3: Add the accessor**

In `crates/suite-ui/src/status_bar.rs`, add an `impl JobState<'_>` block (above the
existing `impl StatusBar<'_>`):

```rust
impl JobState<'_> {
    /// The terminal [`Outcome`] for a finished state (the single "how it ended"
    /// vocabulary shared with [`Toast`](crate::Toast)), or `None` while the job is
    /// [`Idle`](JobState::Idle) or [`Running`](JobState::Running). Names the mapping
    /// `line()` paints through, so callers (history rows, footers) can reuse the
    /// identical outcome without re-deriving it from `ok`.
    pub fn outcome(&self) -> Option<Outcome> {
        match self {
            JobState::Idle | JobState::Running { .. } => None,
            JobState::Done { ok: true, .. } => Some(Outcome::Success),
            JobState::Done { ok: false, .. } => Some(Outcome::Failure),
            JobState::Cancelled { .. } => Some(Outcome::Cancelled),
            // `JobState` is #[non_exhaustive]: a future finished state has no known
            // outcome here until it is mapped, so report None rather than guess.
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
}
```

- [ ] **Step 4: Refactor `line()` to use `outcome()` (behaviour-preserving)**

The `Done`/`Cancelled` arms of `line()` currently each construct an `Outcome` inline.
Leave `Idle`/`Running` as-is, and collapse the three finished arms into one that goes
through the accessor, so the mapping lives in exactly one place. Replace the three
`Done { ok: true }` / `Done { ok: false }` / `Cancelled` arms with:

```rust
ref finished @ (JobState::Done { name, .. } | JobState::Cancelled { name }) => {
    // outcome() is Some for every finished state; the verb is per-outcome.
    let outcome = finished.outcome().expect("finished state has an outcome");
    let (glyph, style) = outcome.glyph_style(theme);
    let verb = match outcome {
        Outcome::Success => "done",
        Outcome::Failure => "failed",
        Outcome::Cancelled => "cancelled",
        #[allow(unreachable_patterns)]
        _ => "finished",
    };
    Line::from(vec![
        Span::styled(glyph, style),
        Span::styled(format!("{name} — {verb}"), style),
    ])
}
```

> If the `@`-binding with an or-pattern proves awkward to thread `name` through under
> the borrow checker, the equivalent un-collapsed form is acceptable: keep the three
> separate arms but have each call `self.job.outcome()` / a local `Outcome` literal
> and share the `verb` match. The acceptance criterion is "the `Outcome` mapping is
> not duplicated between `outcome()` and `line()`," not the exact arm structure.

- [ ] **Step 5: Run the full status_bar tests + the unchanged snapshot expectations**

Run: `cargo test -p suite-ui status_bar::`
Expected: PASS — the new `outcome()` test plus every pre-existing test
(`each_state_leads_with_its_distinguishing_glyph`, `colour_on_applies_per_state_hues`,
`no_color_drops_every_hue...`, etc.) still green, because the rendered output is
unchanged. The gallery is untouched (no call-site changes).

- [ ] **Step 6: Clippy + fmt + commit**

Run: `cargo clippy -p suite-ui --all-targets -- -D warnings && cargo fmt`

```bash
git add crates/suite-ui/src/status_bar.rs
git commit -m "feat(suite-ui): add JobState::outcome() accessor [R4]

Names the JobState -> Outcome mapping so line() and callers (history rows,
footers) share one source instead of re-deriving it from Done{ok}. Additive and
non-breaking: Done{ok}/Cancelled variants are unchanged."
```

**Acceptance:** `JobState::outcome()` returns `Some(Outcome::..)` for `Done{ok}`/
`Cancelled` and `None` for `Idle`/`Running`; the `Done{ok}`/`Cancelled` variants are
**unchanged**; the `Outcome` mapping appears once (not duplicated between `outcome()`
and `line()`); all suite-ui tests + gallery green; no API break.

---

## Task 9: Docs — headline the two-layer architecture and mark fixes done

**Files:**
- Modify: `docs/design/suite-ui/SUITE_UI_DESIGN.md`
- Modify: `crates/suite-ui/src/lib.rs` (one-line two-layer note, if not already added in Task 5)

**Interfaces:**
- Consumes: nothing. Produces: docs consistent with the shipped code.

- [ ] **Step 1: Flip the resolved review items to "done" in the design doc**

In `docs/design/suite-ui/SUITE_UI_DESIGN.md`, update the §0 table and §11 action list:
mark R1, R2, R3, R4 as **Done (2026-06-20)** with the commit subjects; leave §10
Future Evolution untouched (still out of scope). For R4, note it shipped as the
**additive** variant: *"Implemented as `JobState::outcome() -> Option<Outcome>`
(non-breaking); the `Done{ok}`→`Finished{outcome}` enum collapse from §7 is deferred
to a future coordinated bump and stays recorded there as the eventual cleanup."*
No consumer migration is required for this pass.

- [ ] **Step 2: Confirm the two-layer headline is present**

Verify §2 of the design doc leads with the `thomas-tui` (toolkit) vs `suite-ui`
(shim) split, and that `crates/suite-ui/src/lib.rs`'s crate-doc names both layers
(it already references `thomas_tui` in the `theme` module doc — ensure the top-level
`//!` block states the split in one sentence). If missing, add:

```rust
//! `suite-ui` is the suite-flavoured shim over [`thomas_tui`], the domain-free
//! toolkit: it re-exports the whole toolkit and adds only the widgets welded to
//! suite semantics (job status, severity, health, attention).
```

- [ ] **Step 3: Verify docs build and the full suite is green**

Run: `cargo doc -p thomas-tui -p suite-ui --no-deps`
Then: `cargo test -p thomas-tui -p suite-ui && cargo clippy -p thomas-tui -p suite-ui --all-targets -- -D warnings && cargo fmt --check`
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add docs/design/suite-ui/SUITE_UI_DESIGN.md crates/suite-ui/src/lib.rs
git commit -m "docs(suite-ui): mark R1-R4 done; headline the two-layer architecture"
```

**Acceptance:** the design doc reflects shipped reality (R1–R4 done, consumer
follow-up noted); both crate-docs name the two layers; `cargo doc` clean; full
thomas-tui + suite-ui test/clippy/fmt gate green.

---

## Task 10: Final verification gate

**Files:** none (verification only).

- [ ] **Step 1: Full workspace build does not regress other crates**

Run: `cargo build --workspace`
Expected: the whole workspace still builds. Every change in this plan is additive or
behaviour-preserving (no enum break), so no in-workspace or sibling-repo call-site is
affected.

- [ ] **Step 2: Run every suite-ui/thomas-tui test surface**

Run: `cargo test -p thomas-tui -p suite-ui --all-features`
Then: `cargo test -p thomas-tui --doc && cargo test -p suite-ui --doc`
Expected: all green; test count ≥ baseline + the new tests (4 truncation + 2 thomas
widget + 1 suite widget + 2 thomas snapshots + 1 suite snapshot).

- [ ] **Step 3: Gallery smoke test in all three themes**

Run: `cargo run -p suite-ui --example gallery`
Expected: prints cyan / amber / NO_COLOR frames; the truncated path cells line up to
the pane edge; status footer shows `done`/`failed`/`cancelled`.

- [ ] **Step 4: Update LAST_WORK.md**

Per the suite rule, before declaring done, add a dated entry to
`LAST_WORK.md` (repo root) summarising R1–R4 + docs, the commit list, and the
consumer follow-up for the `JobState` break.

```bash
git add LAST_WORK.md
git commit -m "docs(last-work): record suite-ui review fixes R1-R4"
```

**Acceptance:** `cargo build --workspace` green; all thomas-tui/suite-ui tests +
doctests + snapshots green; clippy `-D warnings` clean; fmt clean; gallery runs;
LAST_WORK.md updated.

---

## Self-Review (coverage check)

- **R1 (truncation bug):** Task 2 (rewrite + wide-char tests), reinforced by Task 6
  (CJK-in-pane snapshot). ✅
- **R2 (widget API):** Task 3 (thomas-tui `Themed` + contract), Task 4 (suite-ui
  impls), Task 5 (documented contract). Both halves of the recommendation — codify
  the by-shape convention *and* add the `ratatui::Widget` surface — are covered. ✅
- **R3 (testing):** Tasks 6 + 7 (insta snapshots); the "avoid `assert_buffer_eq!`"
  item is satisfied by the Global Constraints note (the macro is not used today, so
  there is nothing to migrate — new exact checks use `assert_buffer_lines`). ✅
- **R4 (JobState/Outcome):** Task 8 — additive `JobState::outcome()` accessor (per
  approval; the breaking enum collapse is deferred and recorded in design-doc §7). ✅
- **Two-layer docs:** Task 9. ✅
- **"Other small fixes":** the misleading `text.rs` "Unicode-safe" doc-comment is
  corrected in Task 2; the color_eyre panic-ordering note is documentation-only and
  lives in the design doc §8 (no code change), so it is not a separate task.
- **Type consistency:** `Themed<W>`/`Themable::themed` names are identical across
  Tasks 3/4/7; `JobState::Finished { name, outcome }` is identical in Tasks 7/8;
  `Outcome::glyph_style` is the single mapping used in Task 8 and unchanged. ✅

## Execution ordering note

With R4 additive, the natural order **1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10** holds
with no special constraints: Task 8 no longer changes the `JobState` shape, so Task 7's
snapshot (which uses the unchanged `Done{ok}` variant) is independent of it. Tasks 6
and 7 only need Task 2 (and, for the doc links, the crates to compile). Task 9 should
run last before the final gate so it records the actual shipped commit subjects.
