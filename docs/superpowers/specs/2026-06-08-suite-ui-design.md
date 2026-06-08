# suite-ui — shared TUI chrome for the Linux Ops Suite

**Date:** 2026-06-08
**Status:** Approved, building

## Summary

A small Rust library crate, `suite-ui`, providing the shared terminal-UI
*chrome* (theme/palette, rounded pane styling, common overlays, keymap
conventions) used by the suite's TUIs — **RexOps** and **ScriptVault**. The
gold-standard source is ScriptVault's existing `theme.rs` / `ui/` code; we lift
the proven parts almost verbatim and generalize them so both tools can share
them.

This pass builds the crate **self-contained**: it compiles, ships an example
gallery, and is unit-tested. Wiring RexOps and ScriptVault to actually consume
it is a documented follow-up (the consumption mechanism — git dep vs path — is
decided per-repo later).

## Architectural decision (and the exception it records)

The umbrella repo (`linux-ops-suite`) was, until now, explicitly **not** a Cargo
workspace and explicitly **forbade shared code** between tools — see the old
"Why shared code is forbidden" section of `docs/ARCHITECTURE.md`. The chosen
direction overrides that: the umbrella becomes a minimal workspace and gains one
**blessed shared UI crate**.

This is a deliberate, documented exception, justified by *what* is being shared:

- `suite-ui` contains **pure presentation** — styles, borders, modal layout.
- It carries **no domain types, no data, and no cross-tool data flow**.
- The file-contract rule exists to stop one tool's *internals/logic* from
  silently coupling to another's. Shared chrome doesn't reintroduce that
  coupling: a colour change in `suite-ui` can't corrupt a snapshot or change a
  risk classification.

`docs/ARCHITECTURE.md` and `README.md` are updated so the docs reflect this new
reality rather than contradicting it.

## Crate layout

```
crates/suite-ui/
  Cargo.toml
  src/
    lib.rs              # public re-exports + crate docs
    theme.rs            # Theme, ThemeChoice, ColorChoice, Health  (the core)
    keys.rs             # shared keymap constants + KEY_HINT helper
    widgets.rs          # pane(), centered_rect(), centered_fixed()
    overlays/
      mod.rs            # re-exports
      help.rs           # HelpSheet
      confirm.rs        # ConfirmModal
      toast.rs          # Toast
      palette.rs        # PaletteFrame (+ PaletteItem) — CHROME ONLY
  examples/
    gallery.rs          # renders every component; cyan / amber / NO_COLOR
```

Each module has one purpose. Each visual component takes a `&Theme`, a borrowed
data slice, and a `Rect`, and draws into a `Frame`. **No component touches app
state or domain types.**

## Theme API

Lifted ~verbatim from ScriptVault (proven, already tested):

- `Theme { color: bool, accent: Color }` — `Copy`, passed by value.
- `ThemeChoice { Cyan, Amber }` — accent-only swap. `Cyan = Color::Cyan`,
  `Amber = Color::Rgb(215,153,33)`.
- `ColorChoice { Auto, Always, Never }` — the single `NO_COLOR` gate; one env
  read in `no_color_env()`.
- `Theme::resolve(ColorChoice, ThemeChoice)` — the one place colour is decided.
- `Theme::with_color(bool)` — deterministic test/example seam. **Made `pub`**
  (not `cfg(test)`) so the `examples/gallery.rs` can use it.
- private `accent(style, color)` primitive.

Semantic style methods carried over verbatim:
`prompt`, `title`, `dim`, `selection`, `selected_rail`, `accent_bar`,
`match_text`, `match_label(color)`, `status_error`, `live_marker`, `meta_key`,
`stderr`.

**Dropped** (stays in ScriptVault — app-specific, not suite chrome):
`SyntaxKind` and `syntax()` — preview syntax highlighting is a ScriptVault
concern.

**Added** for RexOps (generalizing its scattered free functions
`health_style` / `working_style` / `help_style` / `confirm_style` into methods
on the same gated `Theme`):

- `Health { Healthy, Degraded, Unavailable, Unknown }`
- `health(Health) -> Style` — colour: green+bold / yellow / red / dark-gray;
  `NO_COLOR`: bold / plain / bold / dim (so severity still reads without hue).
- `working() -> Style` — the "refreshing…" indicator (yellow / plain).
- `confirm() -> Style` — bright attention for pending destructive actions.

## Widgets

From ScriptVault's `ui/layout.rs`, generalized (drop the ScriptVault-specific
`layout_areas`/`list_rect`):

- `pane(title: &str, theme: &Theme) -> Block<'static>` — rounded border, dim
  border style, accent title. The consistent pane chrome.
- `centered_rect(pct_w, pct_h, area) -> Rect` — verbatim.
- `centered_fixed(w, h, area) -> Rect` — verbatim.

## Overlays — components take data, not `&App`

The limitation in ScriptVault's current overlays is that every `render_*` takes
`&App` and reaches into private fields, so it can't be reused. `suite-ui`
inverts this: the app keeps its state and passes in only what to draw.

