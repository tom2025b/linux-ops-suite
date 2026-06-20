//! Human-readable formatting helpers.

/// Format a byte count as a short human-readable size: `563 B`, `2.1 KB`,
/// `3.4 MB`. Base-1024, one decimal place above the byte threshold.
///
/// This is the form rewind and tripwire already used. rex-check previously
/// printed single-letter units (`K`/`M`/…); it is standardized onto this.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut val = bytes as f64;
    let mut unit = 0;
    while val >= 1024.0 && unit < UNITS.len() - 1 {
        val /= 1024.0;
        unit += 1;
    }
    format!("{val:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_below_a_kib_are_plain() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1), "1 B");
        assert_eq!(human_size(1023), "1023 B");
    }

    #[test]
    fn scales_through_the_units() {
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1024 * 1024), "1.0 MB");
        assert_eq!(human_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(human_size(1024u64.pow(4)), "1.0 TB");
    }

    #[test]
    fn very_large_values_stay_in_the_top_unit() {
        // Past TB we keep scaling the number rather than inventing PB.
        assert!(human_size(5 * 1024u64.pow(4)).ends_with(" TB"));
    }
}
