//! Pure formatting utilities for Bulwark reports.
//!
//! This module is the single source of truth for small, reusable formatting
//! helpers used across human, JSON, and Markdown output:
//! - human_size: friendly byte counts
//! - md_escape: safe Markdown table cell escaping
//! - truncate_path / truncate_desc: Unicode-aware truncation
//!
//! All functions here are pure and allocation-minimal where possible.
//! They contain no terminal-specific logic (colors, alignment) — that lives
//! in the human table module.
//! Keeping them here means any future output format (HTML, TUI widgets, etc.)
//! can reuse the exact same truncation and sizing logic without duplication.

/// Escape the characters that would otherwise break a Markdown table cell.
///
/// A literal `|` becomes `\|`. Newlines and carriage returns are collapsed
/// to spaces because a Markdown table cell must stay on one line.
///
/// Everything else passes through unchanged.
///
/// This is the central escaping logic for all Markdown output.
///
/// # Examples
/// ```
/// use bulwark_core::core::report::md_escape;
///
/// // A pipe is escaped so it can't split a table cell.
/// assert_eq!(md_escape("a|b"), "a\\|b");
///
/// // Newlines (and CRs) collapse to spaces so the cell stays on one line.
/// assert_eq!(md_escape("line1\nline2"), "line1 line2");
///
/// // Ordinary text is untouched.
/// assert_eq!(md_escape("just text"), "just text");
/// ```
pub fn md_escape(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// Small human-readable byte-size formatter.
///
/// Converts bytes to short strings like "512 B", "1.5K", "12.3M".
/// Uses binary units (1024) and one decimal for larger sizes.
///
/// This is intentionally tiny and dependency-free.
///
/// # Examples
/// ```
/// use bulwark_core::core::report::human_size;
///
/// // Bytes under 1 KiB keep the "N B" form (with a space).
/// assert_eq!(human_size(0), "0 B");
/// assert_eq!(human_size(512), "512 B");
///
/// // 1024 bytes rolls over to one decimal place of the next unit.
/// assert_eq!(human_size(1024), "1.0K");
/// assert_eq!(human_size(1536), "1.5K");
/// assert_eq!(human_size(1_048_576), "1.0M");
/// ```
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut size = bytes as f64;
    let mut u = 0;
    while size >= 1024.0 && u < UNITS.len() - 1 {
        size /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} {}", UNITS[u])
    } else {
        format!("{size:.1}{}", UNITS[u])
    }
}

/// Truncate a path, keeping the filename (tail) visible.
///
/// Always operates on Unicode characters (`.chars()`), never raw bytes.
/// This prevents panics on multi-byte UTF-8 and guarantees correct visual width.
///
/// We keep the end of the path because the filename is usually the most
/// useful part when reviewing an inventory.
///
/// # Examples
/// ```
/// use bulwark_core::core::report::truncate_path;
///
/// // Short paths are returned unchanged.
/// assert_eq!(truncate_path("/tmp/tool.sh", 48), "/tmp/tool.sh");
///
/// // Long paths keep the tail (the filename) and gain a leading ellipsis.
/// let long = "/very/deeply/nested/directory/structure/backup-tool.sh";
/// let out = truncate_path(long, 20);
/// assert!(out.starts_with('…'));
/// assert!(out.ends_with("backup-tool.sh"));
/// assert_eq!(out.chars().count(), 20);
///
/// // Degenerate widths never panic: max 0 yields "", max 1 yields just "…".
/// assert_eq!(truncate_path("/tmp/tool.sh", 0), "");
/// assert_eq!(truncate_path("/tmp/tool.sh", 1), "…");
/// ```
pub fn truncate_path(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        return s.to_string();
    }
    // Guard the degenerate widths so `max - 1` can never underflow `usize`
    // (which would panic in debug and silently wrap to a giant `skip` in
    // release). With `max == 0` there is no room for anything; with `max == 1`
    // only the ellipsis fits.
    if max == 0 {
        return String::new();
    }
    let tail: String = s.chars().skip(char_count - (max - 1)).collect();
    format!("…{tail}")
}

/// Truncate a description to fit in the table.
///
/// Operates on characters for Unicode safety.
/// Uses `saturating_sub(3)` to avoid underflow on very small widths.
///
/// # Examples
/// ```
/// use bulwark_core::core::report::truncate_desc;
///
/// // Fits within the limit (after trimming) → returned as-is.
/// assert_eq!(truncate_desc("  backs up the NAS  ", 40), "backs up the NAS");
///
/// // Too long → truncated to `max` chars with a trailing ellipsis.
/// let out = truncate_desc("this description is definitely too long to fit", 20);
/// assert!(out.ends_with('…'));
/// assert_eq!(out.chars().count(), 18);
/// ```
pub fn truncate_desc(s: &str, max: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        let head: String = trimmed.chars().take(max.saturating_sub(3)).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md_escape_escapes_pipes_and_collapses_newlines() {
        assert_eq!(md_escape("a|b"), "a\\|b");
        assert_eq!(md_escape("line1\nline2"), "line1 line2");
        assert_eq!(md_escape("crlf\r\nhere"), "crlf  here");
        assert_eq!(md_escape("just text"), "just text");
    }

    #[test]
    fn human_size_formats_reasonably() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1.0K");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(1_048_576), "1.0M");
    }

    #[test]
    fn truncation_is_utf8_safe_and_never_panics_mid_codepoint() {
        let desc = format!("{}é and yet more trailing text", "a".repeat(36));
        let out = truncate_desc(&desc, 40);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 38);

        let path = format!("/home/user/工具/{}/café-🚀-tool.sh", "x".repeat(60));
        let out = truncate_path(&path, 48);
        assert!(out.starts_with('…'));
        assert_eq!(out.chars().count(), 48);

        assert_eq!(truncate_desc("café ☕", 40), "café ☕");
        assert_eq!(truncate_path("/tmp/é", 48), "/tmp/é");
    }

    #[test]
    fn truncate_path_handles_degenerate_widths_without_panicking() {
        // Regression: `max == 0` underflowed `char_count - (max - 1)`, panicking
        // in debug. `truncate_path` is public in the library, so a downstream
        // caller could hit it. Tiny widths must degrade, never panic.
        assert_eq!(truncate_path("/tmp/tool.sh", 0), "");
        assert_eq!(truncate_path("/tmp/tool.sh", 1), "…");
        assert_eq!(truncate_path("/tmp/tool.sh", 2).chars().count(), 2);
        // An empty input trivially "fits" any width and is returned unchanged.
        assert_eq!(truncate_path("", 0), "");
        // A non-empty string with max 0 has no room for even the ellipsis.
        assert_eq!(truncate_path("ab", 0), "");
    }
}
