use acme::coordinator_event::{
    CoordinatorEvent, DispatchGates, EventContext, EventKind, Rejection,
    allow_automatic_dispatch, classify_event,
};
use acme::CoordinatorConfig;

fn event(kind: EventKind) -> EventContext<'static> {
    EventContext {
        kind,
        repository: "tylerbutler/acme",
        trusted_repository: "tylerbutler/acme",
        owner: "tylerbutler",
        default_branch: "main",
        ref_name: "refs/heads/main",
        actor: "tylerbutler",
        triggering_actor: "tylerbutler",
        workflow_name: None,
        workflow_repository: None,
        workflow_branch: None,
        workflow_conclusion: None,
        issue_label: None,
    }
}

#[test]
fn accepts_only_protected_main_reconciliation_events() {
    assert_eq!(
        classify_event(&event(EventKind::Push)),
        Ok(CoordinatorEvent::Automatic)
    );

    let mut build = event(EventKind::WorkflowRun);
    build.workflow_name = Some("Build cloud");
    build.workflow_repository = Some("tylerbutler/acme");
    build.workflow_branch = Some("main");
    build.workflow_conclusion = Some("success");
    assert_eq!(
        classify_event(&build),
        Ok(CoordinatorEvent::Automatic)
    );

    assert_eq!(
        classify_event(&event(EventKind::PullRequest)),
        Err(Rejection::UnsupportedEvent)
    );
    let mut foreign = event(EventKind::Push);
    foreign.ref_name = "refs/heads/feature";
    assert_eq!(
        classify_event(&foreign),
        Err(Rejection::UntrustedRef)
    );
}

#[test]
fn owner_ready_label_is_the_only_manual_issue_trigger() {
    let mut manual = event(EventKind::IssueLabeled);
    manual.issue_label = Some("ready-for-agent");
    assert_eq!(
        classify_event(&manual),
        Ok(CoordinatorEvent::Manual)
    );

    manual.issue_label = Some("needs-spec");
    assert_eq!(
        classify_event(&manual),
        Err(Rejection::UnsupportedEvent)
    );

    manual.issue_label = Some("ready-for-agent");
    manual.actor = "copilot";
    assert_eq!(
        classify_event(&manual),
        Err(Rejection::UnauthorizedActor)
    );
}

#[test]
fn workflow_run_requires_a_completed_build_on_protected_main() {
    let mut run = event(EventKind::WorkflowRun);
    run.workflow_name = Some("Build cloud");
    run.workflow_repository = Some("tylerbutler/acme");
    run.workflow_branch = Some("main");
    run.workflow_conclusion = Some("success");
    assert_eq!(
        classify_event(&run),
        Ok(CoordinatorEvent::Automatic)
    );

    run.workflow_branch = Some("pull/123/merge");
    assert_eq!(
        classify_event(&run),
        Err(Rejection::UntrustedWorkflow)
    );

    run.workflow_branch = Some("main");
    run.workflow_conclusion = Some("in_progress");
    assert_eq!(
        classify_event(&run),
        Err(Rejection::UntrustedWorkflow)
    );
}

#[test]
fn automatic_dispatch_requires_every_gate_and_positive_limits() {
    let config = CoordinatorConfig {
        automatic_dispatch: true,
        max_active_tasks: 1,
        max_assignments_per_reconciliation: 1,
        max_attempts: 1,
    };
    let enabled = DispatchGates {
        global_enabled: Some(true),
        coordinator_enabled: Some(true),
        assignment_enabled: Some(true),
    };
    assert!(allow_automatic_dispatch(&config, &enabled));

    for gates in [
        DispatchGates {
            coordinator_enabled: None,
            ..enabled
        },
        DispatchGates {
            coordinator_enabled: Some(false),
            ..enabled
        },
        DispatchGates {
            assignment_enabled: Some(false),
            ..enabled
        },
        DispatchGates {
            global_enabled: None,
            ..enabled
        },
    ] {
        assert!(!allow_automatic_dispatch(&config, &gates));
    }

    for invalid in [
        CoordinatorConfig {
            automatic_dispatch: false,
            ..config.clone()
        },
        CoordinatorConfig {
            max_active_tasks: 0,
            ..config.clone()
        },
        CoordinatorConfig {
            max_assignments_per_reconciliation: 0,
            ..config.clone()
        },
        CoordinatorConfig {
            max_attempts: 0,
            ..config
        },
    ] {
        assert!(!allow_automatic_dispatch(&invalid, &enabled));
    }
}
