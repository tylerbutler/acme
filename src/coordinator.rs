use crate::config::CoordinatorConfig;
use crate::dispatch::{DispatchDeclaration, Prerequisite, TaskKind, content_sha256, valid_sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Evidence<T> {
    Known(T),
    Unknown,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalEvidence {
    pub repository: String,
    pub issue: u64,
    pub spec_path: String,
    pub spec_sha256: String,
    pub issue_body_sha256: String,
    pub kind: TaskKind,
    pub prerequisites: Vec<Prerequisite>,
    pub spec_pull_request: u64,
    pub merged_by: String,
    pub main_revision: String,
    pub present_on_protected_main: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptanceReceipt {
    pub issue: u64,
    pub merge_commit: String,
    pub accepted_by: String,
    pub receipt_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrerequisiteCompletion {
    pub repository: String,
    pub issue: u64,
    pub implementation_pull_request: u64,
    pub merge_commit: String,
    pub merged_by: String,
    pub acceptance: Option<AcceptanceReceipt>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IssueState {
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExistingWork {
    pub active_claims: u32,
    pub active_assignments: u32,
    pub open_unmerged_implementation_prs: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CapacityState {
    pub active_work: u32,
    pub assignments_this_reconciliation: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskObservation {
    pub issue: u64,
    pub current_spec: Evidence<String>,
    pub current_issue_body: Evidence<String>,
    pub issue_state: Evidence<IssueState>,
    pub approval: Evidence<Option<ApprovalEvidence>>,
    pub native_prerequisites: Evidence<Vec<u64>>,
    pub existing_work: Evidence<ExistingWork>,
    pub completion: Evidence<Option<PrerequisiteCompletion>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KillSwitches {
    pub global: Evidence<bool>,
    pub coordinator: Evidence<bool>,
    pub assignment: Evidence<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannerInput<'a> {
    pub config: &'a CoordinatorConfig,
    pub repository: &'a str,
    pub owner_login: &'a str,
    pub authorized_approvers: &'a [String],
    pub kill_switches: &'a KillSwitches,
    pub capacity: &'a Evidence<CapacityState>,
    pub declarations: &'a [DispatchDeclaration],
    pub observations: &'a [TaskObservation],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReasonCode {
    AutomaticDispatchDisabled,
    InvalidCapacityPolicy,
    GlobalKillSwitchActive,
    GlobalKillSwitchUnknown,
    GlobalKillSwitchError,
    CoordinatorKillSwitchActive,
    CoordinatorKillSwitchUnknown,
    CoordinatorKillSwitchError,
    AssignmentKillSwitchActive,
    AssignmentKillSwitchUnknown,
    AssignmentKillSwitchError,
    CapacityUnknown,
    CapacityError,
    CapacityExhausted,
    ReconciliationLimitReached,
    InvalidDeclaration,
    RepositoryMismatch,
    UnsupportedTaskKind,
    DuplicateDeclaration,
    MissingObservation,
    AmbiguousObservation,
    SpecContentUnknown,
    SpecContentError,
    SpecChanged,
    IssueBodyUnknown,
    IssueBodyError,
    IssueBodyChanged,
    ApprovalUnknown,
    ApprovalError,
    ApprovalMissing,
    ApprovalMismatch,
    UnprotectedApproval,
    UnauthorizedApprover,
    IssueStateUnknown,
    IssueStateError,
    IssueNotOpen,
    NativePrerequisitesUnknown,
    NativePrerequisitesError,
    WeakenedPrerequisites,
    ExistingWorkUnknown,
    ExistingWorkError,
    ExistingWorkConflict,
    MissingPrerequisite,
    AmbiguousPrerequisite,
    DependencyCycle,
    PrerequisiteEvidenceUnknown,
    PrerequisiteEvidenceError,
    PrerequisiteNotMerged,
    UnauthorizedPrerequisiteMerge,
    OwnerAcceptanceMissing,
    InvalidOwnerAcceptance,
}

impl ReasonCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutomaticDispatchDisabled => "automatic_dispatch_disabled",
            Self::InvalidCapacityPolicy => "invalid_capacity_policy",
            Self::GlobalKillSwitchActive => "global_kill_switch_active",
            Self::GlobalKillSwitchUnknown => "global_kill_switch_unknown",
            Self::GlobalKillSwitchError => "global_kill_switch_error",
            Self::CoordinatorKillSwitchActive => "coordinator_kill_switch_active",
            Self::CoordinatorKillSwitchUnknown => "coordinator_kill_switch_unknown",
            Self::CoordinatorKillSwitchError => "coordinator_kill_switch_error",
            Self::AssignmentKillSwitchActive => "assignment_kill_switch_active",
            Self::AssignmentKillSwitchUnknown => "assignment_kill_switch_unknown",
            Self::AssignmentKillSwitchError => "assignment_kill_switch_error",
            Self::CapacityUnknown => "capacity_unknown",
            Self::CapacityError => "capacity_error",
            Self::CapacityExhausted => "capacity_exhausted",
            Self::ReconciliationLimitReached => "reconciliation_limit_reached",
            Self::InvalidDeclaration => "invalid_declaration",
            Self::RepositoryMismatch => "repository_mismatch",
            Self::UnsupportedTaskKind => "unsupported_task_kind",
            Self::DuplicateDeclaration => "duplicate_declaration",
            Self::MissingObservation => "missing_observation",
            Self::AmbiguousObservation => "ambiguous_observation",
            Self::SpecContentUnknown => "spec_content_unknown",
            Self::SpecContentError => "spec_content_error",
            Self::SpecChanged => "spec_changed",
            Self::IssueBodyUnknown => "issue_body_unknown",
            Self::IssueBodyError => "issue_body_error",
            Self::IssueBodyChanged => "issue_body_changed",
            Self::ApprovalUnknown => "approval_unknown",
            Self::ApprovalError => "approval_error",
            Self::ApprovalMissing => "approval_missing",
            Self::ApprovalMismatch => "approval_mismatch",
            Self::UnprotectedApproval => "unprotected_approval",
            Self::UnauthorizedApprover => "unauthorized_approver",
            Self::IssueStateUnknown => "issue_state_unknown",
            Self::IssueStateError => "issue_state_error",
            Self::IssueNotOpen => "issue_not_open",
            Self::NativePrerequisitesUnknown => "native_prerequisites_unknown",
            Self::NativePrerequisitesError => "native_prerequisites_error",
            Self::WeakenedPrerequisites => "weakened_prerequisites",
            Self::ExistingWorkUnknown => "existing_work_unknown",
            Self::ExistingWorkError => "existing_work_error",
            Self::ExistingWorkConflict => "existing_work_conflict",
            Self::MissingPrerequisite => "missing_prerequisite",
            Self::AmbiguousPrerequisite => "ambiguous_prerequisite",
            Self::DependencyCycle => "dependency_cycle",
            Self::PrerequisiteEvidenceUnknown => "prerequisite_evidence_unknown",
            Self::PrerequisiteEvidenceError => "prerequisite_evidence_error",
            Self::PrerequisiteNotMerged => "prerequisite_not_merged",
            Self::UnauthorizedPrerequisiteMerge => "unauthorized_prerequisite_merge",
            Self::OwnerAcceptanceMissing => "owner_acceptance_missing",
            Self::InvalidOwnerAcceptance => "invalid_owner_acceptance",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Eligibility {
    Eligible,
    Blocked(ReasonCode),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskDecision {
    pub issue: u64,
    pub eligibility: Eligibility,
}

/// Plan a batch in issue order and reserve capacity for each eligible task.
#[must_use]
pub fn plan(input: &PlannerInput<'_>) -> Vec<TaskDecision> {
    let graph = TaskGraph::new(input.declarations, input.observations);
    let (mut used_active_work, mut used_assignments) = known_capacity(input.capacity);
    let mut decisions = Vec::with_capacity(input.declarations.len());
    let mut ordered: Vec<_> = input.declarations.iter().collect();
    ordered.sort_by_key(|declaration| declaration.issue);

    for declaration in ordered {
        let eligibility = capacity_eligibility(
            task_reason(declaration, input, &graph),
            input,
            used_active_work,
            used_assignments,
        );
        if eligibility == Eligibility::Eligible {
            used_active_work += 1;
            used_assignments += 1;
        }
        decisions.push(TaskDecision {
            issue: declaration.issue,
            eligibility,
        });
    }
    decisions
}

/// Check one selected task without reserving slots for other eligible tasks.
pub(crate) fn target_eligibility(input: &PlannerInput<'_>, issue: u64) -> Eligibility {
    let graph = TaskGraph::new(input.declarations, input.observations);
    let Some(declaration) = graph.declarations.get(&issue).and_then(|rows| rows.first()) else {
        return Eligibility::Blocked(ReasonCode::InvalidDeclaration);
    };
    let (active_work, assignments) = known_capacity(input.capacity);
    capacity_eligibility(
        task_reason(declaration, input, &graph),
        input,
        active_work,
        assignments,
    )
}

struct TaskGraph<'a> {
    declarations: BTreeMap<u64, Vec<&'a DispatchDeclaration>>,
    duplicates: HashSet<u64>,
    observations: HashMap<u64, Vec<&'a TaskObservation>>,
    cycles: HashSet<u64>,
}

impl<'a> TaskGraph<'a> {
    fn new(declarations: &'a [DispatchDeclaration], observations: &'a [TaskObservation]) -> Self {
        let mut declarations_by_issue: BTreeMap<u64, Vec<&DispatchDeclaration>> = BTreeMap::new();
        for declaration in declarations {
            declarations_by_issue
                .entry(declaration.issue)
                .or_default()
                .push(declaration);
        }
        let duplicate_issues: HashSet<u64> = declarations_by_issue
            .iter()
            .filter_map(|(issue, declarations)| (declarations.len() > 1).then_some(*issue))
            .collect();
        let cycles = dependency_cycles(&declarations_by_issue, &duplicate_issues);
        Self {
            declarations: declarations_by_issue,
            duplicates: duplicate_issues,
            observations: observations_by_issue(observations),
            cycles,
        }
    }
}

const fn known_capacity(capacity: &Evidence<CapacityState>) -> (u32, u32) {
    match capacity {
        Evidence::Known(value) => (value.active_work, value.assignments_this_reconciliation),
        Evidence::Unknown | Evidence::Error => (0, 0),
    }
}

fn task_reason(
    declaration: &DispatchDeclaration,
    input: &PlannerInput<'_>,
    graph: &TaskGraph<'_>,
) -> Option<ReasonCode> {
    global_block(input)
        .or(match input.capacity {
            Evidence::Unknown => Some(ReasonCode::CapacityUnknown),
            Evidence::Error => Some(ReasonCode::CapacityError),
            Evidence::Known(_) => None,
        })
        .or_else(|| {
            if graph.duplicates.contains(&declaration.issue) {
                Some(ReasonCode::DuplicateDeclaration)
            } else {
                None
            }
        })
        .or_else(|| {
            if graph.cycles.contains(&declaration.issue) {
                Some(ReasonCode::DependencyCycle)
            } else {
                None
            }
        })
        .or_else(|| {
            declaration_reason(
                declaration,
                input,
                &graph.observations,
                &graph.declarations,
                &graph.duplicates,
                &graph.cycles,
            )
        })
}

const fn capacity_eligibility(
    reason: Option<ReasonCode>,
    input: &PlannerInput<'_>,
    used_active_work: u32,
    used_assignments: u32,
) -> Eligibility {
    match reason {
        Some(reason) => Eligibility::Blocked(reason),
        None if used_active_work >= input.config.max_active_tasks => {
            Eligibility::Blocked(ReasonCode::CapacityExhausted)
        }
        None if used_assignments >= input.config.max_assignments_per_reconciliation => {
            Eligibility::Blocked(ReasonCode::ReconciliationLimitReached)
        }
        None => Eligibility::Eligible,
    }
}

fn global_block(input: &PlannerInput<'_>) -> Option<ReasonCode> {
    if !input.config.automatic_dispatch {
        return Some(ReasonCode::AutomaticDispatchDisabled);
    }
    if input.config.max_active_tasks == 0
        || input.config.max_assignments_per_reconciliation == 0
        || input.config.max_attempts == 0
    {
        return Some(ReasonCode::InvalidCapacityPolicy);
    }
    for (value, active, unknown, error) in [
        (
            &input.kill_switches.global,
            ReasonCode::GlobalKillSwitchActive,
            ReasonCode::GlobalKillSwitchUnknown,
            ReasonCode::GlobalKillSwitchError,
        ),
        (
            &input.kill_switches.coordinator,
            ReasonCode::CoordinatorKillSwitchActive,
            ReasonCode::CoordinatorKillSwitchUnknown,
            ReasonCode::CoordinatorKillSwitchError,
        ),
        (
            &input.kill_switches.assignment,
            ReasonCode::AssignmentKillSwitchActive,
            ReasonCode::AssignmentKillSwitchUnknown,
            ReasonCode::AssignmentKillSwitchError,
        ),
    ] {
        match value {
            Evidence::Known(true) => return Some(active),
            Evidence::Unknown => return Some(unknown),
            Evidence::Error => return Some(error),
            Evidence::Known(false) => {}
        }
    }
    None
}

fn declaration_reason(
    declaration: &DispatchDeclaration,
    input: &PlannerInput<'_>,
    observations: &HashMap<u64, Vec<&TaskObservation>>,
    declarations: &BTreeMap<u64, Vec<&DispatchDeclaration>>,
    duplicate_issues: &HashSet<u64>,
    cycles: &HashSet<u64>,
) -> Option<ReasonCode> {
    if declaration.validate().is_err() {
        return Some(ReasonCode::InvalidDeclaration);
    }
    if declaration.repository != input.repository {
        return Some(ReasonCode::RepositoryMismatch);
    }
    if declaration.kind != TaskKind::Implementation {
        return Some(ReasonCode::UnsupportedTaskKind);
    }

    let observation = match observations.get(&declaration.issue).map(Vec::as_slice) {
        None => return Some(ReasonCode::MissingObservation),
        Some([observation]) => *observation,
        Some(_) => return Some(ReasonCode::AmbiguousObservation),
    };

    observation_reason(declaration, observation, input).or_else(|| {
        prerequisite_reason(
            declaration,
            input,
            observations,
            declarations,
            duplicate_issues,
            cycles,
        )
    })
}

fn observation_reason(
    declaration: &DispatchDeclaration,
    observation: &TaskObservation,
    input: &PlannerInput<'_>,
) -> Option<ReasonCode> {
    match &observation.current_spec {
        Evidence::Known(content) if content_sha256(content) == declaration.spec.sha256 => {}
        Evidence::Known(_) => return Some(ReasonCode::SpecChanged),
        Evidence::Unknown => return Some(ReasonCode::SpecContentUnknown),
        Evidence::Error => return Some(ReasonCode::SpecContentError),
    }
    match &observation.current_issue_body {
        Evidence::Known(content) if content_sha256(content) == declaration.issue_body.sha256 => {}
        Evidence::Known(_) => return Some(ReasonCode::IssueBodyChanged),
        Evidence::Unknown => return Some(ReasonCode::IssueBodyUnknown),
        Evidence::Error => return Some(ReasonCode::IssueBodyError),
    }
    if let Some(reason) = approval_reason(declaration, observation, input) {
        return Some(reason);
    }
    match observation.issue_state {
        Evidence::Known(IssueState::Open) => {}
        Evidence::Known(IssueState::Closed) => return Some(ReasonCode::IssueNotOpen),
        Evidence::Unknown => return Some(ReasonCode::IssueStateUnknown),
        Evidence::Error => return Some(ReasonCode::IssueStateError),
    }
    match &observation.native_prerequisites {
        Evidence::Known(native) => {
            let declared: BTreeSet<_> = declaration
                .prerequisites
                .iter()
                .map(|prerequisite| prerequisite.issue)
                .collect();
            if native.iter().any(|issue| !declared.contains(issue)) {
                return Some(ReasonCode::WeakenedPrerequisites);
            }
        }
        Evidence::Unknown => return Some(ReasonCode::NativePrerequisitesUnknown),
        Evidence::Error => return Some(ReasonCode::NativePrerequisitesError),
    }
    match observation.existing_work {
        Evidence::Known(work)
            if work.active_claims == 0
                && work.active_assignments == 0
                && work.open_unmerged_implementation_prs == 0 => {}
        Evidence::Known(_) => return Some(ReasonCode::ExistingWorkConflict),
        Evidence::Unknown => return Some(ReasonCode::ExistingWorkUnknown),
        Evidence::Error => return Some(ReasonCode::ExistingWorkError),
    }
    None
}

fn approval_reason(
    declaration: &DispatchDeclaration,
    observation: &TaskObservation,
    input: &PlannerInput<'_>,
) -> Option<ReasonCode> {
    match &observation.approval {
        Evidence::Unknown => return Some(ReasonCode::ApprovalUnknown),
        Evidence::Error => return Some(ReasonCode::ApprovalError),
        Evidence::Known(None) => return Some(ReasonCode::ApprovalMissing),
        Evidence::Known(Some(approval)) => {
            if !approval_matches(declaration, approval) {
                return Some(ReasonCode::ApprovalMismatch);
            }
            if !approval.present_on_protected_main
                || approval.spec_pull_request == 0
                || !valid_sha256(&approval.spec_sha256)
                || !valid_revision(&approval.main_revision)
            {
                return Some(ReasonCode::UnprotectedApproval);
            }
            if !contains_identity(input.authorized_approvers, &approval.merged_by) {
                return Some(ReasonCode::UnauthorizedApprover);
            }
        }
    }
    None
}

fn prerequisite_reason(
    declaration: &DispatchDeclaration,
    input: &PlannerInput<'_>,
    observations: &HashMap<u64, Vec<&TaskObservation>>,
    declarations: &BTreeMap<u64, Vec<&DispatchDeclaration>>,
    duplicate_issues: &HashSet<u64>,
    cycles: &HashSet<u64>,
) -> Option<ReasonCode> {
    for prerequisite in &declaration.prerequisites {
        let Some(rows) = declarations.get(&prerequisite.issue) else {
            return Some(ReasonCode::MissingPrerequisite);
        };
        if rows.len() != 1 || duplicate_issues.contains(&prerequisite.issue) {
            return Some(ReasonCode::AmbiguousPrerequisite);
        }
        if rows[0].kind != TaskKind::Implementation {
            return Some(ReasonCode::UnsupportedTaskKind);
        }
        if cycles.contains(&prerequisite.issue) {
            return Some(ReasonCode::DependencyCycle);
        }
        let prerequisite_observation =
            match observations.get(&prerequisite.issue).map(Vec::as_slice) {
                None | Some([]) => return Some(ReasonCode::MissingObservation),
                Some([observation]) => *observation,
                Some(_) => return Some(ReasonCode::AmbiguousObservation),
            };
        match &prerequisite_observation.completion {
            Evidence::Unknown => return Some(ReasonCode::PrerequisiteEvidenceUnknown),
            Evidence::Error => return Some(ReasonCode::PrerequisiteEvidenceError),
            Evidence::Known(None) => return Some(ReasonCode::PrerequisiteNotMerged),
            Evidence::Known(Some(completion)) => {
                if completion.issue != prerequisite.issue || !valid_completion(completion, input) {
                    return Some(ReasonCode::PrerequisiteNotMerged);
                }
                if !contains_identity(input.authorized_approvers, &completion.merged_by) {
                    return Some(ReasonCode::UnauthorizedPrerequisiteMerge);
                }
                if prerequisite.owner_acceptance_required {
                    let Some(acceptance) = &completion.acceptance else {
                        return Some(ReasonCode::OwnerAcceptanceMissing);
                    };
                    if acceptance.issue != prerequisite.issue
                        || acceptance.merge_commit != completion.merge_commit
                        || !acceptance
                            .accepted_by
                            .eq_ignore_ascii_case(input.owner_login)
                        || input.owner_login.trim().is_empty()
                        || acceptance.receipt_id.trim().is_empty()
                    {
                        return Some(ReasonCode::InvalidOwnerAcceptance);
                    }
                }
            }
        }
    }
    None
}

fn approval_matches(declaration: &DispatchDeclaration, approval: &ApprovalEvidence) -> bool {
    declaration.repository == approval.repository
        && declaration.issue == approval.issue
        && declaration.spec.path == approval.spec_path
        && declaration.spec.sha256 == approval.spec_sha256
        && declaration.issue_body.sha256 == approval.issue_body_sha256
        && declaration.kind == approval.kind
        && declaration.prerequisites == approval.prerequisites
}

fn valid_completion(completion: &PrerequisiteCompletion, input: &PlannerInput<'_>) -> bool {
    completion.repository == input.repository
        && completion.issue > 0
        && completion.implementation_pull_request > 0
        && valid_revision(&completion.merge_commit)
        && !completion.merged_by.trim().is_empty()
}

fn valid_revision(value: &str) -> bool {
    (40..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn contains_identity(identities: &[String], login: &str) -> bool {
    !login.trim().is_empty()
        && identities
            .iter()
            .any(|identity| !identity.trim().is_empty() && identity.eq_ignore_ascii_case(login))
}

fn observations_by_issue(observations: &[TaskObservation]) -> HashMap<u64, Vec<&TaskObservation>> {
    let mut by_issue = HashMap::new();
    for observation in observations {
        by_issue
            .entry(observation.issue)
            .or_insert_with(Vec::new)
            .push(observation);
    }
    by_issue
}

fn dependency_cycles(
    declarations: &BTreeMap<u64, Vec<&DispatchDeclaration>>,
    duplicate_issues: &HashSet<u64>,
) -> HashSet<u64> {
    let mut colors: HashMap<u64, u8> = HashMap::new();
    let mut cycles = HashSet::new();
    for issue in declarations.keys().copied() {
        if duplicate_issues.contains(&issue) || colors.get(&issue).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut stack = vec![(issue, 0_usize)];
        colors.insert(issue, 1);
        while let Some((current, next_index)) = stack.last_mut() {
            let dependencies = declarations
                .get(current)
                .and_then(|rows| rows.first())
                .map_or(&[][..], |declaration| declaration.prerequisites.as_slice());
            if *next_index == dependencies.len() {
                colors.insert(*current, 2);
                stack.pop();
                continue;
            }
            let dependency = dependencies[*next_index].issue;
            *next_index += 1;
            if !declarations.contains_key(&dependency) || duplicate_issues.contains(&dependency) {
                continue;
            }
            match colors.get(&dependency).copied().unwrap_or(0) {
                0 => {
                    colors.insert(dependency, 1);
                    stack.push((dependency, 0));
                }
                1 => {
                    if let Some(start) = stack.iter().position(|(node, _)| *node == dependency) {
                        cycles.extend(stack[start..].iter().map(|(node, _)| *node));
                    }
                }
                _ => {}
            }
        }
    }
    cycles
}
