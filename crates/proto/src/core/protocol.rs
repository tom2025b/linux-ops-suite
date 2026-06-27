use serde::{Deserialize, Serialize}; // derive (de)serialization for YAML/JSON

// -----------------------------------------------------------------------------
// Protocol — the whole checklist.
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Protocol {
    // Stable machine identifier, e.g. "rust-repo-review". This is what the user
    // types in `proto run rust-repo-review`, and what a session references. It
    // should match the filename stem by convention (the loader checks this).
    pub id: String,

    // Human-facing one-liner shown as the heading when the run starts.
    pub title: String,

    // Longer prose describing WHEN and WHY to use this protocol. Optional so a
    // quick checklist isn't forced to write a paragraph. `default` makes a
    // missing YAML key deserialize to an empty String instead of erroring.
    #[serde(default)]
    pub description: String,

    // Optional version of the protocol CONTENT itself (not Proto's version), so
    // an operator can tell "this is the v2 review checklist". Plain string so it
    // can be "1", "1.2", "2024-06", whatever the author prefers.
    #[serde(default)]
    pub version: String,

    // The ordered steps. Order in the YAML list IS the order of execution —
    // there is no separate sort key, which keeps authoring dead simple. The
    // validator guarantees this is non-empty and that step ids are unique.
    pub steps: Vec<Step>,
}

impl Protocol {
    // Number of steps — a tiny convenience so callers (CLI summaries, the run
    // engine's "Step 3 of 7") don't reach into `.steps.len()` everywhere.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

// -----------------------------------------------------------------------------
// Step — one item in the checklist.
// -----------------------------------------------------------------------------
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    // Stable per-step id, unique WITHIN a protocol (e.g. "check-ci"). A session
    // records outcomes keyed by this id, so renaming it breaks old sessions —
    // hence "stable". The validator enforces uniqueness.
    pub id: String,

    // What to show the operator: the actual instruction/question for this step.
    pub title: String,

    // Optional extra guidance — the "how" or "why" behind the step, shown under
    // the title. Keeps the title short while still teaching.
    #[serde(default)]
    pub detail: String,

    // What KIND of step this is, which decides how the run engine prompts and
    // what answers are valid. Defaults to a yes/no confirmation — the most
    // common checklist shape — so most steps need only an id + title.
    #[serde(default)]
    pub kind: StepKind,

    // For a `command` step: the EXACT shell command Proto offers to run on the
    // operator's behalf. Kept separate from `detail`: `detail` is
    // human prose (often with authored parameters like <file>), whereas this is a
    // literal, runnable string. `Option` because it's opt-in: a command step
    // WITHOUT this field stays display-only (the operator runs it themselves),
    // which preserves every existing protocol's behaviour unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

// -----------------------------------------------------------------------------
// StepKind — the behaviour of a step.
// -----------------------------------------------------------------------------
// `#[serde(rename_all = "snake_case")]` makes the YAML write `kind: manual_check`
// (idiomatic YAML) while the Rust variant stays `ManualCheck` (idiomatic Rust).
// `#[serde(tag = "type")]`-style enums are overkill here; these are simple unit
// variants, so a plain string in YAML is the cleanest representation.
// `#[derive(Default)]` on an enum picks the variant tagged `#[default]` as the
// value `Default::default()` returns. That's what `#[serde(default)]` on
// Step::kind uses when the YAML omits `kind:` — so a bare step becomes a
// ManualCheck. (Deriving it is the idiomatic modern form; a hand-written
// `impl Default` would do the same thing with more code.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StepKind {
    // A yes/no confirmation: "Did you do X?" The operator answers and Proto
    // records pass/fail. This is the default and the backbone of a checklist.
    #[default]
    ManualCheck,

    // Pure information — there is nothing to confirm, just something to READ
    // before continuing (e.g. "Note: this protocol assumes a clean git tree").
    // The run engine shows it and waits for acknowledgement, recording no
    // pass/fail. Useful for context and section headers.
    Info,

    // A step tied to a shell command. If the step carries a `command:` field,
    // Proto OFFERS to run it: it prints the exact command and only executes it
    // (through the shared executor — timeout + process-group kill, output
    // streamed live) after a per-command `y`, then derives pass/fail from the
    // exit code. Declining, or a `command` step WITHOUT a `command:` field,
    // falls back to the manual y/n/s self-report. Execution is always opt-in and
    // per-command — Proto never runs anything without that explicit confirmation.
    Command,
}
