// ============================================================================
// crates/scriptvault-core/src/query/parse.rs
// ============================================================================
// parse_query — turn a free-text query string into a structured `Query`.
//
// Grammar (deliberately fzf/Raycast-style, NOT a heavy DSL):
//
//   deploy prod          two fuzzy terms (engine ANDs them across fields)
//   t:ci c:ops deploy    tag=ci AND category=ops, fuzzy "deploy"
//   lang:bash backup     only bash, fuzzy "backup"
//   fav:                 favorites-only filter
//   risk:high            only risk:high entries
//   -t:wip deploy        EXCLUDE tag wip, fuzzy "deploy"
//
// Operators are tokens of the form `key:value`; a leading `-` negates the next
// operator; everything else is fuzzy text. Aliases: t:/tag:, c:/category:.
// The parser is FORGIVING by design — an unrecognized `key:value` (e.g.
// `foo:bar`) or an unparseable `lang:`/`risk:` value is treated as plain fuzzy
// text rather than an error, mirroring the degrade-don't-fail posture of the
// rest of core. This is the SAME parser the CLI uses, so headless search finally
// gets the same filtering the TUI has.
// ============================================================================

use crate::model::Language;

use super::{Filter, Query, RiskLevel};

/// Parse a free-text query into a structured [`Query`]. Never fails: anything it
/// can't interpret as an operator becomes part of the fuzzy `text`.
pub fn parse_query(input: &str) -> Query {
    let mut filters: Vec<Filter> = Vec::new();
    let mut text_terms: Vec<&str> = Vec::new();

    for token in input.split_whitespace() {
        match parse_operator(token) {
            // Recognized operator → add the filter (de-duplicated). The original
            // token is consumed (not echoed into the fuzzy text).
            Some(filter) => {
                if !filters.contains(&filter) {
                    filters.push(filter);
                }
            }
            // Not an operator → it's part of the fuzzy text, verbatim.
            None => text_terms.push(token),
        }
    }

    Query {
        text: text_terms.join(" "),
        filters,
        ..Default::default()
    }
}

/// Remove the LAST operator token from a raw query string, returning the new
/// string. Plain fuzzy-text tokens are preserved; only the final recognized
/// operator (`t:ci`, `lang:bash`, `fav:`, `-t:wip`, …) is dropped. If the query
/// has no operator, the string is returned unchanged. Powers the TUI's
/// "Backspace on empty text pops the last chip" affordance; in core so the chip
/// model and its removal stay together (and a GUI can reuse it).
pub fn pop_last_filter_token(input: &str) -> String {
    let tokens: Vec<&str> = input.split_whitespace().collect();
    // Find the last token that is a recognized operator.
    let last_op = tokens.iter().rposition(|t| parse_operator(t).is_some());
    match last_op {
        Some(idx) => {
            let kept: Vec<&str> = tokens
                .iter()
                .enumerate()
                .filter_map(|(i, t)| (i != idx).then_some(*t))
                .collect();
            kept.join(" ")
        }
        None => input.to_string(),
    }
}

