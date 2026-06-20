//! Pulse's input layer plus the legacy terminal driver it is migrating off.
//!
//! Production now drives the terminal through `suite_ui::Tui` (raw mode + alt
//! screen + a restoring panic hook, all via crossterm), and reads input through
//! [`read_event`], which maps crossterm key events onto Pulse's [`Key`] enum.
//!
//! The original hand-rolled driver below — `RawMode` (libc `termios` behind an
//! `extern "C"` block), the ANSI [`paint`], `install_panic_guard`, and the
//! byte-stream [`read_key`] decoder — is **retained only for its unit tests**
//! during the suite-ui migration and is scheduled for removal in step T10. It is
//! no longer wired into the loop, hence the module-scoped `dead_code` allow.
#![allow(dead_code)]

use std::io::{self, Read, Write};

// ─────────────────────────────────────────────────────────────────────────────
// Raw mode guard
// ─────────────────────────────────────────────────────────────────────────────

/// Owns the terminal's interactive state for the lifetime of the loop. Created
/// with [`RawMode::enter`]; restores everything on drop.
pub struct RawMode {
    /// The original termios to restore. `None` if we never managed to read one
    /// (then drop is a no-op for termios), but ANSI teardown still runs.
    original: Option<Termios>,
}

impl RawMode {
    /// Enter raw mode + alternate screen and hide the cursor. Returns the guard;
    /// hold it for as long as interactive mode runs. If reading the current
    /// termios fails (e.g. stdin isn't a tty), raw mode is skipped but the guard
    /// is still returned so teardown is symmetric.
    pub fn enter() -> io::Result<Self> {
        let original = tcgetattr(STDIN_FILENO).ok();
        if let Some(orig) = &original {
            let mut raw = *orig;
            make_raw(&mut raw);
            // TCSAFLUSH: apply after draining pending output, discarding pending
            // input — a clean switch into raw mode.
            tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw)?;
        }
        // Alt-screen + hide cursor. Doing this after raw mode so a failure above
        // doesn't leave us on the alt-screen.
        let mut out = io::stdout();
        out.write_all(b"\x1b[?1049h\x1b[?25l")?;
        out.flush()?;
        Ok(RawMode { original })
    }

    /// Restore the terminal explicitly. Idempotent with `Drop`; called by both
    /// the normal exit path and the panic hook.
    fn restore(&mut self) {
        let mut out = io::stdout();
        // Show cursor, leave alt-screen.
        let _ = out.write_all(b"\x1b[?25h\x1b[?1049l");
        let _ = out.flush();
        if let Some(orig) = self.original.take() {
            let _ = tcsetattr(STDIN_FILENO, TCSAFLUSH, &orig);
        }
    }

    /// Hand the real terminal to a foreground child for the duration of `body`,
    /// then take it back. Leaves raw mode + the alt-screen (so the child runs on
    /// a normal cooked terminal it fully owns), runs `body`, and re-enters raw
    /// mode + the alt-screen afterwards. Re-entry runs even when `body` returns
    /// an error, so Pulse is never left half-suspended — the same guarantee
    /// `suite_ui::Tui::suspended` gives the crossterm TUIs in the suite.
    ///
    /// Used to launch the RexOps cockpit (`rexops tui`) from inside Pulse: the
    /// cockpit needs the real terminal in its own raw/alt-screen mode, so Pulse
    /// must step fully out of the way first.
    pub fn suspend<T>(&mut self, body: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
        // Step out, IN PLACE: leave the alt-screen + show the cursor, and restore
        // the original termios — but keep `self.original` so re-entry can put raw
        // mode back. (We deliberately don't call `restore()`, which *takes*
        // `original` and would make re-entry impossible.)
        {
            let mut out = io::stdout();
            out.write_all(b"\x1b[?25h\x1b[?1049l")?;
            out.flush()?;
        }
        if let Some(orig) = &self.original {
            tcsetattr(STDIN_FILENO, TCSAFLUSH, orig)?;
        }

        // Run the child. Capture the result so we always attempt re-entry before
        // returning.
        let result = body();

        // Step back in: raw mode (if we had a termios) + alt-screen + hide
        // cursor, mirroring `enter`.
        let reenter = (|| -> io::Result<()> {
            if let Some(orig) = &self.original {
                let mut raw = *orig;
                make_raw(&mut raw);
                tcsetattr(STDIN_FILENO, TCSAFLUSH, &raw)?;
            }
            let mut out = io::stdout();
            out.write_all(b"\x1b[?1049h\x1b[?25l")?;
            out.flush()
        })();

        // A genuine child error wins; otherwise surface any re-entry failure.
        match (result, reenter) {
            (Err(e), _) => Err(e),
            (Ok(_), Err(e)) => Err(e),
            (Ok(v), Ok(())) => Ok(v),
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Install a panic hook that best-effort restores the terminal before the
/// default hook prints the panic. Called once before entering raw mode so a
/// panic inside the loop can't leave the shell in raw/alt-screen state.
pub fn install_panic_guard() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = io::stdout();
        let _ = out.write_all(b"\x1b[?25h\x1b[?1049l");
        let _ = out.flush();
        // Best-effort termios restore via a fresh, sane setting: cooked mode.
        if let Ok(mut t) = tcgetattr(STDIN_FILENO) {
            make_cooked(&mut t);
            let _ = tcsetattr(STDIN_FILENO, TCSAFLUSH, &t);
        }
        prev(info);
    }));
}

