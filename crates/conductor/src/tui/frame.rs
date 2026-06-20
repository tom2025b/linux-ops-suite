//! Pure frame renderers for the interactive TUI: model in, String out, no I/O.
//! Everything the event loop paints is built here, so every screen is
//! snapshot-testable without a PTY. Layout mirrors CONDUCTOR_DESIGN.md ("The
//! plan" and "All clear"). The current step is `▸`, pending `○`, done `✓`,
//! skipped `·`; the right-edge tag is the ring word; the command is shown
//! verbatim under each step. Color (via `Style`) is always optional — the words
//! and glyphs carry every distinction.

use crate::plan::{Plan, Step, StepStatus};
use crate::tui::style::Style;

/// The one-line key-hint strip shown at the foot of the plan screen.
pub const HINT: &str =
    "enter  run step    s  skip    a  advance    r  rexops    ?  help    q  quit";

/// The glyph for a step in the interactive view. The current step overrides this
/// with `▸` regardless of status (it is by definition Pending when focused).
fn glyph(status: StepStatus, is_current: bool) -> char {
    if is_current {
        return '▸';
    }
    match status {
        StepStatus::Pending => '○',
        StepStatus::Done => '✓',
        StepStatus::Skipped => '·',
        StepStatus::Failed => '✗',
    }
}

/// Render one step block: the marker line (glyph, number, title, optional ring
/// tag, optional correlation annotation) then the dim command line.
fn render_step(out: &mut String, n: usize, step: &Step, is_current: bool, style: &Style) {
    let g = glyph(step.status, is_current);
    let marker_color = if is_current {
        style.current_marker()
    } else {
        ""
    };
    let marker_rst = if is_current { style.rst } else { "" };
    out.push_str(&format!(
        "  {mc}{g}{mr} {n}  {title}",
        mc = marker_color,
        g = g,
        mr = marker_rst,
        n = n,
        title = step.title,
    ));
    if let Some(note) = &step.annotation {
        out.push_str(&format!("  {}← {}{}", style.cyn, note, style.rst));
    }
    // The ring tag rides at the end of the title line (right-edge in the design;
    // kept inline here so it never clips at narrow widths).
    out.push_str(&format!(
        "  {rc}{tag}{rst}",
        rc = style.ring_color(step.ring),
        tag = step.ring.tag(),
        rst = style.rst,
    ));
    out.push('\n');
    if let Some(cmd) = &step.command {
        out.push_str(&format!(
            "       {dim}{cmd}{rst}\n",
            dim = style.dim,
            cmd = cmd,
            rst = style.rst
        ));
    }
}

/// The healthy "nothing to conduct" screen — Conductor's voice for an empty plan.
pub fn healthy_screen(style: &Style) -> String {
    format!(
        "\n\n\n                             {g}nothing to conduct{r}\n\n                          the suite is healthy and\n                           every feed is current\n",
        g = style.grn,
        r = style.rst,
    )
}

/// The full plan screen: situation, the ordered steps with the current one
/// marked, an optional transient notice line, and the key-hint strip.
pub fn plan_screen(plan: &Plan, cursor: usize, notice: Option<&str>, style: &Style) -> String {
    if plan.is_empty() {
        return healthy_screen(style);
    }
    let mut out = String::new();
    out.push_str(&format!(
        " {b}conductor{r}\n\n",
        b = style.bold,
        r = style.rst
    ));
    if !plan.situation.is_empty() {
        out.push_str(&format!(
            "   {b}the situation{r}\n",
            b = style.bold,
            r = style.rst
        ));
        for line in &plan.situation {
            out.push_str(&format!("   {line}\n"));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "   {b}the plan{r}   {n} steps\n",
        b = style.bold,
        r = style.rst,
        n = plan.steps.len()
    ));
    for (i, step) in plan.steps.iter().enumerate() {
        render_step(&mut out, i + 1, step, i == cursor, style);
    }
    out.push('\n');
    if let Some(msg) = notice {
        out.push_str(&format!(
            " {dim}{msg}{rst}\n",
            dim = style.dim,
            msg = msg,
            rst = style.rst
        ));
    }
    out.push_str(&format!(
        " {dim}{HINT}{rst}\n",
        dim = style.dim,
        HINT = HINT,
        rst = style.rst
    ));
    out
}

