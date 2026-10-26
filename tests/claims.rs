use acme::assignment::{
    AssignmentCapabilities, AssignmentOutcome, AssignmentRequest, AssignmentTransport, Capability,
};
use acme::claims::{
    ActualWork, AttemptResolution, ClaimBlock, ClaimSession, ClaimStore, GitHubSnapshot,
    GitHubState, OwnerReady, Receipt, ReceiptProvenance, ReceiptState, ReconcileContext,
    ReconcileResult, RecoveryOutcome, Route, StoredReceipt, WorkflowRun, reconcile,
};
use acme::{
    ApprovalEvidence, CoordinatorConfig, Evidence, ExistingWork, IssueState, KillSwitches,
    TaskObservation, content_sha256,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

const REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SPEC: &str = "Approved claims fixture.";
const BODY: &str = "Implement the offline claims fixture.";

fn request(issue: u64) -> AssignmentRequest {
    AssignmentRequest::from_approved_snapshot(&contract(issue), REVISION).unwrap()
}

fn contract(issue: u64) -> String {
    format!(
        r#"version = 1
repository = "proof/acme"
issue = {issue}
kind = "implementation"
prerequisites = []
[spec]
path = "docs/specs/0010-spec-dispatch-coordinator.md"
sha256 = "{}"
[issue_body]
content = "{BODY}"
sha256 = "{}"
"#,
        content_sha256(SPEC),
        content_sha256(BODY),
    )
}

fn context(run_id: u64) -> ReconcileContext {
    ReconcileContext {
        repository: "proof/acme".to_owned(),
        trusted_workflow: ".github/workflows/coordinator.yml".to_owned(),
        trusted_writer: "github-actions[bot]".to_owned(),
        run: WorkflowRun {
            run_id,
            run_attempt: 1,
            workflow_path: ".github/workflows/coordinator.yml".to_owned(),
            workflow_revision: REVISION.to_owned(),
        },
    }
}

fn snapshot(issue: u64) -> GitHubSnapshot {
    let declaration = request(issue).declaration().clone();
    GitHubSnapshot {
        repository: "proof/acme".to_owned(),
        main_revision: REVISION.to_owned(),
        owner_login: "owner".to_owned(),
        authorized_approvers: vec!["owner".to_owned()],
        config: CoordinatorConfig {
            automatic_dispatch: true,
            max_active_tasks: 2,
            max_assignments_per_reconciliation: 2,
            max_attempts: 2,
        },
        kill_switches: KillSwitches {
            global: Evidence::Known(false),
            coordinator: Evidence::Known(false),
            assignment: Evidence::Known(false),
        },
        contracts: vec![contract(issue)],
        observations: vec![TaskObservation {
            issue,
            current_spec: Evidence::Known(SPEC.to_owned()),
            current_issue_body: Evidence::Known(BODY.to_owned()),
            issue_state: Evidence::Known(IssueState::Open),
            approval: Evidence::Known(Some(ApprovalEvidence {
                repository: declaration.repository,
                issue,
                spec_path: declaration.spec.path,
                spec_sha256: declaration.spec.sha256,
                issue_body_sha256: declaration.issue_body.sha256,
                kind: declaration.kind,
                prerequisites: declaration.prerequisites,
                spec_pull_request: 44,
                merged_by: "owner".to_owned(),
                main_revision: REVISION.to_owned(),
                present_on_protected_main: true,
            })),
            native_prerequisites: Evidence::Known(vec![]),
            existing_work: Evidence::Known(ExistingWork::default()),
            completion: Evidence::Known(None),
        }],
        work: Evidence::Known(vec![]),
        resolutions: Evidence::Known(vec![]),
        owner_ready: Evidence::Known(Some(OwnerReady {
            repository: "proof/acme".to_owned(),
            issue,
            applied_by: "owner".to_owned(),
            label_present: true,
        })),
    }
}

#[derive(Default)]
struct Journal {
    receipts: Vec<StoredReceipt>,
    writes: usize,
    fail_write: Option<usize>,
    persist_failed_write: bool,
    fail_lock: bool,
    fail_load: bool,
}

#[derive(Clone, Default)]
struct FakeStore(Arc<Mutex<Journal>>);

struct FakeSession<'a>(MutexGuard<'a, Journal>);

