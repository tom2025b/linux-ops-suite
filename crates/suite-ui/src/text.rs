//! Unicode-aware string truncation, shared so every tool truncates the same way
//! with the same ellipsis.
//!
//! Two shapes the suite's tables and status lines need: keep the **end** of a
//! path (the filename is the useful part when scanning an inventory), or keep the
//! **start** of a description. Both count by `char`, never by byte, so a
//! multi-byte character is never split mid-codepoint, and both saturate on tiny
//! widths rather than panicking.
//!
//! These are plain `String` helpers — no `Theme`, no rendering. A renderer
//! truncates the text first, then styles the result.

/// The single ellipsis glyph the whole suite truncates with — one `…`, not three
/// ASCII dots, so truncated cells line up to one column regardless of tool.
const ELLIPSIS: char = '…';

/// Truncate `s` to at most `max` characters, keeping the **end** and marking the
/// cut with a leading [`ELLIPSIS`]. For paths, where the filename (the tail) is
/// the part worth keeping.
///
/// Char-counted, so Unicode is safe. Input of `max` chars or fewer is returned
/// unchanged; a longer input yields exactly `max` chars: the ellipsis plus the
/// last `max - 1`. `max == 0` yields an empty string.
///
/// ```
/// use suite_ui::truncate_path;
/// assert_eq!(truncate_path("/tmp/tool.sh", 48), "/tmp/tool.sh");
/// let out = truncate_path("/very/deeply/nested/dir/backup-tool.sh", 20);
/// assert!(out.starts_with('…'));
/// assert!(out.ends_with("backup-tool.sh"));
/// assert_eq!(out.chars().count(), 20);
/// ```
pub fn truncate_path(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    // Keep the last `max - 1` chars; the ellipsis takes the first column.
    let tail: String = s.chars().skip(count - (max - 1)).collect();
    format!("{ELLIPSIS}{tail}")
}

/// Truncate `s` to at most `max` characters, keeping the **start** and marking
/// the cut with a trailing [`ELLIPSIS`]. For descriptions and other
/// read-left-to-right text. Leading/trailing whitespace is trimmed first.
///
/// Char-counted. Input of `max` chars or fewer (after trimming) is returned
/// as-is; a longer input is the first `max - 1` chars plus the ellipsis, `max`
/// chars total. `max == 0` yields an empty string.
///
/// ```
/// use suite_ui::truncate_desc;
/// assert_eq!(truncate_desc("  backs up the NAS  ", 40), "backs up the NAS");
/// let out = truncate_desc("this description is definitely too long to fit", 20);
/// assert!(out.ends_with('…'));
/// assert_eq!(out.chars().count(), 20);
/// ```
pub fn truncate_desc(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let head: String = trimmed.chars().take(max - 1).collect();
    format!("{head}{ELLIPSIS}")
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_input_is_returned_unchanged() {
        assert_eq!(truncate_path("/tmp/tool.sh", 48), "/tmp/tool.sh");
        assert_eq!(truncate_desc("backs up the NAS", 40), "backs up the NAS");
        // Exactly `max` is still unchanged (no ellipsis when it already fits).
        assert_eq!(truncate_path("abcde", 5), "abcde");
        assert_eq!(truncate_desc("abcde", 5), "abcde");
    }

    #[test]
    fn truncate_path_keeps_the_tail_and_hits_max_exactly() {
        let long = "/very/deeply/nested/directory/structure/backup-tool.sh";
        let out = truncate_path(long, 20);
        assert!(out.starts_with('…'), "leading ellipsis marks the cut");
        assert!(out.ends_with("backup-tool.sh"), "the filename tail is kept");
        assert_eq!(out.chars().count(), 20, "yields exactly max chars");
    }

    #[test]
    fn truncate_desc_keeps_the_head_trims_and_hits_max_exactly() {
        let out = truncate_desc("this description is definitely too long to fit", 20);
        assert!(out.ends_with('…'), "trailing ellipsis marks the cut");
        assert!(out.starts_with("this description"), "the head is kept");
        assert_eq!(out.chars().count(), 20, "yields exactly max chars");
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
        // Multi-byte chars on both sides of the cut: count by char, not byte.
        let path = "/tmp/café-résumé/être/naïve/tool.sh";
        let out = truncate_path(path, 18);
        assert_eq!(out.chars().count(), 18);
        assert!(out.starts_with('…'));
        // An all-multibyte description.
        let desc = "ééééééééééééééééééééééééé";
        let out = truncate_desc(desc, 10);
        assert_eq!(out.chars().count(), 10);
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
        // max == 1 → just the ellipsis (head/tail of zero chars).
        assert_eq!(truncate_path("/long/path/here", 1), "…");
        assert_eq!(truncate_desc("long text here", 1), "…");
    }
}
