//! Display-width-aware string truncation, shared so every caller truncates the
//! same way with the same ellipsis.
//!
//! Two shapes tables and status lines need: keep the **end** of a path (the
//! filename is the useful part when scanning an inventory), or keep the
//! **start** of a description. Both measure by **display column** (UAX#11), not
//! by `char` or byte, and both iterate **grapheme clusters** — so a 2-column CJK
//! ideograph or emoji counts as 2, a zero-width combining mark as 0, and a
//! multi-codepoint grapheme (ZWJ emoji, base + combining mark) is kept or dropped
//! whole, never split. Both saturate on tiny widths rather than panicking.
//!
//! Measuring columns (not chars) is what keeps truncated text inside the cell
//! budget it was given: a string of N wide chars is 2N columns, so char-counting
//! would overflow the column it was asked to fit and corrupt a table's layout.
//! This uses `unicode-width` + `unicode-segmentation` — the same crates ratatui
//! uses internally for its own column math — so a truncated cell lines up with
//! how ratatui will actually render it.
//!
//! These are plain `String` helpers — no `Theme`, no rendering. A renderer
//! truncates the text first, then styles the result.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The single ellipsis glyph truncation marks the cut with — one `…`, not three
/// ASCII dots, so truncated cells line up to one column regardless of caller.
const ELLIPSIS: char = '…';

/// Truncate `s` to at most `max` display COLUMNS, keeping the **end** and marking
/// the cut with a leading ellipsis. For paths, where the filename (the tail) is
/// the part worth keeping.
///
/// Display-width-aware (UAX#11) and grapheme-safe: a 2-column CJK ideograph or
/// emoji counts as 2, a zero-width combining mark as 0, and a multi-codepoint
/// grapheme (ZWJ emoji, base + combining) is kept or dropped whole, never split.
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

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn short_input_is_returned_unchanged() {
        assert_eq!(truncate_path("/tmp/tool.sh", 48), "/tmp/tool.sh");
        assert_eq!(truncate_desc("backs up the NAS", 40), "backs up the NAS");
        // Exactly `max` is still unchanged (no ellipsis when it already fits).
        assert_eq!(truncate_path("abcde", 5), "abcde");
        assert_eq!(truncate_desc("abcde", 5), "abcde");
    }

    #[test]
    fn truncate_path_keeps_the_tail_within_the_column_budget() {
        let long = "/very/deeply/nested/directory/structure/backup-tool.sh";
        let out = truncate_path(long, 20);
        assert!(out.starts_with('…'), "leading ellipsis marks the cut");
        assert!(out.ends_with("backup-tool.sh"), "the filename tail is kept");
        assert!(out.width() <= 20, "stays within the column budget");
    }

    #[test]
    fn truncate_desc_keeps_the_head_trims_within_the_column_budget() {
        let out = truncate_desc("this description is definitely too long to fit", 20);
        assert!(out.ends_with('…'), "trailing ellipsis marks the cut");
        assert!(out.starts_with("this description"), "the head is kept");
        assert!(out.width() <= 20, "stays within the column budget");
        // Whitespace is trimmed before the length check.
        assert_eq!(truncate_desc("   hello   ", 40), "hello");
    }

    #[test]
    fn ellipsis_is_the_single_glyph_not_three_dots() {
        // The whole point of sharing this: one `…`, never "..." — so cells from
        // different tools line up to one column.
        let p = truncate_path("/a/very/long/path/that/keeps/going/file.sh", 12);
        let d = truncate_desc("a description that is far too long to ever fit here", 12);
        assert!(p.starts_with('…') && !p.starts_with("..."));
        assert!(d.ends_with('…') && !d.ends_with("..."));
    }

    #[test]
    fn unicode_boundaries_are_never_split() {
        // Multi-byte chars on both sides of the cut: measured by column, never split.
        let path = "/tmp/café-résumé/être/naïve/tool.sh";
        let out = truncate_path(path, 18);
        assert!(out.width() <= 18);
        assert!(out.starts_with('…'));
        // An all-multibyte description.
        let desc = "ééééééééééééééééééééééééé";
        let out = truncate_desc(desc, 10);
        assert!(out.width() <= 10);
        assert!(out.ends_with('…'));
        // Already-short multibyte input is untouched.
        assert_eq!(truncate_desc("café ☕", 40), "café ☕");
        assert_eq!(truncate_path("/tmp/é", 48), "/tmp/é");
    }

    #[test]
    fn tiny_widths_saturate_and_never_panic() {
        // max == 0 → empty, not a panic from `max - 1`.
        assert_eq!(truncate_path("/long/path/here", 0), "");
        assert_eq!(truncate_desc("long text here", 0), "");
        // max == 1 → just the ellipsis (head/tail of zero columns).
        assert_eq!(truncate_path("/long/path/here", 1), "…");
        assert_eq!(truncate_desc("long text here", 1), "…");
    }
}