impl ClaimStore for FakeStore {
    type Session<'a> = FakeSession<'a>;

    fn lock(&self, repository: &str) -> Result<Self::Session<'_>, String> {
        assert_eq!(repository, "proof/acme");
        let journal = self.0.lock().unwrap();
        if journal.fail_lock {
            return Err("lock unavailable".to_owned());
        }
        Ok(FakeSession(journal))
    }
}

fn stored(receipt: Receipt) -> StoredReceipt {
    StoredReceipt {
        provenance: Evidence::Known(ReceiptProvenance {
            repository: "proof/acme".to_owned(),
            writer: "github-actions[bot]".to_owned(),
            protected_main: true,
            run: receipt.recorded_by.clone(),
        }),
        receipt,
    }
}

impl ClaimSession for FakeSession<'_> {
    fn load(&mut self) -> Result<Vec<StoredReceipt>, String> {
        if self.0.fail_load {
            return Err("receipt read unavailable".to_owned());
        }
        Ok(self.0.receipts.clone())
    }

    fn append(&mut self, receipt: &Receipt) -> Result<(), String> {
        self.0.writes += 1;
        if self.0.fail_write == Some(self.0.writes) {
            if self.0.persist_failed_write {
                self.0.receipts.push(stored(receipt.clone()));
            }
            return Err("write outcome unknown".to_owned());
        }
        self.0.receipts.push(stored(receipt.clone()));
        Ok(())
    }
}

struct FakeGitHub {
    snapshots: Mutex<VecDeque<Result<GitHubSnapshot, String>>>,
    fallback: GitHubSnapshot,
}

impl FakeGitHub {
    fn new(snapshot: GitHubSnapshot) -> Self {
        Self {
            snapshots: Mutex::default(),
            fallback: snapshot,
        }
    }
}

impl GitHubState for FakeGitHub {
    fn observe(&self, _request: &AssignmentRequest) -> Result<GitHubSnapshot, String> {
        self.snapshots
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(self.fallback.clone()))
    }
}

struct FakeAssignment {
    capabilities: AssignmentCapabilities,
    calls: Mutex<Vec<AssignmentRequest>>,
    outcome: AssignmentOutcome,
}

impl Default for FakeAssignment {
    fn default() -> Self {
        Self {
            capabilities: AssignmentCapabilities {
                exact_target: Capability::Supported,
                immutable_task: Capability::Supported,
            },
            calls: Mutex::default(),
            outcome: AssignmentOutcome::Accepted,
        }
    }
}

impl AssignmentTransport for FakeAssignment {
    fn capabilities(&self) -> AssignmentCapabilities {
        self.capabilities
    }

    fn assign(&self, request: &AssignmentRequest) -> AssignmentOutcome {
        self.calls.lock().unwrap().push(request.clone());
        self.outcome.clone()
    }
}

#[test]
fn persists_bound_intent_before_assignment_and_rejects_duplicate_routes() {
    let store = FakeStore::default();
    let github = FakeGitHub::new(snapshot(17));
    let assignment = FakeAssignment::default();
    let request = request(17);

    assert_eq!(
        reconcile(
            &store,
            &github,
            &assignment,
            &context(100),
            &request,
            Route::Automatic
        ),
        ReconcileResult::Assigned
    );
    let receipts = &store.0.lock().unwrap().receipts.clone();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].receipt.state, ReceiptState::Pending);
    assert_eq!(receipts[1].receipt.state, ReceiptState::Accepted);
    assert_eq!(receipts[0].receipt.attempt.binding.repository, "proof/acme");
    assert_eq!(receipts[0].receipt.attempt.binding.issue, 17);
    assert_eq!(
        receipts[0].receipt.attempt.binding.approved_main_revision,
        REVISION
    );
    assert_eq!(
        receipts[0].receipt.attempt.binding.contract_sha256,
        request.contract_sha256()
    );
    assert_eq!(receipts[0].receipt.attempt.run.run_id, 100);
    assert_eq!(receipts[0].receipt.attempt.number, 1);
    for route in [Route::Manual, Route::Automatic] {
        assert_eq!(
            reconcile(&store, &github, &assignment, &context(101), &request, route),
            ReconcileResult::Blocked(ClaimBlock::RecoveryRequired)
        );
    }
    assert_eq!(assignment.calls.lock().unwrap().as_slice(), &[request]);
}

