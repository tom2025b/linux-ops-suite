//! insta geometry snapshots: render chrome into a fixed TestBackend and snapshot the
//! glyph grid. Snapshots capture LAYOUT ONLY (insta's buffer Display ignores colour);
//! NO_COLOR/accent guarantees stay covered by the in-crate style-assertion tests.

use ratatui::backend::TestBackend;
use ratatui::widgets::Paragraph;
use ratatui::Terminal;
use thomas_tui::{pane, truncate_path, Theme};

/// Render a closure into a `w`×`h` TestBackend and return the glyph grid as a
/// newline-joined string (ratatui 0.29's `Buffer` has no `Display`, so we flatten
/// the cells ourselves). Layout/glyphs only — colour and style are not captured.
fn render_to_string<F: FnOnce(&mut ratatui::Frame)>(w: u16, h: u16, f: F) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(f).unwrap();
    let buf = term.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn snapshot_pane_frames_a_title() {
    let theme = Theme::with_color(true);
    let grid = render_to_string(24, 4, |f| {
        f.render_widget(pane("adapters", theme), f.area())
    });
    insta::assert_snapshot!(grid);
}

#[test]
fn snapshot_truncated_cjk_path_fits_a_narrow_pane() {
    // The R1 regression net: a wide-CJK path truncated to the pane's inner width
    // must not bleed past the right border. Snapshot the whole framed result; the
    // right-edge border column must stay intact.
    let theme = Theme::with_color(false);
    let grid = render_to_string(16, 3, |f| {
        let block = pane("p", theme);
        let inner = block.inner(f.area());
        f.render_widget(block, f.area());
        let text = truncate_path("/srv/日本語/データ/script.sh", inner.width as usize);
        f.render_widget(Paragraph::new(text), inner);
    });
    insta::assert_snapshot!(grid);
}
