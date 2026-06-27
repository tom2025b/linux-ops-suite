use proto::core::render;
use proto::core::session::{Session, StepStatus};

// Build a small two-step session: one passed (with a note containing a pipe, to
// exercise escaping), one acknowledged. Constructed through the public model the
// same way `run` does.
fn sample() -> Session {
    let protocol = proto::Protocol {
        id: "demo".to_string(),
        title: "Demo Protocol".to_string(),
        description: String::new(),
        version: String::new(),
        steps: vec![
            proto::Step {
                id: "check-one".to_string(),
                title: "first".to_string(),
                detail: String::new(),
                kind: proto::StepKind::ManualCheck,
                command: None,
            },
            proto::Step {
                id: "read-this".to_string(),
                title: "second".to_string(),
                detail: String::new(),
                kind: proto::StepKind::Info,
                command: None,
            },
        ],
    };
    let mut s = Session::new(&protocol);
    s.steps[0].status = StepStatus::Passed;
    s.steps[0].note = "saw a | pipe".to_string(); // pipe must be escaped in the table
    s.steps[1].status = StepStatus::Acknowledged;
    s.finished_at = Some(s.started_at);
    s
}

#[test]
fn markdown_has_heading_metadata_and_steps() {
    let md = render::session_markdown("demo-20260606T000000Z", &sample());

    // Heading is the protocol title.
    assert!(md.contains("# Demo Protocol"), "missing heading:\n{md}");
    // Metadata carries the session id and protocol id.
    assert!(
        md.contains("demo-20260606T000000Z"),
        "missing session id:\n{md}"
    );
    assert!(md.contains("`demo`"), "missing protocol id:\n{md}");
    // Outcome line uses the shared tally wording.
    assert!(md.contains("1 passed"), "missing tally:\n{md}");
    // Each step id appears in the table.
    assert!(md.contains("check-one"), "missing step id:\n{md}");
    assert!(md.contains("read-this"), "missing step id:\n{md}");
    // Statuses render as words.
    assert!(md.contains("passed"), "missing passed status:\n{md}");
    assert!(
        md.contains("acknowledged"),
        "missing acknowledged status:\n{md}"
    );
}

#[test]
fn markdown_escapes_pipes_in_notes() {
    let md = render::session_markdown("demo-1", &sample());
    // The raw note "saw a | pipe" must appear with the pipe escaped, so the table
    // layout isn't broken by user-entered text.
    assert!(md.contains("saw a \\| pipe"), "pipe not escaped:\n{md}");
}

#[test]
fn markdown_marks_incomplete_runs() {
    let mut s = sample();
    s.finished_at = None; // an in-progress run
    let md = render::session_markdown("demo-1", &s);
    assert!(md.contains("incomplete"), "should label incomplete:\n{md}");
}