#[test]
fn pinned_and_unknown_handoffs_do_not_claim_or_assign() {
    for capabilities in [
        AssignmentCapabilities::gh_aw_0_93_5(),
        AssignmentCapabilities {
            exact_target: Capability::Unknown,
            immutable_task: Capability::Supported,
        },
    ] {
        let store = FakeStore::default();
        let assignment = FakeAssignment {
            capabilities,
            ..FakeAssignment::default()
        };
        assert!(matches!(
            reconcile(
                &store,
                &FakeGitHub::new(snapshot(17)),
                &assignment,
                &context(100),
                &request(17),
                Route::Automatic
            ),
            ReconcileResult::Blocked(ClaimBlock::Capability(_))
        ));
        assert!(store.0.lock().unwrap().receipts.is_empty());
        assert!(assignment.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn actual_agent_work_and_open_review_prs_block_duplicates_and_capacity() {
    for (issue, active_copilot, open_implementation_prs, expected) in [
        (17, 1, 0, ReconcileResult::ExistingWork),
        (17, 0, 1, ReconcileResult::ExistingWork),
        (
            18,
            2,
            0,
            ReconcileResult::Blocked(ClaimBlock::CapacityExhausted),
        ),
        (
            18,
            0,
            2,
            ReconcileResult::Blocked(ClaimBlock::CapacityExhausted),
        ),
    ] {
        let mut state = snapshot(17);
        state.work = Evidence::Known(vec![ActualWork {
            issue,
            active_copilot,
            open_implementation_prs,
        }]);
        let assignment = FakeAssignment::default();
        assert_eq!(
            reconcile(
                &FakeStore::default(),
                &FakeGitHub::new(state),
                &assignment,
                &context(100),
                &request(17),
                Route::Automatic
            ),
            expected
        );
        assert!(assignment.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn final_revalidation_blocks_scope_authority_and_switch_changes() {
    let mut changed_body = snapshot(17);
    changed_body.observations[0].current_issue_body = Evidence::Known("New scope".to_owned());
    let mut changed_revision = snapshot(17);
    changed_revision.main_revision = "b".repeat(40);
    let mut changed_contract = snapshot(17);
    changed_contract.contracts[0].push('\n');
    let mut changed_target = snapshot(17);
    changed_target.repository = "other/repo".to_owned();
    let mut closed = snapshot(17);
    closed.observations[0].issue_state = Evidence::Known(IssueState::Closed);
    let mut stopped = snapshot(17);
    stopped.kill_switches.assignment = Evidence::Known(true);
    for changed in [
        changed_body,
        changed_revision,
        changed_contract,
        changed_target,
        closed,
        stopped,
    ] {
        let store = FakeStore::default();
        let github = FakeGitHub::new(changed);
        github.snapshots.lock().unwrap().push_back(Ok(snapshot(17)));
        let assignment = FakeAssignment::default();
        assert!(matches!(
            reconcile(
                &store,
                &github,
                &assignment,
                &context(100),
                &request(17),
                Route::Automatic
            ),
            ReconcileResult::Blocked(_)
        ));
        assert!(assignment.calls.lock().unwrap().is_empty());
        assert_eq!(
            store
                .0
                .lock()
                .unwrap()
                .receipts
                .last()
                .unwrap()
                .receipt
                .state,
            ReceiptState::NotAssigned
        );
    }
}

#[test]
fn manual_route_requires_current_owner_gate_but_not_automatic_enablement() {
    let mut state = snapshot(17);
    state.config.automatic_dispatch = false;
    let assignment = FakeAssignment::default();
    assert_eq!(
        reconcile(
            &FakeStore::default(),
            &FakeGitHub::new(state.clone()),
            &assignment,
            &context(100),
            &request(17),
            Route::Automatic
        ),
        ReconcileResult::Blocked(ClaimBlock::Ineligible(
            acme::ReasonCode::AutomaticDispatchDisabled
        ))
    );
    assert_eq!(
        reconcile(
            &FakeStore::default(),
            &FakeGitHub::new(state.clone()),
            &assignment,
            &context(100),
            &request(17),
            Route::Manual
        ),
        ReconcileResult::Assigned
    );
    for ready in [
        Evidence::Unknown,
        Evidence::Error,
        Evidence::Known(None),
        Evidence::Known(Some(OwnerReady {
            repository: "proof/acme".to_owned(),
            issue: 17,
            applied_by: "bot".to_owned(),
            label_present: true,
        })),
        Evidence::Known(Some(OwnerReady {
            repository: "proof/acme".to_owned(),
            issue: 18,
            applied_by: "owner".to_owned(),
            label_present: true,
        })),
        Evidence::Known(Some(OwnerReady {
            repository: "proof/acme".to_owned(),
            issue: 17,
            applied_by: "owner".to_owned(),
            label_present: false,
        })),
    ] {
        state.owner_ready = ready;
        assert_eq!(
            reconcile(
                &FakeStore::default(),
                &FakeGitHub::new(state.clone()),
                &assignment,
                &context(101),
                &request(17),
                Route::Manual
            ),
            ReconcileResult::Blocked(ClaimBlock::OwnerGateRequired)
        );
    }
    assert_eq!(assignment.calls.lock().unwrap().len(), 1);
}

#[test]
fn partial_writes_fail_closed_even_when_the_server_commits_then_times_out() {
    for fail_write in [1, 2] {
        for persist_failed_write in [false, true] {
            let store = FakeStore::default();
            {
                let mut journal = store.0.lock().unwrap();
                journal.fail_write = Some(fail_write);
                journal.persist_failed_write = persist_failed_write;
            }
            let github = FakeGitHub::new(snapshot(17));
            let assignment = FakeAssignment::default();
            assert!(matches!(
                reconcile(
                    &store,
                    &github,
                    &assignment,
                    &context(100),
                    &request(17),
                    Route::Automatic
                ),
                ReconcileResult::Blocked(ClaimBlock::Store(_))
            ));
            let calls = assignment.calls.lock().unwrap().len();
            assert_eq!(calls, usize::from(fail_write == 2));
            store.0.lock().unwrap().fail_write = None;
            if fail_write == 2 || persist_failed_write {
                assert_eq!(
                    reconcile(
                        &store,
                        &github,
                        &assignment,
                        &context(101),
                        &request(17),
                        Route::Manual
                    ),
                    ReconcileResult::Blocked(ClaimBlock::RecoveryRequired)
                );
                assert_eq!(assignment.calls.lock().unwrap().len(), calls);
            }
        }
    }
}

#[test]
fn unknown_outcome_requires_bound_terminal_evidence_and_a_new_run_for_bounded_retry() {
    let store = FakeStore::default();
    let assignment = FakeAssignment {
        outcome: AssignmentOutcome::Unknown,
        ..FakeAssignment::default()
    };
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(snapshot(17)),
            &assignment,
            &context(100),
            &request(17),
            Route::Automatic
        ),
        ReconcileResult::Uncertain
    );
    let attempt = store.0.lock().unwrap().receipts[0].receipt.attempt.clone();
    let mut state = snapshot(17);
    state.resolutions = Evidence::Known(vec![AttemptResolution {
        attempt: attempt.clone(),
        outcome: RecoveryOutcome::NeverAssigned,
    }]);
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(state.clone()),
            &assignment,
            &context(100),
            &request(17),
            Route::Manual
        ),
        ReconcileResult::Blocked(ClaimBlock::RunAlreadyAttempted)
    );
    let mut wrong_attempt = attempt;
    wrong_attempt.binding.issue = 18;
    state.resolutions = Evidence::Known(vec![AttemptResolution {
        attempt: wrong_attempt,
        outcome: RecoveryOutcome::NeverAssigned,
    }]);
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(state.clone()),
            &assignment,
            &context(101),
            &request(17),
            Route::Manual
        ),
        ReconcileResult::Blocked(ClaimBlock::RecoveryRequired)
    );
    let attempt = store.0.lock().unwrap().receipts[0].receipt.attempt.clone();
    state.resolutions = Evidence::Known(vec![AttemptResolution {
        attempt,
        outcome: RecoveryOutcome::NeverAssigned,
    }]);
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(state),
            &assignment,
            &context(101),
            &request(17),
            Route::Manual
        ),
        ReconcileResult::Uncertain
    );
    let attempt = store
        .0
        .lock()
        .unwrap()
        .receipts
        .last()
        .unwrap()
        .receipt
        .attempt
        .clone();
    let mut state = snapshot(17);
    state.resolutions = Evidence::Known(vec![AttemptResolution {
        attempt,
        outcome: RecoveryOutcome::Cancelled,
    }]);
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(state),
            &assignment,
            &context(102),
            &request(17),
            Route::Automatic
        ),
        ReconcileResult::Blocked(ClaimBlock::AttemptsExhausted)
    );
    assert_eq!(assignment.calls.lock().unwrap().len(), 2);
}

