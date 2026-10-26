use acme::{
    AcceptanceReceipt, ApprovalEvidence, CapacityState, CoordinatorConfig, DispatchDeclaration,
    Eligibility, Evidence, ExistingWork, IssueState, KillSwitches, PlannerInput,
    PrerequisiteCompletion, TaskDecision, TaskKind, TaskObservation, content_sha256, plan,
};

const SPEC: &str = "Approved spec text.";
const BODY: &str = "Implement a bounded slice.";
const REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn declaration(issue: u64, prerequisites: Vec<acme::Prerequisite>) -> DispatchDeclaration {
    DispatchDeclaration {
        version: 1,
        repository: "tylerbutler/acme".to_owned(),
        issue,
        spec: acme::SpecBinding {
            path: "docs/specs/0010-spec-dispatch-coordinator.md".to_owned(),
            sha256: content_sha256(SPEC),
        },
        issue_body: acme::ContentSnapshot {
            content: BODY.to_owned(),
            sha256: content_sha256(BODY),
        },
        kind: TaskKind::Implementation,
        prerequisites,
    }
}

fn observation(declaration: &DispatchDeclaration) -> TaskObservation {
    TaskObservation {
        issue: declaration.issue,
        current_spec: Evidence::Known(SPEC.to_owned()),
        current_issue_body: Evidence::Known(BODY.to_owned()),
        issue_state: Evidence::Known(IssueState::Open),
        approval: Evidence::Known(Some(ApprovalEvidence {
            repository: declaration.repository.clone(),
            issue: declaration.issue,
            spec_path: declaration.spec.path.clone(),
            spec_sha256: declaration.spec.sha256.clone(),
            issue_body_sha256: declaration.issue_body.sha256.clone(),
            kind: declaration.kind.clone(),
            prerequisites: declaration.prerequisites.clone(),
            spec_pull_request: 44,
            merged_by: "tylerbutler".to_owned(),
            main_revision: REVISION.to_owned(),
            present_on_protected_main: true,
        })),
        native_prerequisites: Evidence::Known(Vec::new()),
        existing_work: Evidence::Known(ExistingWork::default()),
        completion: Evidence::Known(None),
    }
}

struct TestPlan {
    config: CoordinatorConfig,
    declarations: Vec<DispatchDeclaration>,
    observations: Vec<TaskObservation>,
    approvers: Vec<String>,
    switches: KillSwitches,
    capacity: Evidence<CapacityState>,
}

impl TestPlan {
    fn new(declarations: Vec<DispatchDeclaration>) -> Self {
        let observations = declarations.iter().map(observation).collect();
        Self {
            config: CoordinatorConfig {
                automatic_dispatch: true,
                max_active_tasks: 10,
                max_assignments_per_reconciliation: 10,
                max_attempts: 2,
            },
            declarations,
            observations,
            approvers: vec!["tylerbutler".to_owned()],
            switches: KillSwitches {
                global: Evidence::Known(false),
                coordinator: Evidence::Known(false),
                assignment: Evidence::Known(false),
            },
            capacity: Evidence::Known(CapacityState {
                active_work: 0,
                assignments_this_reconciliation: 0,
            }),
        }
    }

    fn plan(&self) -> Vec<TaskDecision> {
        plan(&PlannerInput {
            config: &self.config,
            repository: "tylerbutler/acme",
            owner_login: "tylerbutler",
            authorized_approvers: &self.approvers,
            kill_switches: &self.switches,
            capacity: &self.capacity,
            declarations: &self.declarations,
            observations: &self.observations,
        })
    }
}

fn eligibility(decisions: &[TaskDecision], issue: u64) -> Eligibility {
    decisions
        .iter()
        .find(|decision| decision.issue == issue)
        .expect("decision should exist")
        .eligibility
}

fn prerequisite(issue: u64, owner_acceptance_required: bool) -> acme::Prerequisite {
    acme::Prerequisite {
        issue,
        owner_acceptance_required,
    }
}

#[test]
fn independently_approved_implementation_tasks_are_eligible() {
    let state = TestPlan::new(vec![
        declaration(43, Vec::new()),
        declaration(44, Vec::new()),
    ]);

    let decisions = state.plan();

    assert_eq!(eligibility(&decisions, 43), Eligibility::Eligible);
    assert_eq!(eligibility(&decisions, 44), Eligibility::Eligible);
}