- `HelpSheet<'a> { title, rows: &'a [(&'a str, &'a str)] }` →
  `.render(frame, area, theme)`. ScriptVault's `help.rs` minus the hard-coded
  rows.
- `ConfirmModal<'a> { title, message }` → `.render(...)`. Accent border,
  attention-styled message. Generalized from RexOps's inline confirm modal.
- `Toast<'a> { text, kind: ToastKind::{Info,Error} }` → `.render(...)`.
  One-line transient flash; `Error` uses `status_error`.
- `PaletteFrame<'a> { query, items: &'a [PaletteItem<'a>], selected }` with
  `PaletteItem<'a> { label, desc }` → `.render(...)`. Draws the command-palette
  **chrome only**: the `>` input row, the selectable list, the footer.

  **Explicitly out of scope:** the palette's dispatch/filtering/effects
  (ScriptVault's 356-line `dispatch_palette` match) stay 100% in ScriptVault.
  `suite-ui` draws the box; the app owns behaviour. This is the clean split —
  chrome is shared, command logic never is.

## Keymap constants (`keys.rs`)

Named constants for the conventions **both** TUIs already share, to kill magic
literals and drift. This is **not** a key handler — each app keeps its own
`match`; this just gives the shared keys one name.

- `QUIT='q'`, `HELP='?'`, `PALETTE` (`^P` / `:`), `UP='k'`/`DOWN='j'`,
  `CONFIRM=Enter`, `CANCEL=Esc`.
- A `KEY_HINT` footer-string helper for the conventional hint line.

Tool-specific keys (ScriptVault's `^F` favorite, RexOps's `1`–`6` screen
switches) stay in their own keymaps. `keys.rs` holds only the shared subset.

## Cargo / workspace / docs

- **NEW** `linux-ops-suite/Cargo.toml`: `[workspace]`, `resolver = "2"`,
  `members = ["crates/suite-ui"]`.
- **NEW** `crates/suite-ui/Cargo.toml`:
  - deps: `ratatui = "0.29"`, `crossterm = "0.28"` (matching both consumers).
  - feature `clap` → derives `clap::ValueEnum` on `ThemeChoice`/`ColorChoice`,
    **off by default** so consumers that don't use clap stay lean. ScriptVault
    (which parses `--theme`/`--color` via clap) would enable it.
- **EDIT** `docs/ARCHITECTURE.md`: replace the "not a workspace / shared code
  forbidden" passages with the new reality — one blessed shared UI crate, and
  why pure chrome doesn't reintroduce the coupling the file-contract rule
  prevents.
- **EDIT** `README.md`: note `suite-ui` in the structure.
- `.gitignore`: add `/target`.

## Testing

Unit tests carried/adapted from ScriptVault plus the new additions:

- `NO_COLOR` strips every foreground colour (`prompt`/`meta_key`/`match_label`,
  and `selection` carries no bg).
- colour-on applies the expected foreground.
- `accent_bar` coloured only with colour on.
- `resolve` respects explicit `--color` over the env.
- `ThemeChoice` swaps the accent hue (cyan vs amber) and `NO_COLOR` wins over
  the theme.
- `dim` is colourless in both modes.
- **new:** `health()` maps each `Health` to its colour with colour on, and to a
  no-foreground attribute under `NO_COLOR`.
- **new:** `centered_fixed` clamps to the parent rect; `centered_rect` centers.

`examples/gallery.rs` renders every component once in cyan, amber, and
`NO_COLOR` — a manual visual smoke test that also guarantees the public API is
actually usable from outside the crate.

## Non-goals (YAGNI)

No config/theme files, no runtime theme registry, no generalized
"palette-as-data" abstraction, no async, and no other tools wired this pass.
Duplication beyond the shared chrome is fine; we are not lifting app logic.

## Addendum (2026-06-08): job-event toast kinds

`ToastKind` gains three job-lifecycle variants — `Success`, `Failure`,
`Cancelled` — alongside `Info`/`Error`. `Toast` stays exactly what it is: a
single, caller-rendered, **stateless** line (no stacking, no timing, no overlay
frame). The app owns when a toast appears and disappears; the kind only chooses
the leading glyph + style.

To keep a toast and the persistent status segment reading identically, the new
kinds reuse the `StatusBar` glyphs and styles:

- `Success`  → `✓ ` + `health(Healthy)` (green/bold)
- `Failure`  → `✗ ` + `status_error` (red/bold)
- `Cancelled`→ `■ ` + `working` (yellow; dim under `NO_COLOR`)

`Info`/`Error` are unchanged. As elsewhere, the leading glyph carries the
distinction when hue drops away under `NO_COLOR`. No new constructors and no
name-formatting in the widget — the caller passes the message text; the kind
picks glyph+style. Stacking, auto-expiry, a corner overlay, and structured
job-name constructors stay out of scope (YAGNI).