#[test]
fn actual_work_reconciles_an_uncertain_attempt_without_retry() {
    let store = FakeStore::default();
    let assignment = FakeAssignment {
        outcome: AssignmentOutcome::Unknown,
        ..FakeAssignment::default()
    };
    let github = FakeGitHub::new(snapshot(17));
    reconcile(
        &store,
        &github,
        &assignment,
        &context(100),
        &request(17),
        Route::Automatic,
    );
    let mut state = snapshot(17);
    state.work = Evidence::Known(vec![ActualWork {
        issue: 17,
        active_copilot: 0,
        open_implementation_prs: 1,
    }]);
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(state),
            &assignment,
            &context(101),
            &request(17),
            Route::Manual
        ),
        ReconcileResult::ExistingWork
    );
    assert_eq!(
        store
            .0
            .lock()
            .unwrap()
            .receipts
            .last()
            .unwrap()
            .receipt
            .state,
        ReceiptState::Accepted
    );
    let attempt = store.0.lock().unwrap().receipts[0].receipt.attempt.clone();
    let mut state = snapshot(17);
    state.resolutions = Evidence::Known(vec![AttemptResolution {
        attempt,
        outcome: RecoveryOutcome::Merged,
    }]);
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(state),
            &assignment,
            &context(102),
            &request(17),
            Route::Automatic
        ),
        ReconcileResult::Complete
    );
    assert_eq!(
        reconcile(
            &store,
            &github,
            &assignment,
            &context(103),
            &request(17),
            Route::Manual
        ),
        ReconcileResult::Complete
    );
    assert_eq!(assignment.calls.lock().unwrap().len(), 1);
}

