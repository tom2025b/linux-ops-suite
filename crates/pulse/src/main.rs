//! pulse — a calm, read-only status instrument for the Linux Ops Suite.
//!
//! Pulse answers one question first: *is the suite healthy right now?* It opens
//! to a single verdict centered on a near-empty screen, not a dashboard. The
//! full design lives in `PULSE_DESIGN.md` at the repo root; this file implements
//! its **default screen** — the three verdict states, with the deliberately
//! minimal healthy layout:
//!
//! ```text
//!
//!
//!                              all clear
//!
//!                                                          2m ago
//! ```
//!
//! Design rules this renderer enforces (see PULSE_DESIGN.md "Default Screen"):
//!   - Healthy is the emptiest screen in the suite: just the lowercase verdict
//!     `all clear`, anchored slightly above center, and one dim `2m ago` time in
//!     the lower-right corner. No wordmark, no supporting line, no source
//!     markers, no cause rows, no rule, no hint strip.
//!   - Non-healthy states fill from the center outward: an ALL-CAPS verdict, a
//!     one-line count/summary, an optional confidence line, up to two/three
//!     cause rows, a source-confidence line, and a bottom hint strip. The
//!     wordmark and the `updated` timestamp label return on these states.
//!   - Elements appear and vanish between states but never change position; the
//!     vertical anchor is constant. That stability is the premium feel.
//!
//! Rendering is the shared suite chrome: the interactive UI and the headless
//! `--dump-view`/`--state` previews both draw through `suite_ui` (over ratatui),
//! in `crate::view`. Colour follows the suite rule — on only when stdout is a TTY
//! and `NO_COLOR` is unset, all gated by `suite_ui::Theme` — and the screen stays
//! fully legible with colour off, because state is always also carried by the
//! verdict word and by marker shape, never by colour alone. This module keeps the
//! arg parsing, the verdict text/summary helpers, and the live/one-shot/dump
//! dispatch; the drawing lives in `crate::view`.
//!
//! Usage:
//!   pulse                 interactive verdict screen (when stdout is a TTY)
//!   pulse --state STATE   force a demo state: healthy | attention | incomplete
//!   pulse --theme THEME   accent: cyan | amber       (suite_ui::ThemeChoice)
//!   pulse --color WHEN    auto | always | never      (suite_ui::ColorChoice)
//!   pulse --dump-view V   render one view once and exit (headless)
//!   pulse --no-clear      force the one-shot render instead of interactive mode
//!   pulse -h | --help     this help
//!
//! Environment:
//!   NO_COLOR   disable colour (also auto-disabled when stdout isn't a TTY).
//!   COLUMNS / LINES   honored as a fallback terminal size when the ioctl can't
//!                     read one (e.g. when stdout is a pipe).

use std::env;
use std::process::ExitCode;

use suite_core::env::stdout_is_tty;
use suite_ui::{ColorChoice, Theme, ThemeChoice};

/// Below this width or height we stop trying to center and fall back to a plain
/// top-left render, so a tiny / odd terminal never clips the verdict. 80x24 is
/// the compact target the suite's prior TUI work calls out.
pub(crate) const MIN_CENTER_WIDTH: u16 = 24;
pub(crate) const MIN_CENTER_HEIGHT: u16 = 8;