// ─────────────────────────────────────────────────────────────────────────────
// Keys
// ─────────────────────────────────────────────────────────────────────────────

/// The keys Pulse acts on. Anything else is ignored by the loop (or, in the
/// search box, `Char` carries the typed letter).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    /// Stream closed (Ctrl-D / EOF) — treated as "quit".
    Eof,
    /// A byte/sequence we don't model; the loop ignores it.
    Other,
}

/// Read one key from stdin (blocking). Decodes:
///   - `\r` / `\n` → Enter
///   - `0x7f` / `0x08` → Backspace
///   - `0x1b` → Esc, *and* swallows a following CSI/SS3 sequence (arrow keys
///     etc.) so a stray arrow doesn't get read as Esc + letters.
///   - `0x04` (Ctrl-D) / EOF → Eof
///   - a UTF-8 char → Char
pub fn read_key(input: &mut impl Read) -> io::Result<Key> {
    let mut b0 = [0u8; 1];
    if input.read(&mut b0)? == 0 {
        return Ok(Key::Eof);
    }
    let b = b0[0];
    match b {
        b'\r' | b'\n' => Ok(Key::Enter),
        0x7f | 0x08 => Ok(Key::Backspace),
        0x04 => Ok(Key::Eof),
        0x1b => {
            // Could be a lone Esc, or the start of an escape sequence. Peek one
            // more byte; if it's '[' or 'O', consume the rest of the sequence
            // (until a final byte in 0x40..=0x7e) and report Other.
            let mut b1 = [0u8; 1];
            if input.read(&mut b1)? == 0 {
                return Ok(Key::Esc);
            }
            if b1[0] == b'[' || b1[0] == b'O' {
                let mut t = [0u8; 1];
                while input.read(&mut t)? == 1 {
                    if (0x40..=0x7e).contains(&t[0]) {
                        break;
                    }
                }
                Ok(Key::Other)
            } else {
                // Esc followed by a normal byte: treat as Esc (Alt-combos aren't
                // used by Pulse). The trailing byte is dropped.
                Ok(Key::Esc)
            }
        }
        // Printable ASCII.
        0x20..=0x7e => Ok(Key::Char(b as char)),
        // Multi-byte UTF-8: gather the continuation bytes and decode.
        _ => decode_utf8(b, input),
    }
}

/// Block on the next key from crossterm, mapped onto Pulse's [`Key`] vocabulary.
///
/// This is the production input source now that the terminal is driven by
/// `suite_ui::Tui` (raw mode + alt screen via crossterm). It produces the exact
/// same [`Key`] values [`read_key`] did, so the pure state machine in
/// [`crate::app`] — and all of its tests — stay untouched; only the *source* of a
/// key changed from a byte stream to crossterm events. Non-key events (resize,
/// mouse, focus) are skipped so the caller only ever sees a real keypress, and
/// crossterm already decodes CSI/SS3 sequences (arrows, function keys), so the
/// hand-rolled escape-swallowing in [`read_key`] is not needed in production (it
/// stays for its unit tests until the byte path is removed in a later step).
pub fn read_event() -> io::Result<Key> {
    use crossterm::event::{self, Event, KeyEventKind};
    loop {
        match event::read()? {
            Event::Key(k) => {
                // Act only on key *press* — terminals (and Windows) can emit
                // Release/Repeat too, and a release must not double-fire a step.
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                return Ok(map_key(k.code, k.modifiers));
            }
            // Resize/mouse/focus/paste: the loop repaints per key and ratatui
            // re-reads the size on the next draw, so just wait for real input.
            _ => continue,
        }
    }
}

