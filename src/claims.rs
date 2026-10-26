use crate::CoordinatorConfig;
use crate::assignment::{
    AssignmentBlocked, AssignmentOutcome, AssignmentRequest, AssignmentTransport, Capability,
    handoff,
};
use crate::coordinator::{
    CapacityState, Eligibility, Evidence, KillSwitches, PlannerInput, ReasonCode, TaskObservation,
    plan,
};
use crate::dispatch::{DispatchDeclaration, content_sha256, valid_sha256};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    Manual,
    Automatic,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowRun {
    pub run_id: u64,
    pub run_attempt: u32,
    pub workflow_path: String,
    pub workflow_revision: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RequestBinding {
    pub repository: String,
    pub issue: u64,
    pub approved_main_revision: String,
    pub contract_sha256: String,
}

impl From<&AssignmentRequest> for RequestBinding {
    fn from(request: &AssignmentRequest) -> Self {
        Self {
            repository: request.declaration().repository.clone(),
            issue: request.declaration().issue,
            approved_main_revision: request.approved_main_revision().to_owned(),
            contract_sha256: request.contract_sha256().to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptId {
    pub binding: RequestBinding,
    pub number: u32,
    pub run: WorkflowRun,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptState {
    Pending,
    Accepted,
    NotAssigned,
    Unknown,
    Completed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryOutcome {
    NeverAssigned,
    Failed,
    Cancelled,
    Merged,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptResolution {
    pub attempt: AttemptId,
    pub outcome: RecoveryOutcome,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u32,
    pub attempt: AttemptId,
    pub route: Route,
    pub state: ReceiptState,
    pub recovery: Option<AttemptResolution>,
    pub recorded_by: WorkflowRun,
}

/// Obtain provenance from authenticated server metadata, never from receipt JSON or issue text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptProvenance {
    pub repository: String,
    pub writer: String,
    pub protected_main: bool,
    pub run: WorkflowRun,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredReceipt {
    pub receipt: Receipt,
    pub provenance: Evidence<ReceiptProvenance>,
}

/// Hold repository-wide exclusion through all reads, writes, and the assignment call.
/// A lost session must prevent further writes and assignment. Do not expire a live session.
pub trait ClaimStore {
    type Session<'a>: ClaimSession
    where
        Self: 'a;

    fn lock(&self, repository: &str) -> Result<Self::Session<'_>, String>;
}

pub trait ClaimSession {
    /// Return the complete ordered journal with independently verified provenance.
    fn load(&mut self) -> Result<Vec<StoredReceipt>, String>;

    /// Acknowledge only a durable append. An error may mean that the append succeeded.
    fn append(&mut self, receipt: &Receipt) -> Result<(), String>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActualWork {
    pub issue: u64,
    pub active_copilot: u32,
    pub open_implementation_prs: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerReady {
    pub repository: String,
    pub issue: u64,
    pub applied_by: String,
    pub label_present: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitHubSnapshot {
    pub repository: String,
    pub main_revision: String,
    pub owner_login: String,
    pub authorized_approvers: Vec<String>,
    pub config: CoordinatorConfig,
    pub kill_switches: KillSwitches,
    pub contracts: Vec<String>,
    pub observations: Vec<TaskObservation>,
    pub work: Evidence<Vec<ActualWork>>,
    pub resolutions: Evidence<Vec<AttemptResolution>>,
    pub owner_ready: Evidence<Option<OwnerReady>>,
}

/// Read fresh, complete repository state from trusted APIs and protected main.
///
/// Recovery must prove the old producer stopped and cannot create more work.
/// Absence from a list, elapsed time, or a terminal workflow alone does not prove recovery.
pub trait GitHubState {
    fn observe(&self, request: &AssignmentRequest) -> Result<GitHubSnapshot, String>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconcileContext {
    pub repository: String,
    pub trusted_workflow: String,
    pub trusted_writer: String,
    pub run: WorkflowRun,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaimBlock {
    Store(String),
    GitHub(String),
    Capability(AssignmentBlocked),
    InvalidContext,
    UntrustedReceipt,
    InvalidHistory,
    SnapshotMismatch,
    Ineligible(ReasonCode),
    OwnerGateRequired,
    ActualStateUnknown,
    RecoveryRequired,
    RunAlreadyAttempted,
    AttemptsExhausted,
    CapacityExhausted,
    ReconciliationLimitReached,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconcileResult {
    Assigned,
    NotAssigned,
    Uncertain,
    ExistingWork,
    Complete,
    Blocked(ClaimBlock),
}

pub fn reconcile(
    store: &impl ClaimStore,
    github: &impl GitHubState,
    transport: &impl AssignmentTransport,
    context: &ReconcileContext,
    request: &AssignmentRequest,
    route: Route,
) -> ReconcileResult {
    match reconcile_inner(store, github, transport, context, request, route) {
        Ok(result) => result,
        Err(reason) => ReconcileResult::Blocked(reason),
    }
}

fn reconcile_inner(
    store: &impl ClaimStore,
    github: &impl GitHubState,
    transport: &impl AssignmentTransport,
    context: &ReconcileContext,
    request: &AssignmentRequest,
    route: Route,
) -> Result<ReconcileResult, ClaimBlock> {
    let binding = RequestBinding::from(request);
    if context.repository != binding.repository
        || context.trusted_writer.trim().is_empty()
        || !valid_run(&context.run, context)
        || context.run.workflow_revision != binding.approved_main_revision
    {
        return Err(ClaimBlock::InvalidContext);
    }
    check_capabilities(transport)?;
    let mut session = store.lock(&context.repository).map_err(ClaimBlock::Store)?;
    let journal = session.load().map_err(ClaimBlock::Store)?;
    let latest = validate_journal(&journal, context)?;
    let previous = latest.get(&binding.issue).copied();
    if previous.is_some_and(|receipt| receipt.attempt.binding != binding) {
        return Err(ClaimBlock::SnapshotMismatch);
    }
    let snapshot = github.observe(request).map_err(ClaimBlock::GitHub)?;
    let work = actual_work(&snapshot, &binding)?;
    if let Some(receipt) = previous {
        if receipt.state == ReceiptState::Completed {
            return Ok(ReconcileResult::Complete);
        }
        if has_work(work, binding.issue) {
            if matches!(receipt.state, ReceiptState::Pending | ReceiptState::Unknown) {
                write_state(&mut session, receipt, ReceiptState::Accepted, context)?;
            }
            return Ok(ReconcileResult::ExistingWork);
        }
    } else if has_work(work, binding.issue) {
        return Ok(ReconcileResult::ExistingWork);
    }

    let recovery = if let Some(receipt) = previous {
        let resolution = resolution_for(&snapshot, &receipt.attempt)?;
        if resolution.outcome == RecoveryOutcome::Merged {
            write_state(&mut session, receipt, ReceiptState::Completed, context)?;
            return Ok(ReconcileResult::Complete);
        }
        if journal.iter().any(|entry| {
            entry.receipt.attempt.binding.issue == binding.issue
                && entry.receipt.attempt.run.run_id == context.run.run_id
        }) {
            return Err(ClaimBlock::RunAlreadyAttempted);
        }
        Some(resolution.clone())
    } else {
        None
    };
    let attempt_number = previous.map_or(Some(1), |receipt| receipt.attempt.number.checked_add(1));
    let attempt_number = attempt_number.ok_or(ClaimBlock::AttemptsExhausted)?;
    authorize(
        &snapshot,
        request,
        route,
        &latest,
        &journal,
        context,
        attempt_number,
    )?;

    let receipt = Receipt {
        version: 1,
        attempt: AttemptId {
            binding,
            number: attempt_number,
            run: context.run.clone(),
        },
        route,
        state: ReceiptState::Pending,
        recovery,
        recorded_by: context.run.clone(),
    };
    session.append(&receipt).map_err(ClaimBlock::Store)?;

    // Keep the durable intent if this process stops before it records the outcome.
    let final_check = github
        .observe(request)
        .map_err(ClaimBlock::GitHub)
        .and_then(|current| {
            if let Some(previous) = previous {
                let resolution = resolution_for(&current, &previous.attempt)?;
                if receipt.recovery.as_ref() != Some(resolution) {
                    return Err(ClaimBlock::RecoveryRequired);
                }
            }
            authorize(
                &current,
                request,
                route,
                &latest,
                &journal,
                context,
                attempt_number,
            )
        });
    if let Err(reason) = final_check {
        write_state(&mut session, &receipt, ReceiptState::NotAssigned, context)?;
        return Err(reason);
    }
    assign_and_record(&mut session, transport, request, &receipt, context)
}

fn assign_and_record(
    session: &mut impl ClaimSession,
    transport: &impl AssignmentTransport,
    request: &AssignmentRequest,
    receipt: &Receipt,
    context: &ReconcileContext,
) -> Result<ReconcileResult, ClaimBlock> {
    let outcome = handoff(transport, request);
    let (state, result) = match outcome {
        Ok(AssignmentOutcome::Accepted) => (ReceiptState::Accepted, ReconcileResult::Assigned),
        Ok(AssignmentOutcome::NotAssigned) => {
            (ReceiptState::NotAssigned, ReconcileResult::NotAssigned)
        }
        Ok(AssignmentOutcome::Unknown) => (ReceiptState::Unknown, ReconcileResult::Uncertain),
        Err(reason) => {
            write_state(session, receipt, ReceiptState::NotAssigned, context)?;
            return Err(ClaimBlock::Capability(reason));
        }
    };
    write_state(session, receipt, state, context)?;
    Ok(result)
}

fn check_capabilities(transport: &impl AssignmentTransport) -> Result<(), ClaimBlock> {
    let capabilities = transport.capabilities();
    for (capability, status) in [
        ("exact_target", capabilities.exact_target),
        ("immutable_task", capabilities.immutable_task),
    ] {
        if status != Capability::Supported {
            return Err(ClaimBlock::Capability(AssignmentBlocked {
                capability,
                status,
            }));
        }
    }
    Ok(())
}

fn actual_work<'a>(
    snapshot: &'a GitHubSnapshot,
    binding: &RequestBinding,
) -> Result<&'a [ActualWork], ClaimBlock> {
    if snapshot.repository != binding.repository {
        return Err(ClaimBlock::SnapshotMismatch);
    }
    match &snapshot.work {
        Evidence::Known(work) if work.iter().all(|row| row.issue > 0) => Ok(work),
        _ => Err(ClaimBlock::ActualStateUnknown),
    }
}

fn has_work(work: &[ActualWork], issue: u64) -> bool {
    work.iter().any(|row| {
        row.issue == issue && (row.active_copilot > 0 || row.open_implementation_prs > 0)
    })
}

fn resolution_for<'a>(
    snapshot: &'a GitHubSnapshot,
    attempt: &AttemptId,
) -> Result<&'a AttemptResolution, ClaimBlock> {
    let Evidence::Known(resolutions) = &snapshot.resolutions else {
        return Err(ClaimBlock::RecoveryRequired);
    };
    let mut matching = resolutions.iter().filter(|row| &row.attempt == attempt);
    match (matching.next(), matching.next()) {
        (Some(resolution), None) => Ok(resolution),
        _ => Err(ClaimBlock::RecoveryRequired),
    }
}

fn authorize(
    snapshot: &GitHubSnapshot,
    request: &AssignmentRequest,
    route: Route,
    latest: &BTreeMap<u64, &Receipt>,
    journal: &[StoredReceipt],
    context: &ReconcileContext,
    attempt_number: u32,
) -> Result<(), ClaimBlock> {
    let binding = RequestBinding::from(request);
    if snapshot.main_revision != binding.approved_main_revision {
        return Err(ClaimBlock::SnapshotMismatch);
    }
    let declarations: Vec<_> = snapshot
        .contracts
        .iter()
        .map(|contract| DispatchDeclaration::from_toml(contract))
        .collect::<Result<_, _>>()
        .map_err(|_| ClaimBlock::SnapshotMismatch)?;
    let mut matching = snapshot
        .contracts
        .iter()
        .zip(&declarations)
        .filter(|(_, declaration)| {
            declaration.repository == binding.repository && declaration.issue == binding.issue
        });
    match (matching.next(), matching.next()) {
        (Some((contract, declaration)), None)
            if content_sha256(contract) == binding.contract_sha256
                && *declaration == *request.declaration() => {}
        _ => return Err(ClaimBlock::SnapshotMismatch),
    }
    if !snapshot.observations.iter().any(|observation| {
        observation.issue == binding.issue
            && matches!(&observation.approval, Evidence::Known(Some(approval))
                if approval.main_revision == binding.approved_main_revision)
    }) {
        return Err(ClaimBlock::SnapshotMismatch);
    }
    let mut config = snapshot.config.clone();
    if route == Route::Manual {
        match &snapshot.owner_ready {
            Evidence::Known(Some(ready))
                if ready.repository == binding.repository
                    && ready.issue == binding.issue
                    && ready.label_present
                    && !snapshot.owner_login.trim().is_empty()
                    && ready.applied_by.eq_ignore_ascii_case(&snapshot.owner_login) => {}
            _ => return Err(ClaimBlock::OwnerGateRequired),
        }
        config.automatic_dispatch = true;
    }
    if attempt_number > config.max_attempts {
        return Err(ClaimBlock::AttemptsExhausted);
    }
    let work = actual_work(snapshot, &binding)?;
    if has_work(work, binding.issue) {
        return Err(ClaimBlock::Ineligible(ReasonCode::ExistingWorkConflict));
    }
    let capacity = Evidence::Known(capacity_state(
        work,
        latest,
        journal,
        context,
        binding.issue,
    ));
    let decisions = plan(&PlannerInput {
        config: &config,
        repository: &context.repository,
        owner_login: &snapshot.owner_login,
        authorized_approvers: &snapshot.authorized_approvers,
        kill_switches: &snapshot.kill_switches,
        capacity: &capacity,
        declarations: &declarations,
        observations: &snapshot.observations,
    });
    match decisions
        .iter()
        .find(|decision| decision.issue == binding.issue)
        .map(|decision| decision.eligibility)
    {
        Some(Eligibility::Eligible) => Ok(()),
        Some(Eligibility::Blocked(ReasonCode::CapacityExhausted)) => {
            Err(ClaimBlock::CapacityExhausted)
        }
        Some(Eligibility::Blocked(ReasonCode::ReconciliationLimitReached)) => {
            Err(ClaimBlock::ReconciliationLimitReached)
        }
        Some(Eligibility::Blocked(reason)) => Err(ClaimBlock::Ineligible(reason)),
        None => Err(ClaimBlock::SnapshotMismatch),
    }
}

fn capacity_state(
    work: &[ActualWork],
    latest: &BTreeMap<u64, &Receipt>,
    journal: &[StoredReceipt],
    context: &ReconcileContext,
    target_issue: u64,
) -> CapacityState {
    let mut occupied = BTreeMap::<u64, u32>::new();
    for row in work {
        let count = row
            .active_copilot
            .saturating_add(row.open_implementation_prs);
        let total = occupied.entry(row.issue).or_default();
        *total = total.saturating_add(count);
    }
    for (issue, receipt) in latest {
        if *issue != target_issue
            && !matches!(
                receipt.state,
                ReceiptState::NotAssigned | ReceiptState::Completed
            )
        {
            let count = occupied.entry(*issue).or_default();
            *count = (*count).max(1);
        }
    }
    let active_work = occupied
        .values()
        .fold(0_u32, |sum, count| sum.saturating_add(*count));
    let assignments_this_reconciliation = journal
        .iter()
        .filter(|entry| {
            entry.receipt.state == ReceiptState::Pending
                && entry.receipt.attempt.run.run_id == context.run.run_id
        })
        .fold(0_u32, |count, _| count.saturating_add(1));
    CapacityState {
        active_work,
        assignments_this_reconciliation,
    }
}

fn write_state(
    session: &mut impl ClaimSession,
    receipt: &Receipt,
    state: ReceiptState,
    context: &ReconcileContext,
) -> Result<(), ClaimBlock> {
    session
        .append(&Receipt {
            state,
            recorded_by: context.run.clone(),
            ..receipt.clone()
        })
        .map_err(ClaimBlock::Store)
}

fn valid_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_run(run: &WorkflowRun, context: &ReconcileContext) -> bool {
    run.run_id > 0
        && run.run_attempt > 0
        && run.workflow_path == context.trusted_workflow
        && run.workflow_path.starts_with(".github/workflows/")
        && std::path::Path::new(&run.workflow_path)
            .extension()
            .is_some_and(|ext| ext == "yml")
        && !run
            .workflow_path
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
        && valid_revision(&run.workflow_revision)
}

fn validate_journal<'a>(
    journal: &'a [StoredReceipt],
    context: &ReconcileContext,
) -> Result<BTreeMap<u64, &'a Receipt>, ClaimBlock> {
    let mut latest: BTreeMap<u64, &Receipt> = BTreeMap::new();
    for stored in journal {
        let receipt = &stored.receipt;
        let binding = &receipt.attempt.binding;
        let Evidence::Known(proof) = &stored.provenance else {
            return Err(ClaimBlock::UntrustedReceipt);
        };
        let trusted_writer = proof.writer == context.trusted_writer;
        if !trusted_writer
            || proof.repository != context.repository
            || !proof.protected_main
            || proof.run != receipt.recorded_by
            || !valid_run(&proof.run, context)
            || !valid_run(&receipt.attempt.run, context)
            || binding.repository != context.repository
        {
            return Err(ClaimBlock::UntrustedReceipt);
        }
        if receipt.version != 1
            || binding.issue == 0
            || !valid_revision(&binding.approved_main_revision)
            || !valid_sha256(&binding.contract_sha256)
            || receipt.attempt.run.workflow_revision != binding.approved_main_revision
        {
            return Err(ClaimBlock::InvalidHistory);
        }
        match latest.get(&binding.issue) {
            None if receipt.attempt.number == 1
                && receipt.state == ReceiptState::Pending
                && receipt.recovery.is_none()
                && receipt.recorded_by == receipt.attempt.run => {}
            Some(previous)
                if receipt.attempt == previous.attempt
                    && receipt.route == previous.route
                    && receipt.recovery == previous.recovery
                    && valid_transition(previous.state, receipt.state) => {}
            Some(previous)
                if previous.attempt.number.checked_add(1) == Some(receipt.attempt.number)
                    && previous.attempt.binding == *binding
                    && receipt.state == ReceiptState::Pending
                    && previous.state != ReceiptState::Completed
                    && receipt.attempt.run.run_id != previous.attempt.run.run_id
                    && receipt.recorded_by == receipt.attempt.run
                    && receipt.recovery.as_ref().is_some_and(|resolution| {
                        resolution.attempt == previous.attempt
                            && resolution.outcome != RecoveryOutcome::Merged
                    }) => {}
            _ => return Err(ClaimBlock::InvalidHistory),
        }
        latest.insert(binding.issue, receipt);
    }
    Ok(latest)
}

const fn valid_transition(previous: ReceiptState, current: ReceiptState) -> bool {
    matches!(
        (previous, current),
        (
            ReceiptState::Pending,
            ReceiptState::Accepted
                | ReceiptState::NotAssigned
                | ReceiptState::Unknown
                | ReceiptState::Completed
        ) | (
            ReceiptState::Unknown,
            ReceiptState::Accepted | ReceiptState::Completed
        ) | (
            ReceiptState::Accepted | ReceiptState::NotAssigned,
            ReceiptState::Completed
        )
    )
}
