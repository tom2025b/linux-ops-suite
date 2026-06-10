# suite-ui — comprehensive pass: shared widgets + consistency

**Date:** 2026-06-10
**Status:** Landed (`feat/suite-ui-comprehensive-pass`) — suite-ui only; consumer
migration is the documented follow-up.

## Summary

A focused pass to make `suite-ui` more comprehensive and internally consistent,
**without breaking its public API or expanding its remit**. It adds the handful
of presentation primitives that all three current consumers (Bulwark, RexOps,
ScriptVault) hand-roll today, refreshes the example gallery to cover every
component, and fixes doc drift between the original spec and the code.

The crate stays what it is: **stateless chrome**. Every new item takes a
`Theme`, borrowed display values, and (where it draws) a `Rect`, and owns no
application state and reads nothing from the environment — identical to the
existing widgets.

## Motivation — what consumers duplicate today

Evidence gathered from the three consuming repos:

1. **Path/string truncation — duplicated and already drifting.**
   - `bulwark-core` has `truncate_path` (leading `…`, keeps the filename tail)
     and `truncate_desc` (trailing `…`), both Unicode-aware, char-counted.
   - Bulwark's TUI `text.rs` has a *second* `truncate_path_tail` that uses
     `...` (three ASCII dots) instead of `…`.
   - Bulwark's `table.rs` re-implements tail-truncation **inline** with `…`.
   That is three implementations and two different ellipsis glyphs for one idea.

2. **"N of M" count styling — hand-rolled in three places.** Bulwark
   `table.rs` (pane title), Bulwark `header.rs` ("showing N of M"), ScriptVault
   `search.rs` (the count in its status strip) each independently decide
   "accent/italic when the view is narrowed, dim when it shows everything".

3. **A `·`-joined status strip — rebuilt per tool.** ScriptVault `search.rs`
   formats `All · Auto · 312` by hand; Bulwark's header builds an equivalent
   `[filter …] [risk …] [sort …]` indicator run. Same shape (separator-joined
   labels, often right-aligned), no shared widget.

4. **Empty-state text — ad-hoc strings, no shared styling.** "No items found",
   "No items match the current filter", "no adapters probed", the palette's
   "(no match)" — every screen invents its own wording and styling for "nothing
   to show here".

5. **`pane()` can't carry a styled/counted title.** Because `pane(title, theme)`
   takes a plain `&str` title, Bulwark's `table.rs` copies pane's *look* by hand
   (rounded border + dim border style + 1-col padding) purely to embed a styled
   `48 of 312` count in the title. The file's own comment flags this as a
   workaround.

## What this pass adds

### 1. `text` module — Unicode-aware truncation helpers

Lifts Bulwark-core's proven semantics into suite-ui so every tool truncates the
same way with the same ellipsis (`…`).

```rust
/// Truncate keeping the END (the filename), with a leading `…`.
/// Char-counted (Unicode-safe). Short input returned unchanged; long input
/// yields exactly `max` chars.
pub fn truncate_path(s: &str, max: usize) -> String;

/// Truncate keeping the START, trimming first, with a trailing `…`.
/// Char-counted. Short input (after trim) returned as-is; long input is the
/// head plus `…`, `max` chars total (head = max-1 once an ellipsis is needed).
pub fn truncate_desc(s: &str, max: usize) -> String;
```

Semantics match `bulwark-core::core::report::format` exactly (so a future
Bulwark migration is a drop-in). `max == 0` and tiny widths are saturating, never
panicking. One module-level `const ELLIPSIS: char = '…';` is the single source.

### 2. `Counted` — the "N of M" / shown-of-total span

A tiny value type that produces the count as a styled `Span`/`Line`, encoding the
one rule the three call sites share: **emphasise when the list is narrowed,
recede when it isn't**.

```rust
pub struct Counted { pub shown: usize, pub total: usize }
impl Counted {
    /// `"48 of 312"`. When shown < total, the suite accent (italic) via
    /// `accent_bar()`; when shown == total, `dim()`. Honours NO_COLOR through
    /// the theme (accent → bold-only off).
    pub fn span(self, theme: Theme) -> Span<'static>;
    pub fn line(self, theme: Theme) -> Line<'static>;
    /// `true` when shown < total (the list is filtered) — the predicate the
    /// styling is built on, exposed for callers that want it.
    pub fn is_narrowed(self) -> bool;
}
```

No widget/`render` — it's a span you fold into a title, header, or strip. That is
exactly how all three current call sites use it.

### 3. `StatusStrip` — a `·`-joined run of small labels

