# suite-ui — Design & Architecture Review

**Date:** 2026-06-20
**Status:** Design review of the *as-built* crates, with scoped improvements.
**Reviewer role:** TUI framework architecture.
**Scope of this document:** Critique + improved design for the **stateless chrome**
layer (`thomas-tui` + `suite-ui`) as it exists today. Recommendations stay inside
the current remit (no state, no event loop, no async). Anything that would require
moving toward a stateful/component-tree model is quarantined in
[§10 Future Evolution](#10-future-evolution) and is **not** part of the proposed
design.

> This supersedes the two dated spec snapshots
> (`docs/superpowers/specs/2026-06-08-suite-ui-design.md` and
> `2026-06-10-suite-ui-comprehensive-pass-design.md`) as the *living* design
> reference. Those remain as historical record. The single most important fact
> they omit — because they predate it — is that the crate was **split in two**
> (see §2). `docs/ARCHITECTURE.md` already reflects the split; the old specs do
> not.

---

## 0. TL;DR

The design is **good and unusually disciplined**. The stateless-chrome thesis is
correct for this codebase, it matches ratatui's native immediate-mode grain, and
the execution (the `Theme`/`NO_COLOR` gate, the `Tui` RAII guard, the
render-into-`TestBackend` tests) is principal-grade. Do **not** turn this into a
framework.

> **Implementation status (2026-06-20): R1–R4 fixed and committed.** See the
> per-item "Done" notes in §11 and the plan
> `docs/superpowers/plans/2026-06-20-suite-ui-fixes.md`. R4 shipped as the
> **additive** `JobState::outcome()` accessor (non-breaking); the breaking
> `Done{ok}`→`Finished{outcome}` enum collapse described in §7 is deferred to a
> future coordinated bump. §10 Future Evolution is unchanged (still out of scope).

The review finds **one real bug**, a handful of **ergonomic/naming inconsistencies**,
and a short list of **current ratatui best practices worth adopting without adding
state**. In priority order:

| # | Item | Severity | Status |
|---|------|----------|--------|
| R1 | Truncation counts `char`s, not display **columns** — wide CJK/emoji corrupt layout. The doc-comment even calls itself "Unicode-safe." | **Bug (High)** | ✅ Fixed |
| R2 | The widget surface is inconsistent: some types have `.line()`, some `.render()`, some both, some neither; none implement ratatui's `Widget` trait, so nothing composes into the ecosystem. | Medium | ✅ Fixed |
| R3 | `assert_buffer_eq!` is deprecated; snapshot tests (`insta`) would lock layout/truncation cheaply. | Medium (tests) | ✅ Fixed |
| R4 | `JobState::Done { ok: bool }` and the richer `Outcome` enum encode the same thing twice. | Low (naming/model) | ✅ Fixed (additive) |
| R5 | `Theme` is a "fat enum of methods" — fine today, but the accent is a single `Color` and there's no semantic-role indirection; documents the ceiling. | Low | ⏳ Documented (no action) |
| R6 | `Tui` guard is excellent; one panic-hook detail (color_eyre ordering) is worth pinning. | Nit | ⏳ Documented (no action) |

Everything below expands these.

---

## 1. What this crate is (and is not)

`suite-ui` is the suite's shared terminal-UI **chrome**: the look-and-feel that
RexOps, ScriptVault, and Bulwark have in common — a theme, rounded panes, a handful
of overlays, a few status widgets, and a terminal lifecycle guard.

The defining constraint, stated three times across the specs and the crate docs and
**worth keeping verbatim**:

> Every visual component takes a `Theme`, a borrowed data slice, and a `Rect`, and
> draws into a `Frame`. None of them owns application state or domain types.
> `suite-ui` draws the box; the app owns the behaviour.

This is the whole reason two otherwise-decoupled tools can share presentation
without coupling their internals, and it is why this crate is the *one* sanctioned
exception to the suite's "no shared code" rule. A colour change in `suite-ui` cannot
corrupt a snapshot or reclassify a risk level, because `suite-ui` has no access to
either. **This constraint is the design. Defend it.**

---

## 2. The architecture as it actually is: two layers

The specs describe one crate. The code is **two**, and this is the right call:

```
crates/
  thomas-tui/        # domain-FREE, general-purpose toolkit. Publishable to anyone.
    theme.rs         #   Theme, ThemeChoice, ColorChoice, Health, Severity
    tui.rs           #   the RAII terminal guard
    widgets.rs       #   pane / pane_titled / pane_blank
    layout.rs        #   centered_rect / centered_fixed
    text.rs          #   truncate_path / truncate_desc
    counted.rs, status_strip.rs, empty_state.rs, filter_chips.rs,
    key_hints.rs, search_bar.rs, freshness.rs, keys.rs
    overlays/        #   help, confirm, palette

  suite-ui/          # SUITE-flavoured shim over thomas-tui. Re-exports everything,
    status_bar.rs    #   adds the few widgets welded to suite semantics:
    badge.rs         #   StatusBar/JobState/Outcome, SeverityBadge,
    attention_flag.rs#   AttentionFlag, HealthStrip,
    health_strip.rs  #   and the job-lifecycle Toast kinds.
    overlays/toast.rs
```

**This split is excellent and under-celebrated.** `thomas-tui` is a genuinely
reusable, dependency-light (`ratatui` + `crossterm` + optional `clap`) TUI toolkit
that has no idea the Linux Ops Suite exists. `suite-ui` is the thin layer that knows
about jobs, severities, and adapter health. The boundary is clean: the only suite
vocabulary that leaks *down* into `thomas-tui` is `Health` and `Severity` on
`Theme` — and a reasonable argument exists to keep even those in `thomas-tui`,
since "coarse health / severity with NO_COLOR-safe styling" is a general need, not a
suite-specific one. (Verdict: leave them; they're generic enough, and moving them
buys nothing.)

**Recommendation A1 (docs):** Promote the two-layer story to the *top* of the
design narrative. A newcomer reading the specs would not learn that `thomas-tui`
exists until they open `Cargo.toml`. `docs/ARCHITECTURE.md` has it; the design doc
(this file) now does too. The `suite-ui` `lib.rs` already cross-links it — keep that.

**Recommendation A2 (positioning):** `thomas-tui` is good enough to publish to
crates.io independently of the suite. That is the ultimate proof the chrome/logic
split is real. Not urgent; worth stating as a north star, because "would this
embarrass us on crates.io?" is a useful review lens for every future addition.

---

## 3. Strengths (what to protect)

These are not filler — each is a decision a less disciplined library gets wrong, and
each should be treated as a regression risk in future PRs.

1. **The single `NO_COLOR` gate (`theme.rs`).** Every hue routes through one
   `accent(style, color)` primitive and one env read (`no_color_env`). `NO_COLOR`
   strips *foreground colour only* and always leaves an attribute (bold/dim/reverse)
   or a glyph carrying the distinction. This is exactly right, exactly what the
   ratatui maintainers tell people to do (ratatui has **no** built-in `NO_COLOR`
   support — it's an open feature request, and "gate it yourself" is the accepted
   answer), and it is thoroughly tested. The `(false, …)` arms in `health()` /
   `severity()` are the kind of detail libraries skip and regret.

2. **The `Tui` RAII guard (`tui.rs`).** Setup in `new`, teardown in `Drop` on every
   exit path (return, `?`, panic). Three things stand out:
   - The **failure-between-init-and-`Ok`** restore (lines 85–91): if `apply_envelope`
     fails, `Self` was never built, so `Drop` can never run — the code restores
     manually right there. This is the exact bug most hand-rolled guards have.
   - `with_suspended` is factored so the leave→run→re-enter control flow is
     **unit-testable without a real tty** by injecting the terminal ops as closures.
     That is a senior move; the tests (`suspended_skips_body_and_reenters_when_leave_fails`,
     `…reenter_error_dominates…`) lock the precedence rules.
   - `print_after_exit` + the **panic-aware drain** (`if !thread::panicking()`): a
     queued "you picked X" line must not leak to the shell if the process crashed
     and picked nothing. Subtle and correct.

3. **The test methodology.** Widgets are rendered into `ratatui::backend::TestBackend`
   and assertions read the actual buffer cells — glyphs, the rounded-corner `╭`, the
   `DIM` modifier on the border, the padding column at `x=1`. This is the gold-standard
   ratatui testing pattern, not "assert the struct fields." `pane_into_a_tiny_area_does_not_panic`
   (1×1 render) is the kind of edge case that bites in production.

4. **`pane()` reimplemented in terms of `pane_titled()`.** The two cannot drift
   because one literally calls the other. Same discipline in `Outcome::glyph_style`
   being the single source both `StatusBar` and `Toast` render through. This
   "one source, delegate to it" instinct is consistent across the crate.

5. **`#[non_exhaustive]` on every public enum, with `#[allow(unreachable_patterns)]`
   fallback arms** that render a *neutral* style rather than borrowing another
   variant's colour or failing to compile. The comments explain precisely why. This
   is forward-compat done properly.

6. **The compose-or-draw dual surface where it exists.** `.line(theme) -> Line` lets
   a caller fold a widget into a row it lays out itself; `.render(frame, area, theme)`
   draws it. Producing pure `Line`/`Span` data is, per the ecosystem, the *most*
   portable possible surface (it slots into any ratatui layout, and into GUI backends
   — see §9). The crate just doesn't offer it *uniformly* (see R2).

---

## 4. R1 — The truncation bug (the one real defect)

### The claim vs. the reality

`text.rs` advertises Unicode safety:

```rust
//! Both count by `char`, never by byte, so a multi-byte character is never split
//! mid-codepoint …
```

and the tests are named `unicode_boundaries_are_never_split`. But "never split a
codepoint" is **not** the property a terminal needs. A terminal lays out in
**display columns**, and:

- A CJK ideograph (`日`) is **1 `char` but 2 columns**.
- Most emoji (`☕`, `🚀`) are **1 `char` but 2 columns**.
- A combining mark (`é` as `e` + U+0301) is **2 `char`s but 1 column**.
- A ZWJ emoji sequence (`👨‍👩‍👧`) is several `char`s and **2 columns**.

`truncate_path("/tmp/é", 48)` happens to be fine, but
`truncate_path("…/日本語/tool.sh", 20)` produces a string of 20 *chars* that occupies
**up to ~26 columns** — it overflows the cell budget it was asked to fit, and in a
`ratatui::Table` that means it **bleeds past the column edge or pushes neighbours**,
which is the precise layout corruption a shared truncation helper exists to prevent.
The crate's own test `café ☕` "passes" only because the input is short enough to skip
truncation; it never exercises a wide char *at the cut*.

This is High severity precisely *because* it's a shared primitive sold as "the one
correct way to truncate." Every consumer inherits the bug, and the misleading
doc-comment means nobody will suspect it.

### The fix (stays stateless, no new behaviour)

Measure **display width**, not `char` count, using the crate the ecosystem
standardised on — and which **ratatui itself uses internally** for exactly this:

- `unicode-width` (`UnicodeWidthStr::width` / `UnicodeWidthChar::width`) → 0/1/2
  columns per UAX#11.
- `unicode-segmentation` (`.graphemes(true)`) so a multi-codepoint grapheme is
  treated as one unit before measuring.

Sketch (tail-keeping variant):

```rust
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Truncate `s` to at most `max` display COLUMNS, keeping the end, leading `…`.
pub fn truncate_path(s: &str, max: usize) -> String {
    if s.width() <= max { return s.to_string(); }
    if max == 0 { return String::new(); }
    let budget = max - 1;                       // reserve one column for '…'
    // Walk graphemes from the end, accumulating width until we'd exceed budget.
    let mut kept = Vec::new();
    let mut used = 0;
    for g in s.graphemes(true).rev() {
        let w = g.width();
        if used + w > budget { break; }
        used += w;
        kept.push(g);
    }
    kept.reverse();
    format!("{ELLIPSIS}{}", kept.concat())
}
```

Notes:
- The post-condition changes from "exactly `max` **chars**" to "**≤ `max` columns**"
  (you cannot always hit `max` columns exactly — a 2-column glyph may leave a
  1-column gap). The tests must assert `UnicodeWidthStr::width(&out) <= max`, not
  `out.chars().count() == max`. **Update the doc-comment to say "columns," and drop
  the false "Unicode-safe" framing for an accurate "display-width-aware" one.**
- **Even better:** since the crate already builds `Line`/`Span`, consider truncating
  against ratatui's own `Span::width()` / `Line::width()` (which delegate to
  `unicode-width`). That guarantees the truncation math is *identical* to the
  renderer's column math — they can never disagree.