#[test]
fn receipts_require_external_provenance_and_valid_history() {
    let original = FakeStore::default();
    reconcile(
        &original,
        &FakeGitHub::new(snapshot(17)),
        &FakeAssignment::default(),
        &context(100),
        &request(17),
        Route::Automatic,
    );
    let receipts = original.0.lock().unwrap().receipts.clone();
    for mutation in 0..8 {
        let store = FakeStore::default();
        let mut bad = receipts.clone();
        match mutation {
            0 => bad[0].provenance = Evidence::Unknown,
            1 => bad[0].provenance = Evidence::Error,
            2 => {
                if let Evidence::Known(proof) = &mut bad[0].provenance {
                    proof.writer = "attacker".to_owned();
                }
            }
            3 => {
                if let Evidence::Known(proof) = &mut bad[0].provenance {
                    proof.protected_main = false;
                }
            }
            4 => {
                if let Evidence::Known(proof) = &mut bad[0].provenance {
                    proof.run.run_id = 999;
                }
            }
            5 => bad[0].receipt.attempt.binding.repository = "other/repo".to_owned(),
            6 => bad[0].receipt.attempt.number = 2,
            7 => {
                bad.remove(0);
            }
            _ => unreachable!(),
        };
        store.0.lock().unwrap().receipts = bad;
        let assignment = FakeAssignment::default();
        assert!(matches!(
            reconcile(
                &store,
                &FakeGitHub::new(snapshot(17)),
                &assignment,
                &context(101),
                &request(17),
                Route::Manual
            ),
            ReconcileResult::Blocked(ClaimBlock::UntrustedReceipt | ClaimBlock::InvalidHistory)
        ));
        assert!(assignment.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn concurrent_manual_and_automatic_routes_share_one_claim() {
    let store = FakeStore::default();
    let github = FakeGitHub::new(snapshot(17));
    let assignment = FakeAssignment::default();
    let start = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for index in 0..8 {
            let store = &store;
            let github = &github;
            let assignment = &assignment;
            let start = &start;
            handles.push(scope.spawn(move || {
                start.wait();
                reconcile(
                    store,
                    github,
                    assignment,
                    &context(100 + index),
                    &request(17),
                    if index % 2 == 0 {
                        Route::Manual
                    } else {
                        Route::Automatic
                    },
                )
            }));
        }

        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == ReconcileResult::Assigned)
                .count(),
            1
        );
    });
    assert_eq!(assignment.calls.lock().unwrap().len(), 1);
    assert_eq!(store.0.lock().unwrap().receipts.len(), 2);
}