/// Default assumed size when no TTY and no COLUMNS/LINES — a classic terminal.
const FALLBACK_WIDTH: u16 = 80;
const FALLBACK_HEIGHT: u16 = 24;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let mut clear = true;
    // None => build the verdict from live suite data (the default). Some(name)
    // => force a demo state so the three layouts can be shown without feeds.
    let mut demo: Option<String> = None;
    // Some((view, query)) => render one interactive view once and exit. A
    // deterministic preview/snapshot path (no event loop, no PTY).
    let mut dump_view: Option<(String, String)> = None;
    // suite-ui theme/colour choices. Default: cyan accent, Auto colour (honours
    // NO_COLOR). Parsed from --theme / --color below.
    let mut theme_choice = ThemeChoice::Cyan;
    let mut color_choice = ColorChoice::Auto;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "--no-clear" => clear = false,
            "--state" => {
                let Some(name) = args.get(i + 1) else {
                    eprintln!("pulse: --state needs a value (healthy | attention | incomplete)");
                    return ExitCode::from(2);
                };
                if Verdict::demo(name).is_none() {
                    eprintln!(
                        "pulse: unknown state '{name}' (expected: healthy | attention | incomplete)"
                    );
                    return ExitCode::from(2);
                }
                demo = Some(name.clone());
                i += 1;
            }
            "--data-dir" => {
                let Some(path) = args.get(i + 1) else {
                    eprintln!("pulse: --data-dir needs a path");
                    return ExitCode::from(2);
                };
                // Equivalent to exporting PULSE_DATA_DIR; sources::DataDir reads it.
                std::env::set_var("PULSE_DATA_DIR", path);
                i += 1;
            }
            "--theme" => {
                let Some(name) = args.get(i + 1) else {
                    eprintln!("pulse: --theme needs a value (cyan | amber)");
                    return ExitCode::from(2);
                };
                theme_choice = match name.as_str() {
                    "cyan" => ThemeChoice::Cyan,
                    "amber" => ThemeChoice::Amber,
                    other => {
                        eprintln!("pulse: unknown theme '{other}' (expected: cyan | amber)");
                        return ExitCode::from(2);
                    }
                };
                i += 1;
            }
            "--color" => {
                let Some(name) = args.get(i + 1) else {
                    eprintln!("pulse: --color needs a value (auto | always | never)");
                    return ExitCode::from(2);
                };
                color_choice = match name.as_str() {
                    "auto" => ColorChoice::Auto,
                    "always" => ColorChoice::Always,
                    "never" => ColorChoice::Never,
                    other => {
                        eprintln!(
                            "pulse: unknown color '{other}' (expected: auto | always | never)"
                        );
                        return ExitCode::from(2);
                    }
                };
                i += 1;
            }
            "--dump-view" => {
                let Some(name) = args.get(i + 1) else {
                    eprintln!("pulse: --dump-view needs a view (default|attention|feeds|details|help|search)");
                    return ExitCode::from(2);
                };
                // Optional trailing query for the search view: --dump-view search aws
                let query = args.get(i + 2).cloned().unwrap_or_default();
                let consumed = if query.is_empty() { 1 } else { 2 };
                dump_view = Some((name.clone(), query));
                i += consumed;
            }
            other => {
                eprintln!("pulse: unexpected argument '{other}' (try --help)");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    // The resolved suite-ui palette (accent + NO_COLOR gate), carried on the app
    // for the ratatui draws. `--color`/`--theme`/`NO_COLOR` all meet here.
    let theme = Theme::resolve(color_choice, theme_choice);

    // Deterministic single-view render (preview / snapshot), no event loop. Goes
    // through the same ratatui draw the live UI uses (headless via TestBackend),
    // so the dump is the real screen — glyphs/layout (a text dump is monochrome).
    if let Some((view, query)) = dump_view {
        let readings = verdict::Readings::load(&sources::DataDir::resolve());
        let mut app = app::App::new(readings, theme);
        return match app.dump(&view, &query, TermSize::resolve()) {
            Some(frame) => {
                println!("{frame}");
                ExitCode::SUCCESS
            }
            None => {
                eprintln!(
                    "pulse: unknown view '{view}' (default|attention|feeds|details|help|search)"
                );
                ExitCode::from(2)
            }
        };
    }

    // Interactive mode when we own a real screen. Color is deliberately not part
    // of this decision: NO_COLOR must keep the UI interactive, only monochrome.
    // Otherwise render once and exit, which keeps the output greppable and CI-
    // friendly. (A forced --state is a static demo with no live data to drill
    // into, so it stays render-once too.)
    let interactive = should_run_interactive(clear, stdout_is_tty(), demo.is_none());

    if interactive {
        let readings = verdict::Readings::load(&sources::DataDir::resolve());
        return match app::App::new(readings, theme).run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("pulse: {e}");
                ExitCode::FAILURE
            }
        };
    }

    // Non-interactive: render the default verdict screen once to stdout, through
    // the same ratatui draw (headless). A forced `--state` is a static demo
    // verdict; otherwise build the live verdict from the suite contracts.
    let app = match demo {
        Some(name) => app::App::from_verdict(Verdict::demo(&name).expect("validated above"), theme),
        None => app::App::new(verdict::Readings::load(&sources::DataDir::resolve()), theme),
    };
    let size = TermSize::resolve();
    print!("{}", view::render_to_string(&app, size.width, size.height));

    ExitCode::SUCCESS
}

