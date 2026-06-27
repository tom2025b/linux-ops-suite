//! Interactive auto-check flow for bare `proto` in a detected project.

use std::io::{self, Write};
use std::path::Path;

use anyhow::Context;
use chrono::{DateTime, Utc};

use crate::core::checks::{self, CheckProfile};
use crate::core::detector::{ProjectType, detect_project_type};
use crate::core::executor::{self, CheckOutcome, CheckStatus, ExecutionOptions};
use crate::core::protocol::{Protocol, Step, StepKind};
use crate::core::session::{Session, StepStatus, now_secs};
use crate::core::store;
use crate::core::summary::{self, CheckRunSummary};

use super::picker;

pub fn handle_or_picker(
    protocols_dir: &Path,
    sessions_dir: &Path,
    feed_dir: &Path,
    write_feed: bool,
) -> anyhow::Result<()> {
    let project_dir = std::env::current_dir().context("could not read current directory")?;

    let Some(project_type) = detect_project_type(&project_dir) else {
        return picker::handle(protocols_dir, sessions_dir, feed_dir, write_feed);
    };

    let profiles = checks::profiles_for_language(project_type);
    if profiles.is_empty() {
        println!(
            "Detected a {project_type} project, but Proto has no built-in check profiles for it yet.\n"
        );
        return picker::handle(protocols_dir, sessions_dir, feed_dir, write_feed);
    }

    handle_detected_project(
        &project_dir,
        project_type,
        &profiles,
        sessions_dir,
        feed_dir,
        write_feed,
    )
}

fn handle_detected_project(
    project_dir: &Path,
    project_type: ProjectType,
    profiles: &[CheckProfile],
    sessions_dir: &Path,
    feed_dir: &Path,
    write_feed: bool,
) -> anyhow::Result<()> {
    print_profile_picker(project_dir, project_type, profiles);

    let Some(profile) = prompt_profile_choice(profiles)? else {
        println!("No check profile selected.");
        return Ok(());
    };

    let started_at = now_secs();
    let options = ExecutionOptions::default().with_working_dir(project_dir);
    let outcomes = run_profile_with_progress(profile, &options);
    let finished_at = now_secs();
    let check_summary = CheckRunSummary::from_outcomes(&outcomes);

    println!();
    print!("{}", check_summary.render_human());

    let session = session_from_check_run(
        project_type,
        profile,
        project_dir,
        &outcomes,
        started_at,
        finished_at,
    );
    let path = store::save(sessions_dir, &session)?;
    let id = store::session_id(&session);

    println!("\nSaved session '{id}'");
    println!("  file: {}", path.display());
    println!("  open: proto show {id}");

    if write_feed {
        update_feed(sessions_dir, feed_dir);
    }

    Ok(())
}

fn print_profile_picker(project_dir: &Path, project_type: ProjectType, profiles: &[CheckProfile]) {
    println!("Proto - {project_type} check profiles");
    println!("project: {}\n", project_dir.display());
    println!("Profiles ({}):\n", profiles.len());

    let name_width = profiles
        .iter()
        .map(|profile| profile.name.len())
        .max()
        .unwrap_or(0);

    for (index, profile) in profiles.iter().enumerate() {
        println!(
            "  {:>2}.  {:<width$}  -  {}",
            index + 1,
            profile.name,
            profile.description,
            width = name_width
        );
    }
}

fn prompt_profile_choice(profiles: &[CheckProfile]) -> anyhow::Result<Option<&CheckProfile>> {
    loop {
        print!(
            "\nPick a profile [1-{}] or name (Enter/q to cancel): ",
            profiles.len()
        );
        io::stdout().flush()?;

        let mut input = String::new();
        let read = io::stdin().read_line(&mut input)?;
        if read == 0 {
            return Ok(None);
        }

        let trimmed = input.trim();
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("q")
            || trimmed.eq_ignore_ascii_case("quit")
        {
            return Ok(None);
        }

        if let Ok(number) = trimmed.parse::<usize>() {
            if (1..=profiles.len()).contains(&number) {
                return Ok(Some(&profiles[number - 1]));
            }
            println!("  No profile number {number}. Enter 1-{}.", profiles.len());
            continue;
        }

        if let Some(profile) = profiles.iter().find(|profile| {
            profile.name.eq_ignore_ascii_case(trimmed) || slugify(&profile.name) == slugify(trimmed)
        }) {
            return Ok(Some(profile));
        }

        println!("  No profile named '{trimmed}'. Try a number or profile name.");
    }
}

fn run_profile_with_progress(
    profile: &CheckProfile,
    options: &ExecutionOptions,
) -> Vec<CheckOutcome> {
    println!("\nRunning {} checks...\n", profile.checks.len());

    let mut outcomes = Vec::with_capacity(profile.checks.len());
    for (index, check) in profile.checks.iter().enumerate() {
        println!("[{}/{}] {}", index + 1, profile.checks.len(), check.name);
        println!("    $ {}", check.command);

        let outcome = executor::execute_check(check, options);
        println!(
            "    {} {} ({})\n",
            status_marker(outcome.status),
            outcome.status,
            summary::format_duration(outcome.duration)
        );

        outcomes.push(outcome);
    }

    outcomes
}