fn independent_tasks() -> GitHubSnapshot {
    let mut state = snapshot(17);
    state.contracts.push(contract(18));
    state.observations.push(snapshot(18).observations.remove(0));
    state
}

#[test]
fn independent_claims_use_remaining_capacity_without_speculative_reservations() {
    let store = FakeStore::default();
    let github = FakeGitHub::new(independent_tasks());
    let assignment = FakeAssignment::default();
    for issue in [17, 18] {
        assert_eq!(
            reconcile(
                &store,
                &github,
                &assignment,
                &context(100),
                &request(issue),
                Route::Automatic
            ),
            ReconcileResult::Assigned
        );
    }
    assert_eq!(assignment.calls.lock().unwrap().len(), 2);
}

#[test]
fn manual_selection_does_not_reserve_slots_for_other_eligible_tasks() {
    let mut state = independent_tasks();
    state.owner_ready = snapshot(18).owner_ready;
    state.config.max_active_tasks = 1;
    state.config.max_assignments_per_reconciliation = 1;
    assert_eq!(
        reconcile(
            &FakeStore::default(),
            &FakeGitHub::new(state),
            &FakeAssignment::default(),
            &context(100),
            &request(18),
            Route::Manual
        ),
        ReconcileResult::Assigned
    );
}

#[test]
fn persisted_intents_enforce_run_limits_across_routes_and_reruns() {
    let store = FakeStore::default();
    let mut state = independent_tasks();
    state.config.max_assignments_per_reconciliation = 1;
    let assignment = FakeAssignment {
        outcome: AssignmentOutcome::NotAssigned,
        ..FakeAssignment::default()
    };
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(state.clone()),
            &assignment,
            &context(100),
            &request(17),
            Route::Manual
        ),
        ReconcileResult::NotAssigned
    );
    let mut rerun = context(100);
    rerun.run.run_attempt = 2;
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(state),
            &assignment,
            &rerun,
            &request(18),
            Route::Automatic
        ),
        ReconcileResult::Blocked(ClaimBlock::ReconciliationLimitReached)
    );
    assert_eq!(assignment.calls.lock().unwrap().len(), 1);
}

#[test]
fn unobserved_claims_reserve_capacity_until_actual_state_is_reconciled() {
    let store = FakeStore::default();
    let mut state = independent_tasks();
    state.config.max_active_tasks = 1;
    let assignment = FakeAssignment {
        outcome: AssignmentOutcome::Unknown,
        ..FakeAssignment::default()
    };
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(state.clone()),
            &assignment,
            &context(100),
            &request(17),
            Route::Automatic
        ),
        ReconcileResult::Uncertain
    );
    state.owner_ready = snapshot(18).owner_ready;
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(state),
            &assignment,
            &context(101),
            &request(18),
            Route::Manual
        ),
        ReconcileResult::Blocked(ClaimBlock::CapacityExhausted)
    );
    assert_eq!(assignment.calls.lock().unwrap().len(), 1);
}

#[test]
fn unavailable_boundaries_and_unknown_actual_state_never_assign() {
    for failure in 0..5 {
        let store = FakeStore::default();
        let mut state = snapshot(17);
        match failure {
            0 => store.0.lock().unwrap().fail_lock = true,
            1 => store.0.lock().unwrap().fail_load = true,
            2 => state.work = Evidence::Unknown,
            3 => state.work = Evidence::Error,
            4 => {}
            _ => unreachable!(),
        }
        let github = FakeGitHub::new(state);
        if failure == 4 {
            github
                .snapshots
                .lock()
                .unwrap()
                .push_back(Err("API timeout".to_owned()));
        }
        let assignment = FakeAssignment::default();
        assert!(matches!(
            reconcile(
                &store,
                &github,
                &assignment,
                &context(100),
                &request(17),
                Route::Automatic
            ),
            ReconcileResult::Blocked(_)
        ));
        assert!(assignment.calls.lock().unwrap().is_empty());
        assert!(store.0.lock().unwrap().receipts.is_empty());
    }
}