/// Try to interpret a single whitespace-delimited token as a structured
/// operator. Returns `None` when the token is plain fuzzy text (the forgiving
/// path: unknown keys, empty values, and unparseable `lang:`/`risk:` values all
/// fall back to text rather than erroring).
fn parse_operator(token: &str) -> Option<Filter> {
    // Negation: only `-t:`/`-tag:` is defined. Anything else starting with `-`
    // (e.g. `-c:ops`) is NOT a recognized negation, so it stays plain text.
    if let Some(rest) = token.strip_prefix('-') {
        let (key, value) = rest.split_once(':')?;
        if matches!(key.to_ascii_lowercase().as_str(), "t" | "tag") && !value.is_empty() {
            return Some(Filter::NotTag(value.to_ascii_lowercase()));
        }
        return None;
    }

    let (key, value) = token.split_once(':')?;
    let key = key.to_ascii_lowercase();

    // `fav:` is the one valueless operator (a filter with no argument).
    if matches!(key.as_str(), "fav" | "favorite" | "favourite") {
        return Some(Filter::Favorite);
    }

    // Every other operator REQUIRES a non-empty value; a half-typed `t:` is
    // treated as plain text so it doesn't silently filter to nothing.
    if value.is_empty() {
        return None;
    }

    match key.as_str() {
        "t" | "tag" => Some(Filter::Tag(value.to_ascii_lowercase())),
        "c" | "category" => Some(Filter::Category(value.to_ascii_lowercase())),
        // `lang:`/`risk:` only become filters when the value PARSES; otherwise
        // the token falls back to fuzzy text (returns None).
        "lang" | "language" => Language::from_label(value).map(Filter::Lang),
        "risk" => RiskLevel::from_label(value).map(Filter::Risk),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_the_fuzzy_text() {
        let q = parse_query("deploy");
        assert_eq!(q.text, "deploy");
        assert!(q.filters.is_empty());
    }

    #[test]
    fn pop_last_filter_removes_only_the_last_operator() {
        // last operator dropped; earlier operators and fuzzy text preserved
        assert_eq!(
            pop_last_filter_token("t:ci lang:bash deploy"),
            "t:ci deploy"
        );
        assert_eq!(pop_last_filter_token("t:ci"), "");
        assert_eq!(pop_last_filter_token("deploy t:ci"), "deploy");
        // negation counts as an operator
        assert_eq!(pop_last_filter_token("deploy -t:wip"), "deploy");
        // no operator -> unchanged
        assert_eq!(pop_last_filter_token("deploy prod"), "deploy prod");
        assert_eq!(pop_last_filter_token(""), "");
        // the popped token need not be the textually-last token
        assert_eq!(pop_last_filter_token("t:ci deploy prod"), "deploy prod");
    }

    #[test]
    fn multiple_plain_terms_are_preserved_in_order() {
        let q = parse_query("deploy prod app");
        assert_eq!(q.text, "deploy prod app");
        assert!(q.filters.is_empty());
    }

    #[test]
    fn empty_and_whitespace_are_empty_query() {
        assert_eq!(parse_query(""), Query::default());
        assert_eq!(parse_query("    "), Query::default());
    }

    #[test]
    fn tag_operator_short_and_long() {
        let q = parse_query("t:ci");
        assert_eq!(q.filters, vec![Filter::Tag("ci".into())]);
        assert_eq!(q.text, "");

        let q = parse_query("tag:ci");
        assert_eq!(q.filters, vec![Filter::Tag("ci".into())]);
    }

    #[test]
    fn category_operator_short_and_long() {
        assert_eq!(
            parse_query("c:ops").filters,
            vec![Filter::Category("ops".into())]
        );
        assert_eq!(
            parse_query("category:ops").filters,
            vec![Filter::Category("ops".into())]
        );
    }

    #[test]
    fn lang_operator_parses_known_language() {
        let q = parse_query("lang:bash");
        assert_eq!(q.filters, vec![Filter::Lang(Language::Bash)]);
        assert_eq!(q.text, "");
    }

    #[test]
    fn unknown_lang_value_falls_back_to_plain_text() {
        // Forgiving: an unrecognized language is NOT an error; the whole token
        // is kept as fuzzy text so the user still gets a search.
        let q = parse_query("lang:cobol deploy");
        assert!(q.filters.is_empty());
        assert_eq!(q.text, "lang:cobol deploy");
    }

    #[test]
    fn fav_operator_sets_favorite_filter() {
        let q = parse_query("fav:");
        assert_eq!(q.filters, vec![Filter::Favorite]);
        assert_eq!(q.text, "");
    }

    #[test]
    fn risk_operator_parses_level() {
        assert_eq!(
            parse_query("risk:high").filters,
            vec![Filter::Risk(RiskLevel::High)]
        );
    }

    #[test]
    fn negated_tag_is_not_tag_filter() {
        let q = parse_query("-t:wip");
        assert_eq!(q.filters, vec![Filter::NotTag("wip".into())]);
        assert_eq!(q.text, "");
    }

    #[test]
    fn combined_operators_and_text() {
        let q = parse_query("t:ci c:ops deploy prod");
        assert!(q.filters.contains(&Filter::Tag("ci".into())));
        assert!(q.filters.contains(&Filter::Category("ops".into())));
        assert_eq!(q.filters.len(), 2);
        assert_eq!(q.text, "deploy prod");
    }

    #[test]
    fn operators_are_case_insensitive_keys_but_preserve_value_case_lowered() {
        // The operator KEY is case-insensitive (T: works); tag values are
        // lowercased for case-insensitive matching against normalized tags.
        let q = parse_query("T:CI");
        assert_eq!(q.filters, vec![Filter::Tag("ci".into())]);
    }

    #[test]
    fn unknown_operator_is_plain_text() {
        // `foo:bar` is not a known operator -> kept as fuzzy text, not dropped.
        let q = parse_query("foo:bar deploy");
        assert!(q.filters.is_empty());
        assert_eq!(q.text, "foo:bar deploy");
    }

    #[test]
    fn operator_with_empty_value_except_fav_is_plain_text() {
        // `t:` with no value is meaningless as a filter -> treat as plain text
        // (so a half-typed operator doesn't silently filter to nothing). `fav:`
        // is the one valueless operator and is handled separately.
        let q = parse_query("t: deploy");
        assert!(q.filters.is_empty());
        assert_eq!(q.text, "t: deploy");
    }

    #[test]
    fn duplicate_filters_are_deduplicated() {
        // Typing the same filter twice shouldn't produce two identical filters.
        let q = parse_query("t:ci t:ci deploy");
        assert_eq!(q.filters, vec![Filter::Tag("ci".into())]);
        assert_eq!(q.text, "deploy");
    }

    #[test]
    fn negated_unknown_operator_is_plain_text() {
        // Only `-t:`/`-tag:` negation is defined; `-c:` etc. fall back to text.
        let q = parse_query("-c:ops deploy");
        assert!(q.filters.is_empty());
        assert_eq!(q.text, "-c:ops deploy");
    }
}
