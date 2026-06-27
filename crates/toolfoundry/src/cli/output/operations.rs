use toolfoundry_core::{
    health::HealthReport,
    install::{DriftReport, InstallApplyReport, InstallPlan},
    lifecycle::{LifecycleReport, LifecycleTransitionReport},
};

use super::labels::{
    drift_status_label, exists_label, health_status_label, install_action_kind_label,
    install_plan_status_label, link_status_label, review_status_label,
};

pub fn print_install_plan(plan: &InstallPlan) {
    println!(
        "install plan: {} ({} actions, {}, dry_run={})",
        plan.tool_id,
        plan.action_count,
        install_plan_status_label(plan.status),
        plan.dry_run
    );

    if plan.actions.is_empty() {
        println!("noop: install state already matches manifest");
        return;
    }

    for action in &plan.actions {
        match &action.source {
            Some(source) => println!(
                "{}: {} -> {} - {}",
                install_action_kind_label(action.kind),
                source,
                action.target,
                action.message
            ),
            None => println!(
                "{}: {} - {}",
                install_action_kind_label(action.kind),
                action.target,
                action.message
            ),
        }
    }
}

pub fn print_install_apply_report(report: &InstallApplyReport) {
    println!(
        "install applied: {} ({} planned actions)",
        report.tool_id, report.planned_count
    );

    for action in &report.actions {
        println!(
            "{} | target={} | source={} | {}",
            install_action_kind_label(action.kind),
            action.target,
            action.source.as_deref().unwrap_or("-"),
            action.message
        );
    }

    println!(
        "final drift: {}",
        drift_status_label(report.final_drift.status)
    );
}

pub fn print_health_report(report: &HealthReport) {
    let status = if report.is_healthy() {
        "healthy"
    } else {
        "unhealthy"
    };

    println!(
        "health report: {} ({}/{}, {status})",
        report.tool_id,
        report.passed_count(),
        report.outcomes.len()
    );

    for outcome in &report.outcomes {
        println!(
            "{}: {} - {}",
            health_status_label(outcome.status),
            outcome.id,
            outcome.message
        );
    }
}

pub fn print_lifecycle_report(report: &LifecycleReport) {
    println!(
        "lifecycle report: {} ({}, review {})",
        report.tool_id,
        report.state,
        review_status_label(report.review_status)
    );
    println!("review_after: {}", report.review_after);
    println!("as_of: {}", report.as_of);

    if report.allowed_next_states.is_empty() {
        println!("allowed_next_states: none");
    } else {
        let states = report
            .allowed_next_states
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        println!("allowed_next_states: {states}");
    }

    if let Some(replacement) = &report.replacement {
        println!("replacement: {replacement}");
    }
}

pub fn print_lifecycle_transition_report(report: &LifecycleTransitionReport) {
    let status = if report.allowed { "allowed" } else { "blocked" };
    let allowed_next_states = report
        .allowed_next_states
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");

    println!(
        "lifecycle transition: {} | {} -> {} | {}",
        report.tool_id, report.from, report.to, status
    );
    println!("allowed next states: {allowed_next_states}");
}

pub fn print_drift_report(report: &DriftReport) {
    let status = drift_status_label(report.status);

    println!(
        "drift report: {} ({}/{}, {status})",
        report.tool_id,
        report.current_link_count(),
        report.links.len()
    );
    println!(
        "artifact: {}",
        exists_label(report.artifact_exists, &report.artifact_path)
    );
    println!(
        "target: {}",
        exists_label(report.target_exists, &report.target_path)
    );

    for link in &report.links {
        println!(
            "{}: {} -> {} - {}",
            link_status_label(link.status),
            link.source,
            link.target,
            link.message
        );
    }
}
