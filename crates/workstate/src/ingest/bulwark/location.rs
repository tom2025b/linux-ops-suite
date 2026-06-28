use serde_json::Value;

/// Flatten Bulwark's structured `location` tagged union into a display `String`.
///
/// `pub(crate)` so the adapter (its sibling `super`) can call it; it is an internal
/// seam, not part of Workstate's public API.
///
/// On the wire `location` is `{"type": "<tag>", ...more fields per tag}`:
///   * `"json_path"` — carries `"path": "<json path string>"` → emit the path
///   * `"byte_range"` — carries `"start"` and `"end"` integers → `"bytes S..E"`
///   * `"line"`       — carries `"line"` integer → `"line N"`
///   * `"unknown"` or any other tag (or no location at all) → `"unknown"`
///
/// TOTAL by construction: every code path returns a `String`, no panics, no
/// `unwrap`. Missing or unrecognized sub-fields fall back to `"unknown"` rather
/// than erroring — a sparse location is handled gracefully.
pub(crate) fn flatten_location(loc: Option<&Value>) -> String {
    // Extract the "type" discriminant. If there is no location, or no "type" key,
    // the whole match falls to the wildcard arm and returns "unknown".
    let tag = loc
        .and_then(|v| v.get("type"))
        .and_then(Value::as_str)
        .unwrap_or(""); // "" hits the wildcard → "unknown"

    match tag {
        "json_path" => {
            // Extract the path string from the "path" sibling key.
            // If it's absent or not a string, fall back to "unknown".
            loc.and_then(|v| v.get("path"))
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string()
        }
        "byte_range" => {
            // Extract integer start/end offsets; fall back to "unknown" if missing.
            let start = loc.and_then(|v| v.get("start")).and_then(Value::as_i64);
            let end = loc.and_then(|v| v.get("end")).and_then(Value::as_i64);
            match (start, end) {
                // Inline format args (no separate binding needed) — clippy-clean.
                (Some(s), Some(e)) => format!("bytes {s}..{e}"),
                // Partial data is still unusable as a display range.
                _ => "unknown".to_string(),
            }
        }
        "line" => {
            // Extract the line number; fall back to "unknown" if missing.
            let line = loc.and_then(|v| v.get("line")).and_then(Value::as_i64);
            match line {
                Some(n) => format!("line {n}"),
                None => "unknown".to_string(),
            }
        }
        // "unknown" tag OR any unrecognized/absent tag → display "unknown"
        _ => "unknown".to_string(),
    }
}

// =============================================================================
// Tests — unit tests for every render arm, co-located with the helper.
// =============================================================================
// These stay IN-CRATE because `flatten_location` is `pub(crate)` (an external
// integration test could not see it). The behavioral test that exercises location
// rendering THROUGH the public `normalize` lives in `tests/bulwark.rs`; these guard
// each arm directly.
#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises every render path of `flatten_location` in isolation, confirming
    /// the exact string output for each location type tag.
    #[test]
    fn location_flattening_covers_all_arms() {
        // json_path: emit the "path" string directly.
        let json_path = serde_json::json!({"type": "json_path", "path": "$.foo.bar"});
        assert_eq!(flatten_location(Some(&json_path)), "$.foo.bar");

        // byte_range: format as "bytes S..E".
        let byte_range = serde_json::json!({"type": "byte_range", "start": 10, "end": 20});
        assert_eq!(flatten_location(Some(&byte_range)), "bytes 10..20");

        // line: format as "line N".
        let line = serde_json::json!({"type": "line", "line": 99});
        assert_eq!(flatten_location(Some(&line)), "line 99");

        // Explicit "unknown" tag → "unknown".
        let unknown_tag = serde_json::json!({"type": "unknown"});
        assert_eq!(flatten_location(Some(&unknown_tag)), "unknown");

        // Unrecognized tag → "unknown".
        let bad_tag = serde_json::json!({"type": "invisible"});
        assert_eq!(flatten_location(Some(&bad_tag)), "unknown");

        // No location value at all → "unknown".
        assert_eq!(flatten_location(None), "unknown");

        // byte_range with only one of start/end → "unknown" (not a valid range).
        let partial_range = serde_json::json!({"type": "byte_range", "start": 5});
        assert_eq!(flatten_location(Some(&partial_range)), "unknown");

        // line with missing "line" key → "unknown".
        let no_line_num = serde_json::json!({"type": "line"});
        assert_eq!(flatten_location(Some(&no_line_num)), "unknown");
    }
}
