#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventKind {
    Push,
    WorkflowRun,
    WorkflowDispatch,
    IssueLabeled,
    PullRequest,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinatorEvent {
    Automatic,
    Manual,
    OwnerReconciliation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rejection {
    UntrustedRepository,
    UntrustedRef,
    UnauthorizedActor,
    UntrustedWorkflow,
    UnsupportedEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventContext<'a> {
    pub kind: EventKind,
    pub repository: &'a str,
    pub trusted_repository: &'a str,
    pub owner: &'a str,
    pub default_branch: &'a str,
    pub ref_name: &'a str,
    pub actor: &'a str,
    pub triggering_actor: &'a str,
    pub workflow_name: Option<&'a str>,
    pub workflow_repository: Option<&'a str>,
    pub workflow_branch: Option<&'a str>,
    pub workflow_conclusion: Option<&'a str>,
    pub issue_label: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchGates {
    pub global_enabled: Option<bool>,
    pub coordinator_enabled: Option<bool>,
    pub assignment_enabled: Option<bool>,
}

#[must_use]
pub const fn allow_automatic_dispatch(config: &CoordinatorConfig, gates: &DispatchGates) -> bool {
    config.automatic_dispatch
        && config.max_active_tasks > 0
        && config.max_assignments_per_reconciliation > 0
        && config.max_attempts > 0
        && matches!(gates.global_enabled, Some(true))
        && matches!(gates.coordinator_enabled, Some(true))
        && matches!(gates.assignment_enabled, Some(true))
}

pub fn classify_event(event: &EventContext<'_>) -> Result<CoordinatorEvent, Rejection> {
    if event.repository != event.trusted_repository {
        return Err(Rejection::UntrustedRepository);
    }

    match event.kind {
        EventKind::Push => {
            require_main_ref(event)?;
            Ok(CoordinatorEvent::Automatic)
        }
        EventKind::WorkflowRun => {
            if event.workflow_name != Some("Build cloud")
                || event.workflow_repository != Some(event.trusted_repository)
                || event.workflow_branch != Some(event.default_branch)
                || !matches!(
                    event.workflow_conclusion,
                    Some(
                        "success"
                            | "failure"
                            | "cancelled"
                            | "timed_out"
                            | "action_required"
                            | "skipped"
                            | "neutral"
                            | "stale"
                    )
                )
            {
                return Err(Rejection::UntrustedWorkflow);
            }
            Ok(CoordinatorEvent::Automatic)
        }
        EventKind::WorkflowDispatch => {
            require_main_ref(event)?;
            require_owner(event)?;
            Ok(CoordinatorEvent::OwnerReconciliation)
        }
        EventKind::IssueLabeled => {
            require_main_ref(event)?;
            if event.issue_label != Some("ready-for-agent") {
                return Err(Rejection::UnsupportedEvent);
            }
            require_owner(event)?;
            Ok(CoordinatorEvent::Manual)
        }
        EventKind::PullRequest | EventKind::Other => Err(Rejection::UnsupportedEvent),
    }
}

fn require_main_ref(event: &EventContext<'_>) -> Result<(), Rejection> {
    let expected = format!("refs/heads/{}", event.default_branch);
    if event.ref_name != expected {
        return Err(Rejection::UntrustedRef);
    }
    Ok(())
}

fn require_owner(event: &EventContext<'_>) -> Result<(), Rejection> {
    if !event.actor.eq_ignore_ascii_case(event.owner)
        || !event.triggering_actor.eq_ignore_ascii_case(event.owner)
    {
        return Err(Rejection::UnauthorizedActor);
    }
    Ok(())
}
use crate::config::CoordinatorConfig;
