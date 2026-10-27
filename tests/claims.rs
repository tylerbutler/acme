use acme::assignment::{
    AssignmentCapabilities, AssignmentOutcome, AssignmentPermit, AssignmentRequest,
    AssignmentTransport, Capability,
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
}

#[derive(Clone, Default)]
struct FakeStore(Arc<Mutex<Journal>>);

struct FakeSession<'a>(MutexGuard<'a, Journal>);

impl ClaimStore for FakeStore {
    type Session<'a> = FakeSession<'a>;

    fn lock(&self, repository: &str) -> Result<Self::Session<'_>, String> {
        assert_eq!(repository, "proof/acme");
        Ok(FakeSession(self.0.lock().unwrap()))
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
    fn assignment_returned(&mut self, _: acme::assignment::AssignmentReturn) -> Result<(), String> {
        Ok(())
    }
    fn arm_assignment(&mut self, _: &acme::claims::AttemptId) -> Result<(), String> {
        Ok(())
    }

    fn reserve_assignment(
        &mut self,
        _: &acme::claims::AttemptId,
        _: &AssignmentRequest,
    ) -> Result<String, String> {
        Ok("fake-held-generation".to_owned())
    }

    fn release(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn load(&mut self) -> Result<Vec<StoredReceipt>, String> {
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
                approved_snapshot: Capability::Supported,
                immutable_task: Capability::Unsupported,
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

    fn assign(&self, permit: AssignmentPermit<'_>) -> AssignmentOutcome {
        self.calls.lock().unwrap().push(permit.request().clone());
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
    assert_eq!(
        receipts[0].receipt.attempt.binding.approved_contract,
        contract(17)
    );
    let encoded = serde_json::to_string(&receipts[0].receipt).unwrap();
    assert_eq!(
        serde_json::from_str::<Receipt>(&encoded).unwrap(),
        receipts[0].receipt
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
fn missing_unknown_and_unsupported_handoffs_do_not_claim_or_assign() {
    for capabilities in [
        AssignmentCapabilities::default(),
        AssignmentCapabilities {
            exact_target: Capability::Unknown,
            ..AssignmentCapabilities::gh_aw_0_93_5()
        },
        AssignmentCapabilities {
            exact_target: Capability::Unsupported,
            ..AssignmentCapabilities::gh_aw_0_93_5()
        },
        AssignmentCapabilities {
            approved_snapshot: Capability::Unknown,
            ..AssignmentCapabilities::gh_aw_0_93_5()
        },
        AssignmentCapabilities {
            approved_snapshot: Capability::Unsupported,
            ..AssignmentCapabilities::gh_aw_0_93_5()
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
    let mut changed_spec = snapshot(17);
    changed_spec.observations[0].current_spec = Evidence::Known("Changed spec".to_owned());
    let mut unreadable_spec = snapshot(17);
    unreadable_spec.observations[0].current_spec = Evidence::Error;
    let mut unknown_body = snapshot(17);
    unknown_body.observations[0].current_issue_body = Evidence::Unknown;
    let mut changed_approval = snapshot(17);
    if let Evidence::Known(Some(approval)) = &mut changed_approval.observations[0].approval {
        approval.merged_by = "agent".to_owned();
    }
    let mut unprotected = snapshot(17);
    if let Evidence::Known(Some(approval)) = &mut unprotected.observations[0].approval {
        approval.present_on_protected_main = false;
    }
    let mut disabled = snapshot(17);
    disabled.config.automatic_dispatch = false;
    let mut full = snapshot(17);
    full.work = Evidence::Known(vec![ActualWork {
        issue: 18,
        active_copilot: 0,
        open_implementation_prs: 2,
    }]);
    for changed in [
        changed_body,
        changed_revision,
        changed_contract,
        changed_target,
        closed,
        stopped,
        changed_spec,
        unreadable_spec,
        unknown_body,
        changed_approval,
        unprotected,
        disabled,
        full,
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
fn failed_final_read_records_no_assignment_and_keeps_the_attempt() {
    let store = FakeStore::default();
    let github = FakeGitHub::new(snapshot(17));
    github
        .snapshots
        .lock()
        .unwrap()
        .extend([Ok(snapshot(17)), Err("final read failed".to_owned())]);
    let assignment = FakeAssignment::default();
    assert_eq!(
        reconcile(
            &store,
            &github,
            &assignment,
            &context(100),
            &request(17),
            Route::Automatic
        ),
        ReconcileResult::Blocked(ClaimBlock::GitHub("final read failed".to_owned()))
    );
    let journal = store.0.lock().unwrap();
    assert_eq!(journal.receipts.len(), 2);
    assert_eq!(journal.receipts[1].receipt.state, ReceiptState::NotAssigned);
    drop(journal);
    assert!(assignment.calls.lock().unwrap().is_empty());
}

#[test]
fn final_revalidation_blocks_a_different_valid_approval_receipt() {
    let mut changed = snapshot(17);
    if let Evidence::Known(Some(approval)) = &mut changed.observations[0].approval {
        approval.spec_pull_request = 45;
    }
    let github = FakeGitHub::new(changed);
    github.snapshots.lock().unwrap().push_back(Ok(snapshot(17)));
    let store = FakeStore::default();
    let assignment = FakeAssignment::default();
    assert_eq!(
        reconcile(
            &store,
            &github,
            &assignment,
            &context(100),
            &request(17),
            Route::Automatic
        ),
        ReconcileResult::Blocked(ClaimBlock::SnapshotMismatch)
    );
    assert!(assignment.calls.lock().unwrap().is_empty());
}

#[test]
fn constructed_requests_do_not_authorize_substituted_bindings() {
    let original = contract(17);
    let changed_body = original
        .replace(BODY, "Unapproved scope")
        .replace(&content_sha256(BODY), &content_sha256("Unapproved scope"));
    for (contract, revision) in [
        (
            original.replace("proof/acme", "other/repo"),
            REVISION.to_owned(),
        ),
        (
            original.replace("issue = 17", "issue = 18"),
            REVISION.to_owned(),
        ),
        (changed_body, REVISION.to_owned()),
        (format!("{original}\n"), REVISION.to_owned()),
        (original.clone(), "b".repeat(40)),
    ] {
        let store = FakeStore::default();
        let assignment = FakeAssignment::default();
        let request = AssignmentRequest::from_approved_snapshot(&contract, &revision).unwrap();
        let mut run = context(100);
        run.run.workflow_revision = revision;
        assert!(matches!(
            reconcile(
                &store,
                &FakeGitHub::new(snapshot(17)),
                &assignment,
                &run,
                &request,
                Route::Automatic
            ),
            ReconcileResult::Blocked(_)
        ));
        assert!(store.0.lock().unwrap().receipts.is_empty());
        assert!(assignment.calls.lock().unwrap().is_empty());
    }
}

#[test]
fn later_hash_changes_block_reconciliation_even_with_existing_work() {
    for (spec, body, expected) in [
        ("Changed spec", BODY, acme::ReasonCode::SpecChanged),
        (SPEC, "Changed body", acme::ReasonCode::IssueBodyChanged),
    ] {
        let store = FakeStore::default();
        let assignment = FakeAssignment::default();
        assert_eq!(
            reconcile(
                &store,
                &FakeGitHub::new(snapshot(17)),
                &assignment,
                &context(100),
                &request(17),
                Route::Automatic
            ),
            ReconcileResult::Assigned
        );
        let mut current = snapshot(17);
        current.observations[0].current_spec = Evidence::Known(spec.to_owned());
        current.observations[0].current_issue_body = Evidence::Known(body.to_owned());
        current.work = Evidence::Known(vec![ActualWork {
            issue: 17,
            active_copilot: 1,
            open_implementation_prs: 0,
        }]);
        for route in [Route::Automatic, Route::Manual] {
            assert_eq!(
                reconcile(
                    &store,
                    &FakeGitHub::new(current.clone()),
                    &assignment,
                    &context(101),
                    &request(17),
                    route
                ),
                ReconcileResult::Blocked(ClaimBlock::Ineligible(expected))
            );
        }
        assert_eq!(assignment.calls.lock().unwrap().len(), 1);
    }
}

#[test]
fn verified_completion_releases_capacity_despite_later_scope_drift() {
    for (spec, body) in [("Changed spec", BODY), (SPEC, "Changed body")] {
        let store = FakeStore::default();
        let assignment = FakeAssignment::default();
        assert_eq!(
            reconcile(
                &store,
                &FakeGitHub::new(snapshot(17)),
                &assignment,
                &context(100),
                &request(17),
                Route::Automatic
            ),
            ReconcileResult::Assigned
        );
        let attempt = store.0.lock().unwrap().receipts[0].receipt.attempt.clone();
        let mut completed = snapshot(17);
        completed.observations[0].current_spec = Evidence::Known(spec.to_owned());
        completed.observations[0].current_issue_body = Evidence::Known(body.to_owned());
        completed.resolutions = Evidence::Known(vec![AttemptResolution {
            attempt,
            outcome: RecoveryOutcome::Merged,
        }]);
        assert_eq!(
            reconcile(
                &store,
                &FakeGitHub::new(completed.clone()),
                &assignment,
                &context(101),
                &request(17),
                Route::Automatic
            ),
            ReconcileResult::Complete
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
            ReceiptState::Completed
        );
        assert_eq!(assignment.calls.lock().unwrap().len(), 1);
        let mut next = snapshot(18);
        next.config.max_active_tasks = 1;
        assert_eq!(
            reconcile(
                &store,
                &FakeGitHub::new(next),
                &assignment,
                &context(102),
                &request(18),
                Route::Automatic
            ),
            ReconcileResult::Assigned
        );
        assert_eq!(
            reconcile(
                &store,
                &FakeGitHub::new(completed),
                &assignment,
                &context(103),
                &request(17),
                Route::Manual
            ),
            ReconcileResult::Complete
        );
        assert_eq!(assignment.calls.lock().unwrap().len(), 2);
    }
}

#[test]
fn body_edit_after_final_read_does_not_replace_the_outgoing_snapshot() {
    struct EditAfterFinalRead {
        state: Mutex<(GitHubSnapshot, usize)>,
    }

    impl GitHubState for EditAfterFinalRead {
        fn observe(&self, _request: &AssignmentRequest) -> Result<GitHubSnapshot, String> {
            let mut state = self.state.lock().unwrap();
            let observed = state.0.clone();
            state.1 += 1;
            if state.1 == 2 {
                state.0.observations[0].current_issue_body =
                    Evidence::Known("Edit after the final read".to_owned());
            }
            drop(state);
            Ok(observed)
        }
    }

    let github = EditAfterFinalRead {
        state: Mutex::new((snapshot(17), 0)),
    };
    let store = FakeStore::default();
    let assignment = FakeAssignment::default();
    let approved = request(17);
    assert_eq!(
        reconcile(
            &store,
            &github,
            &assignment,
            &context(100),
            &approved,
            Route::Automatic
        ),
        ReconcileResult::Assigned
    );
    let calls = assignment.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let config = serde_json::to_value(calls[0].gh_aw_config()).unwrap();
    let instructions = config["custom-instructions"].as_str().unwrap();
    let binding: serde_json::Value =
        serde_json::from_str(instructions.split_once('\n').unwrap().1).unwrap();
    assert_eq!(binding["approved_contract"], contract(17));
    assert_eq!(config["target"], 17);
    drop(calls);
    assert_eq!(
        reconcile(
            &store,
            &github,
            &assignment,
            &context(101),
            &approved,
            Route::Manual
        ),
        ReconcileResult::Blocked(ClaimBlock::Ineligible(acme::ReasonCode::IssueBodyChanged))
    );
    assert_eq!(assignment.calls.lock().unwrap().len(), 1);
}

#[test]
fn receipt_snapshot_substitution_invalidates_trusted_history() {
    let store = FakeStore::default();
    let github = FakeGitHub::new(snapshot(17));
    let assignment = FakeAssignment::default();
    assert_eq!(
        reconcile(
            &store,
            &github,
            &assignment,
            &context(100),
            &request(17),
            Route::Automatic
        ),
        ReconcileResult::Assigned
    );
    store.0.lock().unwrap().receipts[0]
        .receipt
        .attempt
        .binding
        .approved_contract
        .push('\n');
    assert_eq!(
        reconcile(
            &store,
            &github,
            &assignment,
            &context(101),
            &request(17),
            Route::Automatic
        ),
        ReconcileResult::Blocked(ClaimBlock::InvalidHistory)
    );
    assert_eq!(assignment.calls.lock().unwrap().len(), 1);
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
fn uncertain_assignment_is_not_retried_on_either_route_without_recovery() {
    let store = FakeStore::default();
    let assignment = FakeAssignment {
        outcome: AssignmentOutcome::Unknown,
        ..FakeAssignment::default()
    };
    let github = FakeGitHub::new(snapshot(17));
    assert_eq!(
        reconcile(
            &store,
            &github,
            &assignment,
            &context(100),
            &request(17),
            Route::Automatic
        ),
        ReconcileResult::Uncertain
    );
    for route in [Route::Automatic, Route::Manual] {
        assert_eq!(
            reconcile(
                &store,
                &github,
                &assignment,
                &context(101),
                &request(17),
                route
            ),
            ReconcileResult::Blocked(ClaimBlock::RecoveryRequired)
        );
    }
    assert_eq!(assignment.calls.lock().unwrap().len(), 1);
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
