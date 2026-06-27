// highlight.rs — syntax-highlight preview lines via syntect, styled through Theme.
// -----------------------------------------------------------------------------
// Binary-only. Lexes to SyntaxKind then asks Theme for Style (guarantees zero
// fg under NO_COLOR). Unknown lang or error -> plain text. Core never sees it.

use std::sync::OnceLock;

use ratatui::text::{Line, Span};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

use scriptvault_core::Language;

use super::theme::{SyntaxKind, SyntaxStyle, Theme};

/// Load syntect's bundled syntaxes once (parsing them is not free).
fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Pick a syntect syntax for one of our languages, by its syntax NAME. A miss
/// (or `Unknown`) returns `None` → the caller renders plain text. (Names verified
/// to resolve against the bundled default syntaxes for all seven languages.)
fn syntax_for(lang: Language) -> Option<&'static SyntaxReference> {
    let set = syntaxes();
    let name = match lang {
        Language::Bash => "Bourne Again Shell (bash)",
        Language::Python => "Python",
        Language::Rust => "Rust",
        Language::Node => "JavaScript",
        Language::Ruby => "Ruby",
        Language::Sql => "SQL",
        Language::Lua => "Lua",
        Language::Unknown => return None,
    };
    set.find_syntax_by_name(name)
}

/// Classify a scope's textual name (e.g. "comment.line.number-sign",
/// "string.quoted.double", "keyword.control") into our coarse `SyntaxKind`.
fn classify(scope_name: &str) -> SyntaxKind {
    if scope_name.starts_with("comment") {
        SyntaxKind::Comment
    } else if scope_name.starts_with("string") {
        SyntaxKind::StringLit
    } else if scope_name.starts_with("constant.numeric") {
        SyntaxKind::Number
    } else if scope_name.starts_with("keyword") || scope_name.starts_with("storage") {
        SyntaxKind::Keyword
    } else {
        SyntaxKind::Normal
    }
}

/// Highlight `lines` of source in `lang`, returning ratatui `Line`s whose spans
/// are styled THROUGH `theme`. Unknown language or any lexing hiccup → plain,
/// unstyled lines. Never errors, never panics.
pub fn highlight_lines(lines: &[String], lang: Language, theme: Theme) -> Vec<Line<'static>> {
    let Some(syntax) = syntax_for(lang) else {
        return plain(lines);
    };
    let set = syntaxes();
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();

    let mut out: Vec<Line<'static>> = Vec::with_capacity(lines.len());
    for raw in lines {
        // syntect wants a trailing newline to lex a line correctly.
        let line_nl = format!("{raw}\n");
        // `parse_line` yields byte offsets paired with scope-stack operations.
        let ops = match state.parse_line(&line_nl, set) {
            Ok(ops) => ops,
            Err(_) => {
                out.push(Line::from(Span::raw(raw.clone())));
                continue;
            }
        };
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut last = 0usize;
        for (offset, op) in ops {
            if offset > last {
                push_span(
                    &mut spans,
                    &line_nl[last..offset],
                    current_kind(&stack),
                    theme,
                );
            }
            // Applying the op can only fail on a malformed stack; ignore and
            // keep going (worst case a token is classified as Normal).
            stack.apply(&op).ok();
            last = offset;
        }
        if last < line_nl.len() {
            // Trim the synthetic '\n' off the trailing run.
            push_span(
                &mut spans,
                line_nl[last..].trim_end_matches('\n'),
                current_kind(&stack),
                theme,
            );
        }
        out.push(Line::from(spans));
    }
    out
}

/// The current `SyntaxKind` from the top (most-specific) scope on the stack.
/// `as_slice()` is used because recent syntect keeps the `scopes` field crate-
/// internal in spirit; `Scope` is `Copy`, so `build_string()` (which takes
/// `self`) works on the dereferenced reference.
fn current_kind(stack: &ScopeStack) -> SyntaxKind {
    match stack.as_slice().last() {
        Some(scope) => classify(&scope.build_string()),
        None => SyntaxKind::Normal,
    }
}

/// Push a themed span, skipping empties.
fn push_span(spans: &mut Vec<Span<'static>>, text: &str, kind: SyntaxKind, theme: Theme) {
    if text.is_empty() {
        return;
    }
    spans.push(Span::styled(text.to_string(), theme.syntax(kind)));
}

/// Render lines as plain, unstyled text (the fallback + NO_COLOR-safe baseline).
fn plain(lines: &[String]) -> Vec<Line<'static>> {
    lines
        .iter()
        .map(|l| Line::from(Span::raw(l.clone())))
        .collect()
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn unknown_language_is_plain() {
        let lines = vec!["anything".to_string()];
        let out = highlight_lines(&lines, Language::Unknown, Theme::with_color(true));
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn bash_highlight_has_no_fg_color_under_no_color() {
        let lines = vec!["# a comment".to_string(), "echo \"hi\"".to_string()];
        let out = highlight_lines(&lines, Language::Bash, Theme::with_color(false));
        // No span in any line may carry a foreground colour.
        for line in &out {
            for span in &line.spans {
                assert!(
                    matches!(span.style.fg, None | Some(Color::Reset)),
                    "NO_COLOR highlight produced a foreground colour: {:?}",
                    span.style.fg
                );
            }
        }
    }

    #[test]
    fn bash_highlight_has_some_color_when_enabled() {
        let lines = vec!["# a comment".to_string()];
        let out = highlight_lines(&lines, Language::Bash, Theme::with_color(true));
        let any_color = out
            .iter()
            .flat_map(|l| &l.spans)
            .any(|s| !matches!(s.style.fg, None | Some(Color::Reset)));
        assert!(
            any_color,
            "expected at least one coloured span for a bash comment"
        );
    }
}

// Syntect lexing + Theme routing = NO_COLOR guarantee. See ARCHITECTURE.md.