/// The compact (<80×24) fallback: a plain unpadded list that cannot clip — title
/// line then command line per step, with a one-letter current marker.
pub fn compact_plan(plan: &Plan, cursor: usize, style: &Style) -> String {
    if plan.is_empty() {
        return format!("{g}nothing to conduct{r}\n", g = style.grn, r = style.rst);
    }
    let mut out = String::new();
    for (i, step) in plan.steps.iter().enumerate() {
        let g = glyph(step.status, i == cursor);
        out.push_str(&format!(
            "{g} {n} {t} [{tag}]\n",
            g = g,
            n = i + 1,
            t = step.title,
            tag = step.ring.tag()
        ));
        if let Some(cmd) = &step.command {
            out.push_str(&format!("    {cmd}\n", cmd = cmd));
        }
    }
    out.push_str("enter run  s skip  a next  q quit\n");
    out
}

/// The help screen: every key with a one-line description.
pub fn help_screen(style: &Style) -> String {
    format!(
        " {b}keys{r}\n   enter  run the current step (read-only runs; changes-state needs Phase 3)\n   s      skip the current step\n   a      advance focus without running\n   r      hand off to the rexops cockpit\n   ?      toggle this help\n   q      quit\n",
        b = style.bold,
        r = style.rst,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use crate::state::{FeedStatus, Finding, Freshness, Severity, SuiteState};

    fn plain() -> Style {
        Style::resolve(true)
    }

    /// A non-trivial plan: a stale-feed refresh (Ring 2), a safety capture
    /// (Ring 2), and an investigate step (Ring 1) — exercises every ring + the
    /// situation block.
    fn sample_plan() -> Plan {
        let mut s = SuiteState::empty();
        s.feeds.push(FeedStatus {
            name: "tools",
            freshness: Freshness::Stale,
        });
        s.findings.push(Finding {
            what: "deploy-prod.sh".into(),
            why: "AWS key".into(),
            source: "bulwark".into(),
            severity: Severity::Critical,
        });
        plan::build(&s)
    }

    fn longest_line(frame: &str) -> usize {
        frame.lines().map(|l| l.chars().count()).max().unwrap_or(0)
    }

    #[test]
    fn plan_screen_shows_steps_commands_and_ring_tags() {
        let p = sample_plan();
        let out = plan_screen(&p, 0, None, &plain());
        assert!(out.contains("the plan"));
        assert!(out.contains("workstate snapshot"));
        assert!(out.contains("changes state"));
        assert!(out.contains("bulwark show deploy-prod.sh"));
        assert!(out.contains("read-only"));
        // current marker on step 1
        assert!(out.contains("▸ 1"));
        // pending marker on a later step
        assert!(out.contains("○ "));
        assert!(out.contains(HINT));
    }

    #[test]
    fn plan_screen_renders_notice_line_when_present() {
        let p = sample_plan();
        let out = plan_screen(&p, 0, Some("needs Phase 3 — not run"), &plain());
        assert!(out.contains("needs Phase 3 — not run"));
    }

    #[test]
    fn healthy_screen_speaks_conductors_voice() {
        let out = healthy_screen(&plain());
        assert!(out.contains("nothing to conduct"));
        assert!(out.contains("the suite is healthy"));
        assert!(!out.contains("the plan"));
    }

    #[test]
    fn no_color_frames_have_no_escapes() {
        let p = sample_plan();
        assert!(!plan_screen(&p, 0, Some("x"), &plain()).contains('\u{1b}'));
        assert!(!healthy_screen(&plain()).contains('\u{1b}'));
        assert!(!compact_plan(&p, 0, &plain()).contains('\u{1b}'));
        assert!(!help_screen(&plain()).contains('\u{1b}'));
    }

    #[test]
    fn frames_fit_80_columns_with_color_off() {
        let p = sample_plan();
        assert!(
            longest_line(&plan_screen(
                &p,
                0,
                Some("needs Phase 3 — not run"),
                &plain()
            )) <= 80
        );
        assert!(longest_line(&healthy_screen(&plain())) <= 80);
        assert!(longest_line(&help_screen(&plain())) <= 80);
    }

    #[test]
    fn compact_plan_is_narrow_and_lists_every_step() {
        let p = sample_plan();
        let out = compact_plan(&p, 1, &plain());
        // current marker now on step 2
        assert!(out.contains("▸ 2"));
        assert!(out.contains("workstate snapshot"));
        assert!(longest_line(&out) <= 60, "compact must stay narrow: {out}");
    }
}