#[test]
fn receipt_json_is_strict_but_never_supplies_its_own_provenance() {
    let store = FakeStore::default();
    reconcile(
        &store,
        &FakeGitHub::new(snapshot(17)),
        &FakeAssignment::default(),
        &context(100),
        &request(17),
        Route::Automatic,
    );
    let receipt = store.0.lock().unwrap().receipts[0].receipt.clone();
    let encoded = serde_json::to_string(&receipt).unwrap();
    let decoded: Receipt = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, receipt);
    let mut json = serde_json::to_value(&receipt).unwrap();
    json["provenance"] = serde_json::json!({"trusted": true});
    assert!(serde_json::from_value::<Receipt>(json).is_err());
    store.0.lock().unwrap().receipts = vec![StoredReceipt {
        receipt: decoded,
        provenance: Evidence::Unknown,
    }];
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(snapshot(17)),
            &FakeAssignment::default(),
            &context(101),
            &request(17),
            Route::Manual
        ),
        ReconcileResult::Blocked(ClaimBlock::UntrustedReceipt)
    );
}

#[test]
fn terminal_recovery_needs_no_actual_work_and_is_rechecked_before_assignment() {
    for outcome in [
        RecoveryOutcome::NeverAssigned,
        RecoveryOutcome::Failed,
        RecoveryOutcome::Cancelled,
    ] {
        let store = FakeStore::default();
        let assignment = FakeAssignment {
            outcome: AssignmentOutcome::Unknown,
            ..FakeAssignment::default()
        };
        reconcile(
            &store,
            &FakeGitHub::new(snapshot(17)),
            &assignment,
            &context(100),
            &request(17),
            Route::Automatic,
        );
        let attempt = store.0.lock().unwrap().receipts[0].receipt.attempt.clone();
        let mut state = snapshot(17);
        state.resolutions = Evidence::Known(vec![AttemptResolution { attempt, outcome }]);
        let mut with_pr = state.clone();
        with_pr.work = Evidence::Known(vec![ActualWork {
            issue: 17,
            active_copilot: 0,
            open_implementation_prs: 1,
        }]);
        assert_eq!(
            reconcile(
                &store,
                &FakeGitHub::new(with_pr),
                &assignment,
                &context(101),
                &request(17),
                Route::Automatic
            ),
            ReconcileResult::ExistingWork
        );
        let github = FakeGitHub::new(snapshot(17));
        github.snapshots.lock().unwrap().push_back(Ok(state));
        assert_eq!(
            reconcile(
                &store,
                &github,
                &assignment,
                &context(101),
                &request(17),
                Route::Manual
            ),
            ReconcileResult::Blocked(ClaimBlock::RecoveryRequired)
        );
        assert_eq!(assignment.calls.lock().unwrap().len(), 1);
        assert_eq!(
            store
                .0
                .lock()
                .unwrap()
                .receipts
                .last()
                .unwrap()
                .receipt
                .state,
            ReceiptState::NotAssigned
        );
    }
}

#[test]
fn terminal_failure_or_cancellation_can_recover_within_the_attempt_limit() {
    for outcome in [RecoveryOutcome::Failed, RecoveryOutcome::Cancelled] {
        let store = FakeStore::default();
        let assignment = FakeAssignment::default();
        reconcile(
            &store,
            &FakeGitHub::new(snapshot(17)),
            &assignment,
            &context(100),
            &request(17),
            Route::Automatic,
        );
        let attempt = store.0.lock().unwrap().receipts[0].receipt.attempt.clone();
        let mut state = snapshot(17);
        state.resolutions = Evidence::Known(vec![AttemptResolution { attempt, outcome }]);
        assert_eq!(
            reconcile(
                &store,
                &FakeGitHub::new(state),
                &assignment,
                &context(101),
                &request(17),
                Route::Manual
            ),
            ReconcileResult::Assigned
        );
        assert_eq!(assignment.calls.lock().unwrap().len(), 2);
        assert_eq!(
            store
                .0
                .lock()
                .unwrap()
                .receipts
                .last()
                .unwrap()
                .receipt
                .attempt
                .number,
            2
        );
    }
}

