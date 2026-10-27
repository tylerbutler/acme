use acme::assignment::{
    AssignmentCapabilities, AssignmentOutcome, AssignmentPermit, AssignmentRequest,
    AssignmentTransport, Capability, handoff as guarded_handoff,
};
use acme::claims::{AttemptId, ClaimSession, Receipt, RequestBinding, StoredReceipt, WorkflowRun};
use std::cell::RefCell;

const CONTRACT: &str = include_str!("fixtures/assignment-handoff/task.toml");
const REVISION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct FakeAssignment {
    capabilities: AssignmentCapabilities,
    calls: RefCell<Vec<AssignmentRequest>>,
    outcome: AssignmentOutcome,
}

impl AssignmentTransport for FakeAssignment {
    fn capabilities(&self) -> AssignmentCapabilities {
        self.capabilities
    }

    fn assign(&self, permit: AssignmentPermit<'_>) -> AssignmentOutcome {
        self.calls.borrow_mut().push(permit.request().clone());
        self.outcome.clone()
    }
}

struct Held;

impl ClaimSession for Held {
    fn assignment_returned(&mut self, _: acme::assignment::AssignmentReturn) -> Result<(), String> {
        Ok(())
    }
    fn load(&mut self) -> Result<Vec<StoredReceipt>, String> {
        Ok(Vec::new())
    }
    fn append(&mut self, _: &Receipt) -> Result<(), String> {
        Ok(())
    }
    fn arm_assignment(&mut self, _: &AttemptId) -> Result<(), String> {
        Ok(())
    }
    fn reserve_assignment(
        &mut self,
        _: &AttemptId,
        _: &AssignmentRequest,
    ) -> Result<String, String> {
        Ok("fake-held-generation".to_owned())
    }
    fn release(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn handoff(
    transport: &impl AssignmentTransport,
    request: &AssignmentRequest,
) -> Result<AssignmentOutcome, String> {
    let attempt = AttemptId {
        binding: RequestBinding::from(request),
        number: 1,
        run: WorkflowRun {
            run_id: 1,
            run_attempt: 1,
            workflow_path: ".github/workflows/dispatch-claims.yml".to_owned(),
            workflow_revision: REVISION.to_owned(),
        },
    };
    guarded_handoff(transport, request, &attempt, &mut Held)
}

#[test]
fn unknown_and_unsupported_capabilities_block_before_transport() {
    let request = AssignmentRequest::from_approved_snapshot(CONTRACT, REVISION).unwrap();
    for exact_target in [
        Capability::Supported,
        Capability::Unsupported,
        Capability::Unknown,
    ] {
        for approved_snapshot in [
            Capability::Supported,
            Capability::Unsupported,
            Capability::Unknown,
        ] {
            let transport = FakeAssignment {
                capabilities: AssignmentCapabilities {
                    exact_target,
                    approved_snapshot,
                    immutable_task: Capability::Unsupported,
                },
                calls: RefCell::default(),
                outcome: AssignmentOutcome::Accepted,
            };
            let result = handoff(&transport, &request);
            if exact_target == Capability::Supported && approved_snapshot == Capability::Supported {
                assert_eq!(result, Ok(AssignmentOutcome::Accepted));
                assert_eq!(*transport.calls.borrow(), vec![request.clone()]);
            } else {
                assert!(result.is_err());
                assert!(transport.calls.borrow().is_empty());
            }
        }
    }
}

#[test]
fn pinned_handoff_supports_request_binding_but_not_context_isolation() {
    let transport = FakeAssignment {
        capabilities: AssignmentCapabilities::gh_aw_0_93_5(),
        calls: RefCell::default(),
        outcome: AssignmentOutcome::Accepted,
    };
    let request = AssignmentRequest::from_approved_snapshot(CONTRACT, REVISION).unwrap();
    assert_eq!(
        transport.capabilities.immutable_task,
        Capability::Unsupported
    );
    assert_eq!(
        handoff(&transport, &request),
        Ok(AssignmentOutcome::Accepted)
    );
    assert_eq!(*transport.calls.borrow(), vec![request]);
}

#[test]
fn missing_capabilities_fail_closed() {
    let transport = FakeAssignment {
        capabilities: AssignmentCapabilities::default(),
        calls: RefCell::default(),
        outcome: AssignmentOutcome::Accepted,
    };
    let request = AssignmentRequest::from_approved_snapshot(CONTRACT, REVISION).unwrap();
    assert!(handoff(&transport, &request).is_err());
    assert!(transport.calls.borrow().is_empty());
}

#[test]
fn configured_handoff_matches_the_pinned_handler_proof() {
    let request = AssignmentRequest::from_approved_snapshot(CONTRACT, REVISION).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/assignment-handoff/config.json")).unwrap();
    assert_eq!(
        serde_json::to_value(request.gh_aw_config()).unwrap(),
        expected
    );
    assert_eq!(request.approved_contract(), CONTRACT);
    assert_eq!(
        AssignmentRequest::from_approved_snapshot(&CONTRACT.replace('\n', "\r\n"), REVISION)
            .unwrap()
            .gh_aw_config(),
        request.gh_aw_config()
    );
}

#[test]
fn request_uses_only_the_approved_contract_and_main_revision() {
    let request = AssignmentRequest::from_approved_snapshot(CONTRACT, REVISION).unwrap();
    assert_eq!(request.declaration().repository, "proof/acme");
    assert_eq!(request.declaration().issue, 17);
    assert_eq!(
        request.declaration().issue_body.content,
        "Implement only the approved offline fixture.\n"
    );
    assert_eq!(request.approved_main_revision(), REVISION);
    assert_eq!(
        request.contract_sha256(),
        "41ca4709763a409b96e9707c8a702fa4ef070d642c548e97dd00291d9eef9578"
    );
    assert_eq!(
        request,
        AssignmentRequest::from_approved_snapshot(&CONTRACT.replace('\n', "\r\n"), REVISION)
            .unwrap()
    );
}

#[test]
fn invalid_contracts_and_revisions_cannot_form_requests() {
    for contract in [
        CONTRACT.replace("issue = 17", "issue = 0"),
        CONTRACT.replace("issue = 17", "issue = 9007199254740992"),
        CONTRACT.replace("implementation", "planning_tracker"),
        CONTRACT.replace("approved offline", "unapproved live"),
        format!("{CONTRACT}\nunknown = true\n"),
    ] {
        assert!(AssignmentRequest::from_approved_snapshot(&contract, REVISION).is_err());
    }
    for revision in ["", "main", "abc", &"G".repeat(40)] {
        assert!(AssignmentRequest::from_approved_snapshot(CONTRACT, revision).is_err());
    }
}

#[test]
fn unknown_assignment_outcomes_are_not_success_or_retried() {
    let transport = FakeAssignment {
        capabilities: AssignmentCapabilities {
            exact_target: Capability::Supported,
            approved_snapshot: Capability::Supported,
            immutable_task: Capability::Unsupported,
        },
        calls: RefCell::default(),
        outcome: AssignmentOutcome::Unknown,
    };
    let request = AssignmentRequest::from_approved_snapshot(CONTRACT, REVISION).unwrap();
    assert_eq!(
        handoff(&transport, &request),
        Ok(AssignmentOutcome::Unknown)
    );
    assert_eq!(transport.calls.borrow().len(), 1);
}
