//! The Plan and its parts. A `Plan` is conductor's whole output: a short
//! `situation` (why there's a plan) and an ordered list of `Step`s. Each step
//! carries a stable `id` (a kebab slug, so Phase 3 / JSON consumers can address a
//! step across runs), its literal `command`, its `ring` (what running it would
//! do), an optional correlation `annotation`, and a `status`. These are *types
//! only*; the rules that fill them live in [`rules`], and rendering lives in
//! `report`.

pub mod rules;

/// What running a step would do — the safety classification from the design.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ring {
    /// Spawns a sibling that only reads (Ring 1).
    ReadOnly,
    /// Spawns a sibling that writes (Ring 2) — requires a confirm in Phase 3.
    ChangesState,
    /// Conductor's own informational step (Ring 0): shows a fix/next command but
    /// runs nothing itself.
    Info,
}

impl Ring {
    /// The short tag rendered at the step's right edge.
    pub fn tag(self) -> &'static str {
        match self {
            Ring::ReadOnly => "read-only",
            Ring::ChangesState => "changes state",
            Ring::Info => "info",
        }
    }
}

/// A step's lifecycle. Phase 1 only ever produces `Pending`; `Done`/`Skipped`
/// are driven by the Phase 2/3 TUI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepStatus {
    Pending,
    Done,
    Skipped,
}

/// One ordered action in the plan.
#[derive(Clone, Debug)]
pub struct Step {
    /// Stable kebab id (e.g. `refresh-stale-data`, `investigate-deploy-prod-sh`).
    /// Deterministic for a given state, so a JSON consumer or the Phase 3 driver
    /// can refer to a step across runs without relying on its position.
    pub id: String,
    pub title: String,
    /// The literal command conductor would spawn (shown verbatim), or `None` for
    /// a pure-prose step.
    pub command: Option<String>,
    pub ring: Ring,
    /// A correlation note (rule 5), rendered inline (e.g. "same file as drift").
    pub annotation: Option<String>,
    pub status: StepStatus,
}

impl Step {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        command: Option<String>,
        ring: Ring,
    ) -> Self {
        Step {
            id: id.into(),
            title: title.into(),
            command,
            ring,
            annotation: None,
            status: StepStatus::Pending,
        }
    }

    /// Attach a correlation annotation, builder-style.
    pub fn annotated(mut self, note: impl Into<String>) -> Self {
        self.annotation = Some(note.into());
        self
    }
}

/// Conductor's complete output for a run.
#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub situation: Vec<String>,
    pub steps: Vec<Step>,
}

impl Plan {
    /// True when there is nothing to conduct.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// A stable id for the whole plan: a short hex digest over the ordered step
    /// ids. Deterministic — the same plan shape always yields the same `plan_id`,
    /// and it changes when the steps or their order change. Empty plan ⇒
    /// `"nothing-to-conduct"`. Used by the JSON envelope so Phase 3 / external
    /// consumers can key off a run's plan identity.
    pub fn plan_id(&self) -> String {
        if self.steps.is_empty() {
            return "nothing-to-conduct".to_string();
        }
        let joined = self
            .steps
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        format!("plan-{:016x}", fnv1a64(joined.as_bytes()))
    }
}

/// Turn an arbitrary label into a stable kebab slug: lowercase, runs of
/// non-alphanumeric collapse to a single `-`, trimmed. Dependency-free. Used to
/// derive a step `id` from a finding name or a fixed role string.
pub fn slug(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut prev_dash = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// FNV-1a 64-bit hash — tiny, dependency-free, and stable across platforms and
/// runs (unlike `DefaultHasher`, whose output isn't guaranteed stable). Good
/// enough to give a plan a deterministic identity; not a cryptographic hash.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Build the runbook from suite state by running the v1 rules in priority order
/// (CONDUCTOR_DESIGN.md). Deterministic: the same state always yields the same
/// plan. Order: refresh → wiring fixes → safety-capture (only if real work
/// follows) → investigate findings (drift-correlated first) → review failed jobs.
pub fn build(state: &crate::state::SuiteState) -> Plan {
    let mut steps: Vec<Step> = Vec::new();

    // 1. trust the data first
    if let Some(refresh) = rules::refresh_stale_feeds(state) {
        steps.push(refresh);
    }
    // 2. wiring gaps
    steps.extend(rules::wiring_gaps(state));

    // 4/5 + 6 computed up front so rule 3 knows whether real work follows
    let findings = rules::investigate_findings(state);
    let jobs = rules::review_failed_jobs(state);

    // 3. capture before you change — only when the plan guides real work
    if !findings.is_empty() || !jobs.is_empty() {
        steps.push(rules::safety_capture());
    }
    // 4/5
    steps.extend(findings);
    // 6
    steps.extend(jobs);

    Plan {
        situation: rules::situation(state),
        steps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_tags_are_words_so_color_is_never_load_bearing() {
        assert_eq!(Ring::ReadOnly.tag(), "read-only");
        assert_eq!(Ring::ChangesState.tag(), "changes state");
        assert_eq!(Ring::Info.tag(), "info");
    }

    #[test]
    fn new_step_defaults_to_pending_and_unannotated() {
        let s = Step::new(
            "do-thing",
            "do a thing",
            Some("pulse".to_string()),
            Ring::ReadOnly,
        );
        assert_eq!(s.id, "do-thing");
        assert_eq!(s.status, StepStatus::Pending);
        assert!(s.annotation.is_none());
        assert_eq!(s.command.as_deref(), Some("pulse"));
    }

    #[test]
    fn annotated_attaches_a_note() {
        let s = Step::new("inv-x", "investigate x", None, Ring::ReadOnly)
            .annotated("same file as drift");
        assert_eq!(s.annotation.as_deref(), Some("same file as drift"));
    }

    #[test]
    fn empty_plan_reports_empty_and_has_a_fixed_id() {
        let p = Plan::default();
        assert!(p.is_empty());
        assert_eq!(p.plan_id(), "nothing-to-conduct");
    }

    #[test]
    fn slug_makes_stable_kebab_ids() {
        assert_eq!(slug("deploy-prod.sh"), "deploy-prod-sh");
        assert_eq!(slug("Refresh Stale Data"), "refresh-stale-data");
        assert_eq!(slug("  weird __ name!! "), "weird-name");
        assert_eq!(slug("already-kebab"), "already-kebab");
    }

    #[test]
    fn plan_id_is_deterministic_and_order_sensitive() {
        let mk = |ids: &[&str]| Plan {
            situation: vec![],
            steps: ids
                .iter()
                .map(|i| Step::new(*i, "t", None, Ring::Info))
                .collect(),
        };
        let a = mk(&["one", "two"]);
        let b = mk(&["one", "two"]);
        let c = mk(&["two", "one"]);
        assert_eq!(a.plan_id(), b.plan_id(), "same steps ⇒ same id");
        assert_ne!(a.plan_id(), c.plan_id(), "different order ⇒ different id");
        assert!(a.plan_id().starts_with("plan-"));
    }
}