const HELP: &str = "\
pulse — calm, read-only status for the Linux Ops Suite

USAGE:
    pulse [OPTIONS]

OPTIONS:
    --state <STATE>   force a demo state: healthy | attention | incomplete
                      (default: build the verdict from live suite data)
    --data-dir <DIR>  read suite feeds from DIR instead of the default data dir
                      (same as setting $PULSE_DATA_DIR)
    --dump-view <V>   render one view once and exit (no event loop):
                      default | attention | feeds | details | help | search
                      (append a query for search: --dump-view search aws)
    --theme <THEME>   accent colour: cyan (default) | amber
    --color <WHEN>    colour output: auto (default, honours NO_COLOR) |
                      always | never
    --no-clear        don't clear the screen first (useful when piping)
    -h, --help        print this help

With no options, Pulse reads the suite's file contracts under $XDG_DATA_HOME
(fallback ~/.local/share) and renders the live verdict. See PULSE_DESIGN.md.
";

mod app;
mod cockpit;
mod sources;
mod status;
mod tui;
mod verdict;
mod view;

use verdict::{State, Verdict};

// ─────────────────────────────────────────────────────────────────────────────
// Rendering
// ─────────────────────────────────────────────────────────────────────────────

fn should_run_interactive(clear: bool, stdout_tty: bool, live_data: bool) -> bool {
    clear && stdout_tty && live_data
}

/// The verdict word for a state. Healthy is intentionally lowercase ("all
/// clear") — a calm state does not shout; the others are ALL CAPS for urgency.
pub(crate) fn verdict_text(state: State) -> String {
    match state {
        State::Healthy => "all clear".to_string(),
        State::NeedsAttention => "NEEDS ATTENTION".to_string(),
        State::Incomplete => "INCOMPLETE".to_string(),
    }
}