/// Map a crossterm key (code + modifiers) onto Pulse's [`Key`]. Ctrl-D and Ctrl-C
/// both mean "leave" → [`Key::Eof`] (the loop treats it as quit), matching the
/// old byte reader where `0x04` (Ctrl-D) was EOF.
fn map_key(code: crossterm::event::KeyCode, mods: crossterm::event::KeyModifiers) -> Key {
    use crossterm::event::{KeyCode, KeyModifiers};
    if mods.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char('d') | KeyCode::Char('c') = code {
            return Key::Eof;
        }
    }
    match code {
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Char(c) => Key::Char(c),
        // Arrows, function keys, Tab, etc. aren't in Pulse's keymap — the loop
        // ignores Other, exactly as the byte reader reported such sequences.
        _ => Key::Other,
    }
}

/// Decode a UTF-8 char whose leading byte is `b0`, reading continuation bytes
/// from `input`. Returns `Key::Other` on any malformed sequence.
fn decode_utf8(b0: u8, input: &mut impl Read) -> io::Result<Key> {
    let len = match b0 {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => return Ok(Key::Other),
    };
    let mut buf = [0u8; 4];
    buf[0] = b0;
    for slot in buf.iter_mut().take(len).skip(1) {
        let mut c = [0u8; 1];
        if input.read(&mut c)? == 0 {
            return Ok(Key::Other);
        }
        *slot = c[0];
    }
    match std::str::from_utf8(&buf[..len]) {
        Ok(s) => Ok(s.chars().next().map(Key::Char).unwrap_or(Key::Other)),
        Err(_) => Ok(Key::Other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Screen helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Clear the screen and home the cursor, then write `frame`, as one flush. Used
/// to repaint between key presses without flicker.
pub fn paint(frame: &str) -> io::Result<()> {
    let mut out = io::stdout();
    out.write_all(b"\x1b[2J\x1b[H")?;
    out.write_all(frame.as_bytes())?;
    out.flush()
}

// ─────────────────────────────────────────────────────────────────────────────
// libc termios  (hand-rolled extern "C", no libc crate — see rex-check/main.rs)
// ─────────────────────────────────────────────────────────────────────────────

const STDIN_FILENO: i32 = 0;
const TCSAFLUSH: i32 = 2; // <termios.h>

// `struct termios` is platform-defined. On Linux/glibc it is:
//   tcflag_t c_iflag,c_oflag,c_cflag,c_lflag;  (u32 each)
//   cc_t c_line;                               (u8)
//   cc_t c_cc[NCCS];                           (NCCS = 32 on Linux)
//   speed_t c_ispeed, c_ospeed;                (u32 each)
// We mirror that layout exactly so tcgetattr/tcsetattr read/write the right
// bytes. Pulse targets Linux (the whole suite is Linux-only).
const NCCS: usize = 32;

#[repr(C)]
#[derive(Clone, Copy)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; NCCS],
    c_ispeed: u32,
    c_ospeed: u32,
}

// termios flag bits we touch (octal, from <termios.h> on Linux).
const ICANON: u32 = 0o0000002;
const ECHO: u32 = 0o0000010;
const ISIG: u32 = 0o0000001;
const IEXTEN: u32 = 0o0100000;
const IXON: u32 = 0o0002000;
const ICRNL: u32 = 0o0000400;
const BRKINT: u32 = 0o0000002;
const INPCK: u32 = 0o0000020;
const ISTRIP: u32 = 0o0000040;
const OPOST: u32 = 0o0000001;
// c_cc indices.
const VMIN: usize = 6;
const VTIME: usize = 5;

/// Turn `t` into raw mode in place (the classic cfmakeraw bit pattern), then set
/// a blocking single-byte read (VMIN=1, VTIME=0).
fn make_raw(t: &mut Termios) {
    t.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
    t.c_oflag &= !OPOST;
    t.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
    t.c_cc[VMIN] = 1;
    t.c_cc[VTIME] = 0;
}

/// Restore canonical ("cooked") input: echo + line editing + signals on. Used by
/// the panic hook as a sane fallback when the original termios isn't available.
fn make_cooked(t: &mut Termios) {
    t.c_iflag |= ICRNL | IXON;
    t.c_oflag |= OPOST;
    t.c_lflag |= ECHO | ICANON | IEXTEN | ISIG;
}

fn tcgetattr(fd: i32) -> io::Result<Termios> {
    extern "C" {
        fn tcgetattr(fd: i32, termios_p: *mut Termios) -> i32;
    }
    // SAFETY: tcgetattr fills a correctly-sized, aligned Termios we own.
    let mut t = unsafe { std::mem::zeroed::<Termios>() };
    let rc = unsafe { tcgetattr(fd, &mut t) };
    if rc == 0 {
        Ok(t)
    } else {
        Err(io::Error::last_os_error())
    }
}

fn tcsetattr(fd: i32, actions: i32, t: &Termios) -> io::Result<()> {
    extern "C" {
        fn tcsetattr(fd: i32, optional_actions: i32, termios_p: *const Termios) -> i32;
    }
    // SAFETY: reads a valid Termios we own; writes nothing.
    let rc = unsafe { tcsetattr(fd, actions, t) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed bytes through `read_key` from an in-memory cursor — no real tty
    /// needed, so the decoder is fully unit-testable.
    fn keys(bytes: &[u8]) -> Vec<Key> {
        let mut cur = io::Cursor::new(bytes.to_vec());
        let mut out = Vec::new();
        loop {
            match read_key(&mut cur).unwrap() {
                Key::Eof => break,
                k => out.push(k),
            }
            // Cursor returns 0 at end → next read_key yields Eof, ending the loop.
        }
        out
    }

    #[test]
    fn decodes_plain_keys() {
        assert_eq!(
            keys(b"af/?q"),
            vec![
                Key::Char('a'),
                Key::Char('f'),
                Key::Char('/'),
                Key::Char('?'),
                Key::Char('q'),
            ]
        );
    }

    #[test]
    fn decodes_enter_and_backspace() {
        assert_eq!(keys(b"\r"), vec![Key::Enter]);
        assert_eq!(keys(b"\n"), vec![Key::Enter]);
        assert_eq!(keys(&[0x7f]), vec![Key::Backspace]);
        assert_eq!(keys(&[0x08]), vec![Key::Backspace]);
    }

    #[test]
    fn lone_escape_is_esc() {
        assert_eq!(keys(&[0x1b]), vec![Key::Esc]);
    }

    #[test]
    fn arrow_key_sequence_is_swallowed_as_other_not_esc_plus_letters() {
        // ESC [ A  (up arrow) must NOT decode as Esc, '[', 'A'.
        assert_eq!(keys(&[0x1b, b'[', b'A']), vec![Key::Other]);
        // ESC O P (F1 in SS3) likewise.
        assert_eq!(keys(&[0x1b, b'O', b'P']), vec![Key::Other]);
    }

    #[test]
    fn ctrl_d_is_eof() {
        // Ctrl-D mid-stream ends the loop in `keys`, so it yields nothing after.
        assert_eq!(keys(&[0x04]), vec![]);
    }

    #[test]
    fn decodes_multibyte_utf8() {
        // "é" is 0xc3 0xa9.
        assert_eq!(keys("é".as_bytes()), vec![Key::Char('é')]);
    }

    #[test]
    fn crossterm_keys_map_to_the_same_key_vocabulary() {
        use crossterm::event::{KeyCode, KeyModifiers};
        let none = KeyModifiers::NONE;
        // The keys Pulse's keymap acts on map straight across.
        assert_eq!(map_key(KeyCode::Enter, none), Key::Enter);
        assert_eq!(map_key(KeyCode::Esc, none), Key::Esc);
        assert_eq!(map_key(KeyCode::Backspace, none), Key::Backspace);
        assert_eq!(map_key(KeyCode::Char('q'), none), Key::Char('q'));
        assert_eq!(map_key(KeyCode::Char('/'), none), Key::Char('/'));
        // Ctrl-D and Ctrl-C both quit (EOF), like 0x04 in the old byte reader.
        assert_eq!(map_key(KeyCode::Char('d'), KeyModifiers::CONTROL), Key::Eof);
        assert_eq!(map_key(KeyCode::Char('c'), KeyModifiers::CONTROL), Key::Eof);
        // Arrows / function keys aren't in the keymap → Other (ignored), exactly
        // as the byte reader reported a swallowed CSI/SS3 sequence.
        assert_eq!(map_key(KeyCode::Up, none), Key::Other);
        assert_eq!(map_key(KeyCode::F(1), none), Key::Other);
        assert_eq!(map_key(KeyCode::Tab, none), Key::Other);
    }

    #[test]
    fn raw_and_cooked_flags_are_inverses_on_the_lflags() {
        let mut t: Termios = unsafe { std::mem::zeroed() };
        make_cooked(&mut t);
        assert!(t.c_lflag & ICANON != 0 && t.c_lflag & ECHO != 0);
        make_raw(&mut t);
        assert!(t.c_lflag & ICANON == 0 && t.c_lflag & ECHO == 0);
        assert_eq!(t.c_cc[VMIN], 1);
        assert_eq!(t.c_cc[VTIME], 0);
    }
}