#[test]
fn final_revalidation_blocks_new_work_capacity_and_owner_gate_changes() {
    for change in 0..7 {
        let mut current = snapshot(17);
        match change {
            0 => current.owner_ready = Evidence::Known(None),
            1 => current.authorized_approvers = vec![],
            2 => current.observations[0].current_spec = Evidence::Unknown,
            3 => {
                current.work = Evidence::Known(vec![ActualWork {
                    issue: 18,
                    active_copilot: 2,
                    open_implementation_prs: 0,
                }]);
            }
            4 => {
                current.work = Evidence::Known(vec![ActualWork {
                    issue: 17,
                    active_copilot: 1,
                    open_implementation_prs: 0,
                }]);
            }
            5 => current.config.max_attempts = 0,
            6 => current.observations[0].native_prerequisites = Evidence::Known(vec![99]),
            _ => unreachable!(),
        }
        let github = FakeGitHub::new(current);
        github.snapshots.lock().unwrap().push_back(Ok(snapshot(17)));
        let assignment = FakeAssignment::default();
        let store = FakeStore::default();
        assert!(matches!(
            reconcile(
                &store,
                &github,
                &assignment,
                &context(100),
                &request(17),
                Route::Manual
            ),
            ReconcileResult::Blocked(_)
        ));
        assert!(assignment.calls.lock().unwrap().is_empty());
        assert_eq!(
            store
                .0
                .lock()
                .unwrap()
                .receipts
                .last()
                .unwrap()
                .receipt
                .state,
            ReceiptState::NotAssigned
        );
    }
}

#[test]
fn a_claim_cannot_be_rebound_to_a_new_contract_target_or_revision() {
    let store = FakeStore::default();
    let assignment = FakeAssignment::default();
    reconcile(
        &store,
        &FakeGitHub::new(snapshot(17)),
        &assignment,
        &context(100),
        &request(17),
        Route::Automatic,
    );
    for substitution in [
        AssignmentRequest::from_approved_snapshot(&(contract(17) + "\n"), REVISION).unwrap(),
        AssignmentRequest::from_approved_snapshot(&contract(17), &"b".repeat(40)).unwrap(),
    ] {
        let mut run = context(101);
        run.run.workflow_revision = substitution.approved_main_revision().to_owned();
        assert_eq!(
            reconcile(
                &store,
                &FakeGitHub::new(snapshot(17)),
                &assignment,
                &run,
                &substitution,
                Route::Manual
            ),
            ReconcileResult::Blocked(ClaimBlock::SnapshotMismatch)
        );
    }
    assert_eq!(
        reconcile(
            &store,
            &FakeGitHub::new(snapshot(17)),
            &assignment,
            &context(101),
            &request(999),
            Route::Manual
        ),
        ReconcileResult::Blocked(ClaimBlock::SnapshotMismatch)
    );
    assert_eq!(assignment.calls.lock().unwrap().len(), 1);
}

#[test]
fn missing_or_conflicting_recovery_evidence_never_expires_a_claim() {
    let store = FakeStore::default();
    let assignment = FakeAssignment::default();
    reconcile(
        &store,
        &FakeGitHub::new(snapshot(17)),
        &assignment,
        &context(100),
        &request(17),
        Route::Automatic,
    );
    let attempt = store.0.lock().unwrap().receipts[0].receipt.attempt.clone();
    for resolutions in [
        Evidence::Unknown,
        Evidence::Error,
        Evidence::Known(vec![]),
        Evidence::Known(vec![
            AttemptResolution {
                attempt: attempt.clone(),
                outcome: RecoveryOutcome::Failed,
            },
            AttemptResolution {
                attempt,
                outcome: RecoveryOutcome::Merged,
            },
        ]),
    ] {
        let mut state = snapshot(17);
        state.resolutions = resolutions;
        assert_eq!(
            reconcile(
                &store,
                &FakeGitHub::new(state),
                &assignment,
                &context(999_999),
                &request(17),
                Route::Manual
            ),
            ReconcileResult::Blocked(ClaimBlock::RecoveryRequired)
        );
    }
    assert_eq!(assignment.calls.lock().unwrap().len(), 1);
}