/// "2 critical · 4 high", collapsed onto one calm line. Drops a zero side so a
/// high-only verdict doesn't read "0 critical". This is the *visible* text;
/// `count_line` adds color.
pub(crate) fn count_summary(v: &Verdict) -> String {
    match (v.critical, v.high) {
        (0, 0) => String::new(),
        (c, 0) => format!("{c} critical"),
        (0, h) => format!("{h} high"),
        (c, h) => format!("{c} critical · {h} high"),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

pub(crate) fn incomplete_summary(v: &Verdict) -> String {
    match (v.unavailable, v.stale) {
        (0, 0) => "suite view unavailable".to_string(),
        (0, stale) => plural(stale, "source stale", "sources stale"),
        (unavailable, 0) => plural(unavailable, "source unavailable", "sources unavailable"),
        (unavailable, stale) => format!(
            "{} · {}",
            plural(unavailable, "unavailable", "unavailable"),
            plural(stale, "stale", "stale")
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Terminal size / TTY  (hand-rolled libc, no dependency — see rex-check)
// ─────────────────────────────────────────────────────────────────────────────

/// Resolved terminal size in character cells.
#[derive(Clone, Copy)]
pub(crate) struct TermSize {
    pub(crate) width: u16,
    pub(crate) height: u16,
}

impl TermSize {
    /// Resolve the terminal size: ask the tty via `TIOCGWINSZ`; if that fails
    /// (e.g. stdout is a pipe), fall back to `$COLUMNS`/`$LINES`, then to a
    /// classic 80x24. Never returns zero in either dimension.
    pub(crate) fn resolve() -> Self {
        if let Some((w, h)) = ioctl_winsize() {
            if w > 0 && h > 0 {
                return TermSize {
                    width: w,
                    height: h,
                };
            }
        }
        let w = env_u16("COLUMNS").unwrap_or(FALLBACK_WIDTH);
        let h = env_u16("LINES").unwrap_or(FALLBACK_HEIGHT);
        TermSize {
            width: w.max(1),
            height: h.max(1),
        }
    }
}

fn env_u16(key: &str) -> Option<u16> {
    env::var(key).ok()?.trim().parse().ok()
}

/// Ask the kernel for stdout's window size via `ioctl(TIOCGWINSZ)`. Returns
/// `(cols, rows)` or None when stdout isn't a terminal. One tiny libc call;
/// avoids a dependency just to center text — same spirit as rex-check's
/// hand-rolled `isatty`.
fn ioctl_winsize() -> Option<(u16, u16)> {
    // struct winsize { ws_row, ws_col, ws_xpixel, ws_ypixel } — all c_ushort.
    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }
    // TIOCGWINSZ is 0x5413 on Linux. This binary targets Linux (the suite is
    // Linux-only), so the constant is fixed here rather than pulled from libc.
    const TIOCGWINSZ: u64 = 0x5413;
    extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
    let mut ws = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: ioctl writes a Winsize into our stack buffer for the TIOCGWINSZ
    // request; the buffer is correctly sized and aligned, and we only read it
    // back on success.
    let rc = unsafe { ioctl(1, TIOCGWINSZ, &mut ws as *mut Winsize) };
    if rc == 0 {
        Some((ws.ws_col, ws.ws_row))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_resolution_honours_the_suite_ui_colour_gate() {
        // --color=never forces colour off: no accent fg survives, mirroring
        // suite-ui's own gate (the single place NO_COLOR is enforced now).
        let off = Theme::resolve(ColorChoice::Never, ThemeChoice::Cyan);
        assert!(!off.color_enabled(), "never disables colour");
        assert_eq!(off.title().fg, None, "no accent fg under colour-off");
        // --color=always forces it on regardless of the runner's NO_COLOR.
        let on = Theme::resolve(ColorChoice::Always, ThemeChoice::Cyan);
        assert!(on.color_enabled(), "always forces colour on");
        assert!(on.title().fg.is_some(), "accent fg present with colour on");
        // The accent swaps with the theme choice (only when colour is on).
        assert_ne!(
            Theme::resolve(ColorChoice::Always, ThemeChoice::Cyan)
                .title()
                .fg,
            Theme::resolve(ColorChoice::Always, ThemeChoice::Amber)
                .title()
                .fg,
            "cyan vs amber accent differ when colour is on"
        );
    }

    #[test]
    fn verdict_words_match_the_design() {
        assert_eq!(verdict_text(State::Healthy), "all clear");
        assert_eq!(verdict_text(State::NeedsAttention), "NEEDS ATTENTION");
        assert_eq!(verdict_text(State::Incomplete), "INCOMPLETE");
    }

    #[test]
    fn no_color_does_not_disable_interactive_mode() {
        assert!(should_run_interactive(true, true, true));
        assert!(!should_run_interactive(false, true, true));
        assert!(!should_run_interactive(true, false, true));
        assert!(!should_run_interactive(true, true, false));
    }

    #[test]
    fn count_summary_drops_zero_sides() {
        let mut v = Verdict::demo("attention").unwrap();
        v.critical = 0;
        v.high = 3;
        assert_eq!(count_summary(&v), "3 high");
        v.critical = 1;
        v.high = 0;
        assert_eq!(count_summary(&v), "1 critical");
        v.critical = 2;
        v.high = 4;
        assert_eq!(count_summary(&v), "2 critical · 4 high");
    }

    #[test]
    fn incomplete_summary_names_stale_vs_unavailable_sources() {
        let mut v = Verdict::demo("incomplete").unwrap();
        v.unavailable = 0;
        v.stale = 1;
        assert_eq!(incomplete_summary(&v), "1 source stale");
        v.unavailable = 2;
        v.stale = 0;
        assert_eq!(incomplete_summary(&v), "2 sources unavailable");
    }

    // The rendered screens (healthy emptiness, busy chrome, layout stability,
    // width safety, compact fallback) are now verified against the real ratatui
    // draw in `crate::view`'s tests, not the retired string renderer.
}