#[test]
fn dependents_wait_for_merged_prerequisites_and_owner_receipts() {
    let mut state = TestPlan::new(vec![
        declaration(43, Vec::new()),
        declaration(44, vec![prerequisite(43, true)]),
    ]);
    state.observations[0].completion = Evidence::Known(Some(PrerequisiteCompletion {
        repository: "tylerbutler/acme".to_owned(),
        issue: 43,
        implementation_pull_request: 51,
        merge_commit: REVISION.to_owned(),
        merged_by: "tylerbutler".to_owned(),
        acceptance: None,
    }));

    assert_eq!(
        eligibility(&state.plan(), 44),
        Eligibility::Blocked(acme::ReasonCode::OwnerAcceptanceMissing)
    );

    state.observations[0].completion = Evidence::Known(Some(PrerequisiteCompletion {
        repository: "tylerbutler/acme".to_owned(),
        issue: 43,
        implementation_pull_request: 51,
        merge_commit: REVISION.to_owned(),
        merged_by: "tylerbutler".to_owned(),
        acceptance: Some(AcceptanceReceipt {
            issue: 43,
            merge_commit: REVISION.to_owned(),
            accepted_by: "tylerbutler".to_owned(),
            receipt_id: "main:receipt-1".to_owned(),
        }),
    }));

    assert_eq!(eligibility(&state.plan(), 44), Eligibility::Eligible);
}

#[test]
fn capacity_counts_existing_work_and_planned_assignments_deterministically() {
    let mut state = TestPlan::new(vec![
        declaration(44, Vec::new()),
        declaration(43, Vec::new()),
    ]);
    state.config.max_active_tasks = 2;
    state.config.max_assignments_per_reconciliation = 2;
    state.capacity = Evidence::Known(CapacityState {
        active_work: 1,
        assignments_this_reconciliation: 0,
    });

    let decisions = state.plan();

    assert_eq!(eligibility(&decisions, 43), Eligibility::Eligible);
    assert_eq!(
        eligibility(&decisions, 44),
        Eligibility::Blocked(acme::ReasonCode::CapacityExhausted)
    );
}

#[test]
fn stale_scope_unauthorized_approval_and_trackers_fail_closed() {
    let tracker = DispatchDeclaration {
        kind: TaskKind::PlanningTracker,
        ..declaration(45, Vec::new())
    };
    let mut state = TestPlan::new(vec![declaration(43, Vec::new()), tracker]);
    state.observations[0].current_issue_body = Evidence::Known("Changed scope".to_owned());
    state.observations[1].approval = Evidence::Known(Some(ApprovalEvidence {
        merged_by: "untrusted-user".to_owned(),
        ..match state.observations[1].approval.clone() {
            Evidence::Known(Some(approval)) => approval,
            _ => unreachable!(),
        }
    }));

    let decisions = state.plan();

    assert_eq!(
        eligibility(&decisions, 43),
        Eligibility::Blocked(acme::ReasonCode::IssueBodyChanged)
    );
    assert_eq!(
        eligibility(&decisions, 45),
        Eligibility::Blocked(acme::ReasonCode::UnsupportedTaskKind)
    );
}

#[test]
fn unknown_errors_kill_switches_and_weak_dependencies_block() {
    let mut state = TestPlan::new(vec![declaration(43, Vec::new())]);
    state.observations[0].issue_state = Evidence::Unknown;
    assert_eq!(
        eligibility(&state.plan(), 43),
        Eligibility::Blocked(acme::ReasonCode::IssueStateUnknown)
    );

    state.observations[0].issue_state = Evidence::Known(IssueState::Open);
    state.observations[0].native_prerequisites = Evidence::Known(vec![99]);
    assert_eq!(
        eligibility(&state.plan(), 43),
        Eligibility::Blocked(acme::ReasonCode::WeakenedPrerequisites)
    );

    state.observations[0].native_prerequisites = Evidence::Known(Vec::new());
    state.switches.assignment = Evidence::Error;
    assert_eq!(
        eligibility(&state.plan(), 43),
        Eligibility::Blocked(acme::ReasonCode::AssignmentKillSwitchError)
    );

    state.switches.assignment = Evidence::Known(false);
    state.capacity = Evidence::Unknown;
    assert_eq!(
        eligibility(&state.plan(), 43),
        Eligibility::Blocked(acme::ReasonCode::CapacityUnknown)
    );
}

#[test]
fn cycles_and_invalid_activation_policy_are_blocked() {
    let mut state = TestPlan::new(vec![
        declaration(43, vec![prerequisite(44, false)]),
        declaration(44, vec![prerequisite(43, false)]),
    ]);
    assert_eq!(
        eligibility(&state.plan(), 43),
        Eligibility::Blocked(acme::ReasonCode::DependencyCycle)
    );

    state.declarations = vec![declaration(43, Vec::new())];
    state.observations = state.declarations.iter().map(observation).collect();
    state.config.max_attempts = 0;
    assert_eq!(
        eligibility(&state.plan(), 43),
        Eligibility::Blocked(acme::ReasonCode::InvalidCapacityPolicy)
    );
}