- Add tests with genuinely wide content at the cut: `日本語`, `🚀🚀🚀`, a ZWJ
  sequence, and a combining-mark string (more `char`s than columns). These are the
  cases the current suite structurally cannot catch.

**Dependency cost:** two tiny, ubiquitous, no-`std`-optional crates that are already
transitive deps via ratatui. Net new compile cost ≈ zero. This is the highest-value
change in the review.

---

## 5. R2 — The widget surface is inconsistent

Today the public widgets expose three different shapes with no rule:

| Widget | `.line()` | `.render()` | impl `Widget` |
|--------|:---:|:---:|:---:|
| `SearchBar` | ✓ | ✓ | ✗ |
| `StatusBar` | ✓ | ✓ | ✗ |
| `StatusStrip` | ✓ | ✓ | ✗ |
| `Counted` | `.span()`/`.line()` | — | ✗ |
| `KeyHints` | ✓ | ✓ | ✗ |
| `EmptyState` | (helper) | ✓ | ✗ |
| `Toast` | — | ✓ | ✗ |
| `HelpSheet`/`ConfirmModal`/`PaletteFrame` | — | ✓ | ✗ |

Two problems:

1. **No predictable contract.** A caller cannot guess whether a given widget can be
   folded into a composed line or only drawn into a `Rect`. The convention exists in
   spirit ("one-line widgets get `.line()`; framed overlays get `.render()`") but
   isn't written down or enforced. **Write it down** (see A3).