The general form of ScriptVault's `All · Auto · 312`: a sequence of short
segments, dim, joined by a spaced `·`, drawn on one line (commonly right-aligned
over a pane's inner area, the way ScriptVault already does it).

```rust
pub struct StatusStrip<'a> { pub segments: &'a [&'a str] }
impl StatusStrip<'_> {
    /// `"All · Auto · 312"` styled dim, `·` separators between segments.
    /// Empty slice → empty line. Mirrors KeyHints/FilterChips conventions.
    pub fn line(&self, theme: Theme) -> Line<'static>;
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: Theme);
}
```

Stateless and dim by design; a caller that wants one segment emphasised composes
`line()` spans itself or drops a `Counted` span in. Right-alignment stays the
caller's `Paragraph::alignment` choice (the widget draws a left-origin line, like
every other one-line widget here), so it composes the same way in any layout.

### 4. `EmptyState` — a centered "nothing to show" placeholder

One shared, consistently-styled way to say a region is empty, replacing the
ad-hoc strings.

```rust
pub struct EmptyState<'a> {
    pub message: &'a str,           // e.g. "No items match the current filter."
    pub hint: Option<&'a str>,      // e.g. "Press Esc to clear the filter."
}
impl EmptyState<'_> {
    /// Center the message (dim, bold) in `area`, with the optional hint dimmer
    /// on the line below. Draws text only — no border/clear — so it sits inside
    /// a pane the caller already framed.
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: Theme);
}
```

### 5. `pane_titled` — a pane that accepts a pre-styled title line

A sibling of `pane()` for the (real, recurring) case of a title with an embedded
styled count, so consumers stop reproducing pane's look by hand.

```rust
/// Same chrome as `pane` (rounded border, dim border style, 1-col h-padding) but
/// the caller supplies the whole title `Line` (so it can embed a styled count,
/// e.g. `results (48 of 312)` with the count in `Counted`'s style).
pub fn pane_titled(title: Line<'static>, theme: Theme) -> Block<'static>;
```

`pane()` stays unchanged and is reimplemented in terms of `pane_titled` (build the
` {title} ` span in `theme.title()`, delegate) so the two cannot drift.

## Consistency / polish (no API breaks)

- **`keys.rs`:** add the `CONFIRM` (Enter) and `CANCEL` (Esc) the original spec
  promised but never landed — as `KeyCode` constants, since they aren't `char`s.
  Leave the existing `char` constants and `is_palette`/`key_hint` as-is. (The
  `key_hint()` string already matches what consumers show; no change there.)
- **Gallery:** add panels for `truncate_*` (before/after), `Counted`
  (narrowed vs full), `StatusStrip`, `EmptyState`, `FilterChips` (currently
  absent), and a `pane_titled` example with a `Counted` count. Keep the
  cyan / amber / NO_COLOR sweep.
- **Docs:** update the `lib.rs` crate-doc component list to include the new
  items; fix the stale references in the *original* spec doc are left as
  historical record (specs are dated snapshots), but `lib.rs` — the living
  doc — is made accurate.

## Out of scope (YAGNI)

- No consumer migration this pass. Replacing each tool's hand-rolled copies with
  these widgets is a documented per-repo follow-up (the same way the crate was
  originally rolled out). This pass lands and tests the shared code only.
- No new theming surface: no config/theme files, no runtime registry, no new
  accent. Full theming = every new item routes through the existing gated
  `Theme`, so colour-on / amber / NO_COLOR all keep working with zero new gates.
- No stateful widgets, no input capture, no async, no scrollbar/gauge/table
  abstraction (consumers use ratatui's `Table` directly and that's fine).
- No breaking API changes. Everything here is additive; `pane()` keeps its
  signature.

## Testing

Per new item, mirroring the crate's existing test style (assert on the composed
spans/string and on the NO_COLOR gate):

- `truncate_path` / `truncate_desc`: short-input passthrough; long input yields
  `max` chars with the right ellipsis on the right side; Unicode boundary safety
  (multi-byte chars not split); `max == 0` / tiny widths don't panic.
- `Counted`: text is `"{shown} of {total}"`; narrowed → a foreground (accent)
  with colour on and `is_narrowed()` true; full → dim, no fg; under NO_COLOR no
  fg in either case but the narrowed one keeps an attribute.
- `StatusStrip`: segments joined by `·` with N-1 separators; empty slice → empty
  line; dim (no fg) in both colour modes.
- `EmptyState`: (pure helper for the composed lines) message present; hint
  present only when `Some`; NO_COLOR drops fg.
- `pane_titled` / `pane`: `pane` still renders its title in `theme.title()`;
  doc-test shows `pane_titled` with an embedded count.

Plus doc-tests on every new public item (the crate's convention), and the gallery
as the cross-theme visual smoke test. `cargo test -p suite-ui --all-features`,
`clippy`, and `fmt` must be green, and the three consumers must still build
unchanged (this pass is purely additive to them).