fn session_from_check_run(
    project_type: ProjectType,
    profile: &CheckProfile,
    project_dir: &Path,
    outcomes: &[CheckOutcome],
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
) -> Session {
    let protocol = protocol_from_profile(project_type, profile, project_dir);
    let mut session = Session::new(&protocol);

    session.started_at = started_at;
    session.finished_at = Some(finished_at);
    session.generated_at = finished_at;

    for (result, outcome) in session.steps.iter_mut().zip(outcomes) {
        result.status = step_status_for_outcome(outcome.status);
        result.answered_at = Some(finished_at);
        result.note = session_note_for_outcome(outcome);
    }

    session
}

fn protocol_from_profile(
    project_type: ProjectType,
    profile: &CheckProfile,
    project_dir: &Path,
) -> Protocol {
    Protocol {
        id: check_run_protocol_id(project_type, profile),
        title: format!("{project_type} checks: {}", profile.name),
        description: format!(
            "Auto-detected {project_type} check profile run in {}",
            project_dir.display()
        ),
        version: String::new(),
        steps: profile
            .checks
            .iter()
            .map(|check| Step {
                id: check.id.clone(),
                title: check.name.clone(),
                detail: check.command.clone(),
                kind: StepKind::Command,
                command: Some(check.command.clone()),
            })
            .collect(),
    }
}

fn check_run_protocol_id(project_type: ProjectType, profile: &CheckProfile) -> String {
    format!(
        "checks-{}-{}",
        slugify(&project_type.to_string()),
        slugify(&profile.name)
    )
}

fn step_status_for_outcome(status: CheckStatus) -> StepStatus {
    match status {
        CheckStatus::Pass => StepStatus::Passed,
        CheckStatus::Fail | CheckStatus::Error => StepStatus::Failed,
    }
}

fn session_note_for_outcome(outcome: &CheckOutcome) -> String {
    let mut parts = vec![
        format!("status: {}", outcome.status),
        format!("command: {}", outcome.full_command),
        format!("duration: {}", summary::format_duration(outcome.duration)),
    ];

    if let Some(exit_code) = outcome.exit_code {
        parts.push(format!("exit_code: {exit_code}"));
    }
    if outcome.timed_out {
        parts.push("timed_out: true".to_string());
    }
    if let Some(message) = &outcome.error_message {
        parts.push(format!("error: {}", single_line(message)));
    }
    if outcome.status != CheckStatus::Pass {
        if let Some(stderr) = first_non_empty_line(&outcome.stderr) {
            parts.push(format!("stderr: {}", single_line(stderr)));
        } else if let Some(stdout) = first_non_empty_line(&outcome.stdout) {
            parts.push(format!("stdout: {}", single_line(stdout)));
        }
    }

    parts.join("; ")
}

fn first_non_empty_line(output: &str) -> Option<&str> {
    output.lines().find(|line| !line.trim().is_empty())
}

fn single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn status_marker(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Pass => "[pass]",
        CheckStatus::Fail => "[FAIL]",
        CheckStatus::Error => "[ERR ]",
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;

    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.is_empty() {
        "unnamed".to_string()
    } else {
        slug
    }
}

fn update_feed(sessions_dir: &Path, feed_dir: &Path) {
    let result = store::build_feed(sessions_dir).and_then(|feed| store::save_feed(feed_dir, &feed));
    match result {
        Ok(feed_path) => println!("  feed: {}", feed_path.display()),
        Err(err) => eprintln!("  warning: could not update Workstate feed: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::checks::{Check, CheckProfile};

    fn profile() -> CheckProfile {
        CheckProfile {
            name: "Quick Check".to_string(),
            description: "Fast checks".to_string(),
            checks: vec![
                Check {
                    id: "build".to_string(),
                    name: "Build".to_string(),
                    command: "cargo build".to_string(),
                },
                Check {
                    id: "test".to_string(),
                    name: "Tests".to_string(),
                    command: "cargo test".to_string(),
                },
            ],
        }
    }

    fn outcome(id: &str, status: CheckStatus) -> CheckOutcome {
        CheckOutcome {
            check_id: id.to_string(),
            name: id.to_string(),
            status,
            exit_code: match status {
                CheckStatus::Pass => Some(0),
                CheckStatus::Fail => Some(101),
                CheckStatus::Error => None,
            },
            stdout: String::new(),
            stderr: "compiler error".to_string(),
            duration: std::time::Duration::from_secs(1),
            full_command: format!("run {id}"),
            program: "run".to_string(),
            args: vec![id.to_string()],
            working_dir: None,
            timed_out: false,
            error_message: (status == CheckStatus::Error).then(|| "could not spawn".to_string()),
        }
    }

    #[test]
    fn check_run_sessions_reuse_existing_session_model() {
        let started = "2026-06-06T12:00:00Z".parse().unwrap();
        let finished = "2026-06-06T12:00:05Z".parse().unwrap();
        let outcomes = vec![
            outcome("build", CheckStatus::Pass),
            outcome("test", CheckStatus::Error),
        ];

        let session = session_from_check_run(
            ProjectType::Rust,
            &profile(),
            Path::new("/tmp/project"),
            &outcomes,
            started,
            finished,
        );

        assert_eq!(session.protocol_id, "checks-rust-quick-check");
        assert_eq!(session.protocol_title, "Rust checks: Quick Check");
        assert_eq!(session.steps[0].status, StepStatus::Passed);
        assert_eq!(session.steps[1].status, StepStatus::Failed);
        assert!(session.steps[1].note.contains("status: Error"));
    }

    #[test]
    fn slugify_handles_profile_names() {
        assert_eq!(slugify("Strict Mode"), "strict-mode");
        assert_eq!(slugify("  Full Suite! "), "full-suite");
        assert_eq!(slugify(""), "unnamed");
    }
}