2. **Nothing implements `ratatui::Widget`.** Every widget uses an *inherent*
   `.render(frame, area, theme)` method. That is ergonomic for the app author, but it
   means **none of these compose into the ratatui ecosystem**: they can't be handed to
   `frame.render_widget(&w, area)`, can't nest inside `tui-popup` / `tui-scrollview`
   containers, can't be used anywhere a `Widget` is expected.

### The constraint that forces a choice

ratatui's trait signature is fixed: `fn render(self, area: Rect, buf: &mut Buffer)`.
**It has no `theme` parameter.** So a theme-gated widget can implement `Widget` only
if the theme rides *inside* the struct. On stable ratatui 0.29 the idiomatic
render-without-consume form is **`impl Widget for &T`** (note: `WidgetRef` is still
behind the `unstable-widget-ref` feature in 0.29 — do **not** rely on it).

### Recommendation R2 (two options, pick one — I recommend B)

**Option A — codify the inherent convention, do nothing structural.** Document the
rule ("single-line widgets expose `fn line(&self, theme) -> Line` *and*
`fn render(&self, frame, area, theme)`; framed overlays expose `fn render` only"),
make every widget conform (e.g. give `Counted` a `render` or explicitly document why
it's span-only), and stop. Zero new types, zero ecosystem interop. Lowest effort.

**Option B (recommended) — add an opt-in `Widget` surface by carrying the theme.**
Offer, *in addition* to today's methods, a thin themed wrapper so the widgets also
work as real ratatui widgets:

```rust
/// Bind a chrome widget to a theme so it implements `ratatui::Widget`
/// (render-by-reference, stays stateless — it borrows, owns nothing).
pub struct Themed<W> { pub widget: W, pub theme: Theme }

impl Widget for &Themed<SearchBar<'_>> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(self.widget.line(self.theme)).render(area, buf);
    }
}
// …or a `.themed(theme)` ext-method on each widget returning `Themed`.
```

This keeps the existing ergonomic `.line()/.render()` API **and** unlocks
`frame.render_widget(&bar.themed(theme), area)` and nesting inside ecosystem
containers — *without adding a single byte of state*. It is the smallest change that
makes the crate a good ratatui citizen. The `Line`/`Span` accessors remain the
most-portable surface and stay exactly as they are.

**Recommendation A3 (docs):** Add a "Widget API contract" section to `lib.rs` stating
the rule: *every widget is a struct of borrowed display values; single-line widgets
provide `line(theme) -> Line`; anything that occupies a region provides
`render(frame, area, theme)`; none capture input or own state.* Make it a checklist
item for new widgets.

---

## 6. R3 — Testing: adopt snapshots, drop the deprecated macro

The unit tests are already strong. Two current-practice upgrades, both pure wins:

1. **Avoid the deprecated `assert_buffer_eq!`** (deprecated since ratatui 0.26.3).
   The crate doesn't use it today (good — its tests read cells directly), so this is
   a *don't-regress* note plus a recommendation for any new exact-buffer comparison:
   use `TestBackend::assert_buffer_lines([...])` (compare against expected lines
   without hand-building a `Buffer`) or plain `assert_eq!(backend.buffer(), &expected)`
   (since `Buffer: PartialEq`).

2. **Add `insta` snapshot tests for layout/truncation.** Render a widget into
   `TestBackend`, then `insta::assert_snapshot!(terminal.backend().buffer())` — the
   buffer's `Display` renders the glyph grid to text. This is officially recommended
   and is the cheapest possible regression net for:
   - the **truncation fix** (snapshot a `日本語`/emoji path in a fixed-width pane and
     watch the column edge),
   - pane/overlay layout (centering, padding, the palette frame),
   - the gallery itself (snapshot each panel).

   **Caveat to document:** `insta` buffer snapshots capture *glyphs and layout, not
   colour/style.* So keep a handful of explicit cell-style assertions (the existing
   `add_modifier.contains(DIM)` checks) for the `NO_COLOR` and accent guarantees —
   snapshots can't see those. The right split is: **snapshots for geometry, explicit
   assertions for style.**

No new runtime behaviour, no state — purely a stronger safety net for the changes in
§4 and §5.

---

## 7. R4 — `JobState` vs `Outcome` model redundancy

`status_bar.rs` has both:

```rust
enum Outcome { Success, Failure, Cancelled }            // the (glyph, style) source
enum JobState<'a> { Idle, Running{name}, Cancelled{name}, Done{name, ok: bool} }
```

`Done { ok: bool }` re-encodes Success-vs-Failure that `Outcome` already models,
and `JobState::Cancelled` parallels `Outcome::Cancelled`. The `line()` method then
*translates* `Done{ok:true} → Outcome::Success`, `Done{ok:false} → Outcome::Failure`,
`Cancelled → Outcome::Cancelled`. So the terminal states are expressed twice and
mapped between.

This isn't a bug — the mapping is centralised and tested — but it's a naming/model
smell. Two cleaner shapes (either is fine; this is low priority and an **API change**,
so batch it with the next breaking bump rather than alone):

- **Collapse `Done`/`Cancelled` into `Outcome`:**
  ```rust
  enum JobState<'a> { Idle, Running { name: &'a str }, Finished { name: &'a str, outcome: Outcome } }
  ```
  Now there is one terminal representation; `Outcome` is the single vocabulary for
  "how it ended," used by `StatusBar`, `Toast`, and any consumer history row. The
  `bool` is gone and a future outcome (e.g. `TimedOut`) is added in exactly one place.

- Or keep `JobState` ergonomic but **derive** the outcome (`impl JobState { fn outcome(&self) -> Option<Outcome> }`) so the mapping has a name and isn't re-inlined.

Prefer the first: it removes the `bool`-blindness (`Done { ok }` is a classic
boolean-trap parameter) and makes `Outcome` the unambiguous source of truth it's
already trying to be.

---

## 8. R5 / R6 — Theme ceiling and lifecycle nits

### R5 — `Theme` is a method-bag with a single accent

`Theme` exposes ~20 semantic style methods (`prompt`, `title`, `selection`,
`health`, `severity`, …). This is **the right shape for today** — semantic roles, not
raw colours, are exactly what callers should ask for, and it's what makes `NO_COLOR`
enforceable in one place. Two observations, neither requiring action now:

1. **The accent is a single `Color`.** A theme is "swap one hue." That's a feature
   (themes are one line) but also the ceiling: there is no second accent, no
   per-surface palette, no notion of a background/dim *colour* (only the dim
   *attribute*). If the suite ever wants, say, a distinct "info" hue separate from
   the accent, it's a `Theme` change, not a caller change — which is the good
   outcome, but worth naming as the current boundary.

2. **If a richer palette is ever wanted**, the ecosystem pattern is a
   *semantic-role palette* (cf. `cursive`'s named roles, or `ratatui-themes`' slots:
   error/warning/success/info). That would be a `Theme` *internal* change — add named
   role accessors, keep the `accent(style, color)` gate — and would **not** touch the
   stateless contract. Capability tiers (truecolor vs 256-colour) would likewise be a
   pure input to `Theme::resolve` via `supports-color` / `COLORTERM`, still no state.
   File under "if asked," not "do now."

### R6 — `Tui` lifecycle nit

The guard is excellent (see §3). One thing to **pin in the design**, since the specs
don't: the crate relies on ratatui's restoring panic hook (`ratatui::try_init`
installs one) **plus** the RAII `Drop`. That belt-and-suspenders is intentional and
correct — keep both. The one current-practice note: when a consumer adds rich panic
reports, the recommended wiring is `color_eyre` (the 2025 ratatui-book default;
`better-panic` is superseded), and its hook must call `ratatui::restore()` **before**
printing, so the backtrace doesn't land in the alt-screen. `Tui` doesn't need to own
this (it's app-level), but the design doc should *say* "consumers: install
color_eyre hooks that restore first" so each tool doesn't rediscover it. The
`with_suspended` injected-closure test seam is a model to copy for any future
lifecycle logic.

---

## 9. Component model verdict (the question the brief asked)

**Should suite-ui lean React-style (retained component tree), immediate-mode, or
something else?**

Decisively: **stay immediate-mode-flavoured and stateless. It already is, and that is
correct.** Reasoning, grounded in the ecosystem:

- **ratatui is immediate-mode at its core.** You re-describe the UI and call
  `Terminal::draw` every frame; ratatui diffs the new `Buffer` against the previous
  and writes only changed cells. A "plain struct of borrowed values, rebuilt each
  frame" widget is the *native grain* of the platform — not a compromise, the idiom.

- **A React/Elm retained model is an *alternative*, and it lives in libraries that
  exist for exactly that** (`tui-realm` / `tuirealm 3.x` for TEA-style components;
  `cursive` for retained, callback-driven, focus-managed view trees). Those are real
  and maintained — and they are the *opposite* of this crate's job. Adopting their
  model would require owning state, focus, and an event loop, which would reintroduce
  the cross-tool coupling the suite forbids. **Decline them on purpose.**

- **What to borrow *conceptually* without their machinery:** `cursive`'s idea of a
  **semantic theme palette** and an explicit **focus/emphasis vocabulary**. You can
  express "this row is focused / selected / errored" as *borrowed display inputs*
  plus named `Theme` roles, gaining the ergonomics without any retained state. (The
  crate already does a version of this: `SearchBar` takes `query`, `StatusBar` takes
  `JobState` — state lives in the caller, the widget just paints it.)

So the answer to "React vs immediate vs other" is **"immediate, and the few places
that smell stateful (a spinner's frame, a list's scroll offset) should take the state
as a borrowed parameter, never own it."** That keeps the contract and still lets the
crate render anything.

---

## 10. Future Evolution

> Everything in this section is **out of scope** for the current design. It is
> recorded so the boundary is deliberate, not accidental, and so a future "we need
> more" conversation starts from an informed place. **None of this should be built
> without an explicit decision to expand the remit** — and each item notes what it
> would cost in coupling.

### 10.1 GUI backends (egui / iced / wgpu / web) — *better than you'd think*

The brief asks how well this supports future GUI backends. Answer: **surprisingly
well, for free, precisely because it stays pure-ratatui.** The portability story in
the ecosystem runs entirely through ratatui's `Backend` trait + the `Buffer`:

- `egui_ratatui` — a ratatui `Backend` that is *also* an `egui` widget; renders a
  full ratatui terminal inside egui/eframe, **native or WASM**.
- `soft_ratatui` — pure-software renderer that rasterises the `Buffer` to RGBA;
  embeds in bevy/macroquad/eframe/web.
- `ratatui-wgpu` — a GPU/wgpu backend targeting desktop + web.

All three consume the **same `Buffer`** your widgets already produce. **Implication:
anything `suite-ui` expresses as ratatui `Widget`/`Line`/`Buffer` content is *already*
renderable in egui, on the GPU, or in a browser** — no abstraction layer required.
There is **no** ratatui↔iced bridge today; do not design for one. The correct
"future GUI" strategy is therefore *not* "add a backend abstraction to suite-ui" — it
is **"keep producing pure ratatui content (R2's `Line`/`Widget` surface) and let the
existing backend crates do the work."** Staying stateless and pure-ratatui *is* the
GUI-readiness plan. (This is the strongest argument yet for adopting R2's `Widget`
surface: it's also the GUI-portability surface.)

What would *break* GUI portability: owning an event loop, hard-coding crossterm input
handling into widgets, or assuming a character grid in widget logic. The crate does
none of these — keep it that way.

### 10.2 Stateful widgets (scroll, spinner animation, text input)

The moment a consumer wants a **scrollable** chrome region, an **animated** spinner,
or an **editable** field *inside the library*, the stateless contract is under
pressure. The ecosystem's stateful widgets (`tui-scrollview`, `throbber-widgets-tui`,
`tui-textarea`, `tui-input`) all own state by nature.

**Recommendation if this ever comes up:** do **not** make the widget own state.
Instead, take the state as a **borrowed parameter** (`scroll_offset: usize`,
`frame_index: usize`, `&TextAreaState`) so the *caller* owns it and the widget stays
a pure function of inputs. That preserves the contract. Only if that proves
genuinely unergonomic across many consumers should a `StatefulWidget`-style API
(state held by the app, passed by `&mut` at render) be considered — and even then,
ratatui's own `StatefulWidget` keeps state in the app, not the widget, which is
compatible with this crate's philosophy. Reach for the ecosystem crates in the
*consumer*, not in `suite-ui`, until the duplication is proven.

### 10.3 An app-runtime / event loop

Explicitly **rejected and previously deleted** (the suite standardised on driving
each tool's own loop via `Tui::terminal()`). Recorded here only so it is not
re-proposed by accident: a shared event loop would have to know about each tool's
state and messages, which is the coupling the whole crate exists to avoid. The
`Tui::terminal()` escape hatch is the correct seam — the library owns the *envelope*,
the app owns the *loop*.

### 10.4 Theming surface (config files, runtime registry, more accents)

YAGNI today, and the specs are right to say so. If it ever lands, it stays a `Theme`
*internal* change (semantic-role palette per §8, optional capability tiers via
`supports-color`/`COLORTERM`) — no config-file parsing in the widgets, no runtime
mutation of a shared theme, no state. The `Theme::resolve` choke point is where any
of this would plug in.

---

## 11. Concrete action list (priority-ordered, all in-scope)

1. **✅ Done (2026-06-20) — [Bug] truncate by display width.** `truncate_path`/
   `truncate_desc` now measure UAX#11 columns over grapheme clusters
   (`unicode-width` + `unicode-segmentation`); the post-condition and module-doc say
   "columns" and the false "Unicode-safe / char count" framing is gone; wide-CJK /
   emoji / combining-mark / ZWJ tests added. *(commit: "fix(thomas-tui): truncate by
   display columns, not char count [R1]")* (§4)
2. **✅ Done (2026-06-20) — [API] widget contract + `ratatui::Widget` surface.** A
   public `ThemedLine` trait + a single blanket `impl Widget for &Themed<W>` give
   every one-line widget `.themed(theme)` (a blanket over a local trait so widgets in
   *other* crates — suite-ui — opt in without tripping the orphan rule); the
   span/line/render-by-shape contract is documented in both crate-docs.
   *(commits: "feat(thomas-tui): add Themed<W> ratatui::Widget surface [R2]",
   "feat(suite-ui): ThemedLine opt-in … [R2]", "docs(suite-ui): … Widget API
   contract … [R2]")* (§5)
3. **✅ Done (2026-06-20) — [Tests] `insta` geometry snapshots.** thomas-tui (pane +
   the CJK-in-narrow-pane R1 regression net) and suite-ui (status footer); the CJK
   snapshot shows the right border intact. `assert_buffer_eq!` was never used, so the
   "avoid it" item is a standing convention only (NO_COLOR/accent stay covered by the
   in-crate style assertions). *(commits: "test(thomas-tui): insta geometry
   snapshots … [R3]", "test(suite-ui): insta snapshot for the status footer [R3]")* (§6)
4. **✅ Done (2026-06-20, additive variant) — [Model] `JobState::outcome()`.** Added
   `JobState::outcome() -> Option<Outcome>` and routed `line()` through it so the
   `JobState → Outcome` mapping lives in one place; the `Done{ok}`/`Cancelled`
   variants are **unchanged** (non-breaking, no consumer migration). The full
   `Done{ok}`→`Finished{outcome}` enum collapse from §7 — which removes the boolean
   trap entirely — remains the eventual cleanup for a future coordinated bump.
   *(commit: "feat(suite-ui): add JobState::outcome() accessor [R4]")* (§7)
5. **✅ Done (2026-06-20) — [Docs] two-layer headline.** The `thomas-tui` (toolkit) /
   `suite-ui` (shim) split now leads §2 of this doc and both crate-docs; `thomas-tui`
   noted as independently publishable. The color_eyre "restore-first" panic wiring is
   recorded as guidance in §8 (consumer-side, documentation-only — no code change in
   this crate). *(folded into the [R2] docs commit + this doc)* (§2, §6, §8)
6. **⏳ Open [Watch] — record the `Theme` single-accent ceiling and the
   stateful-widget / GUI-backend strategy** so future "we need more" requests start
   from §10, not from a redesign. *(Captured by §8 and §10; no code action.)*

**Non-goals reaffirmed (YAGNI, unchanged from the specs):** no stateful widgets, no
input capture, no async, no event loop, no config/theme files, no runtime theme
registry, no cross-toolkit GUI abstraction. The crate stays *chrome*; the app owns
the *behaviour*.

---

## Appendix A — Ecosystem reference (current as of 2026-06)

Facts the recommendations rest on (versions move fast; re-verify against the pinned
ratatui before relying on any single one):

- **ratatui has no built-in `NO_COLOR`** — gating it yourself via "don't set a fg" /
  `Color::Reset` is the upstream-recommended pattern. ✅ matches `theme.rs`.
- **`WidgetRef`/`StatefulWidgetRef` are gated behind `unstable-widget-ref` in 0.29.**
  The stable render-by-ref idiom is **`impl Widget for &T`**. Use that, not
  `WidgetRef`, while on 0.29.
- **`assert_buffer_eq!` deprecated (0.26.3)** → `TestBackend::assert_buffer_lines` /
  `assert_eq!(backend.buffer(), &expected)`.
- **`insta` buffer snapshots** are officially recommended; they capture **glyphs/layout
  only, not colour** — keep explicit style assertions alongside.
- **`unicode-width` (UAX#11) is the ecosystem standard for column width and is used by
  ratatui internally** (with `unicode-segmentation` for graphemes). This is the
  correct fix for R1. ratatui's own `Span::width()`/`Line::width()` delegate to it.
- **`ratatui::try_init`/`restore` + a restore-first panic hook + RAII `Drop`** is the
  current recommended lifecycle; pair the hook with **`color_eyre`** (book default;
  `better-panic` superseded). Hand-rolled RAII guard is idiomatic — no standard guard
  crate. ✅ matches `tui.rs`.
- **GUI/web portability is via ratatui's `Backend`+`Buffer`**: `egui_ratatui`,
  `soft_ratatui`, `ratatui-wgpu` all consume the same `Buffer`. **No ratatui↔iced
  bridge exists.** Staying pure-ratatui = GUI-ready.
- **Stateful ecosystem widgets** (`tui-scrollview`, `throbber-widgets-tui`,
  `tui-textarea`, `tui-input`, `tui-popup` — most under the maintained `tui-widgets`
  umbrella) exist if a *consumer* needs them; keep them out of stateless `suite-ui`,
  or take their state as a borrowed parameter.
- **`tui-realm`/`tuirealm 3.x`** (TEA components) and **`cursive 0.21.x`** (retained,
  focus-managed) are the maintained *alternatives* to this crate's model — correctly
  declined.

> ⚠ **Version note:** `ratatui 0.30.0` has shipped (it reverses the `WidgetRef`
> blanket impl to `impl WidgetRef for &W where W: Widget`, switches `TestBackend`'s
> error to `Infallible`, and adds `ratatui::run(closure)`). The suite pins **0.29**;
> the recommendations above are written for 0.29 and noted where 0.30 differs. Re-check
> the `unstable-widget-ref` flag status against whatever you actually pin before
> depending on `WidgetRef`.
