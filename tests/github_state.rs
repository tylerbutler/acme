use acme::assignment::{
    AssignmentCapabilities, AssignmentOutcome, AssignmentRequest, AssignmentTransport,
};
use acme::claims::{
    ClaimBlock, ClaimSession, ClaimStore, GitHubSnapshot, GitHubState, Receipt, ReceiptProvenance,
    ReconcileContext, ReconcileResult, Route, StoredReceipt, WorkflowRun, reconcile,
};
use acme::github_state::{GitHubClient, GitHubStateReader};
use acme::{
    CapacityState, DispatchDeclaration, Eligibility, Evidence, PlannerInput, ReasonCode,
    content_sha256, plan,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;

const MAIN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MERGE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SPEC: &str = "Approved implementation.";
const BODY: &str = "Build the bounded task.";
const PATH: &str = "factory/dispatch/task.toml";
const SPEC_PATH: &str = "docs/specs/task.md";

#[derive(Default)]
struct Fake {
    responses: BTreeMap<String, Value>,
    files: BTreeMap<(String, String), String>,
    reads: RefCell<Vec<String>>,
}

impl GitHubClient for Fake {
    fn get(&self, endpoint: &str) -> Result<Value, String> {
        self.reads.borrow_mut().push(endpoint.to_owned());
        self.responses
            .get(endpoint)
            .cloned()
            .ok_or_else(|| format!("unavailable GET {endpoint}"))
    }

    fn file(&self, repository: &str, path: &str, revision: &str) -> Result<String, String> {
        assert_eq!(repository, "owner/acme");
        self.reads.borrow_mut().push(format!("{path}@{revision}"));
        self.files
            .get(&(path.to_owned(), revision.to_owned()))
            .cloned()
            .ok_or_else(|| format!("unavailable file {path}@{revision}"))
    }

    fn graphql(&self, query: &str) -> Result<Value, String> {
        assert!(query.starts_with("query "));
        self.reads.borrow_mut().push(query.to_owned());
        let parent = if query.contains("pullRequest(number: ") {
            "pullRequest"
        } else {
            "issue"
        };
        let number = query
            .split(&format!("{parent}(number: "))
            .nth(1)
            .unwrap()
            .split(')')
            .next()
            .unwrap();
        let cursor = query
            .split("after: ")
            .nth(1)
            .unwrap()
            .split(')')
            .next()
            .unwrap();
        self.responses
            .get(&format!(
                "{}:{number}:{cursor}",
                if parent == "pullRequest" {
                    "closing"
                } else {
                    "closed"
                }
            ))
            .cloned()
            .ok_or_else(|| format!("unavailable query {query}"))
    }
}

fn contract() -> String {
    format!(
        r#"version = 1
repository = "owner/acme"
issue = 17
kind = "implementation"
prerequisites = []
[spec]
path = "{SPEC_PATH}"
sha256 = "{}"
[issue_body]
content = "{BODY}"
sha256 = "{}"
"#,
        content_sha256(SPEC),
        content_sha256(BODY)
    )
}

fn request() -> AssignmentRequest {
    AssignmentRequest::from_approved_snapshot(&contract(), MAIN).unwrap()
}

fn fixture() -> Fake {
    let mut fake = Fake::default();
    for (endpoint, value) in [
        ("user".to_owned(), json!({"login":"reader", "type":"User"})),
        (
            "repos/owner/acme".to_owned(),
            json!({"full_name":"owner/acme","default_branch":"main","owner":{"login":"owner","type":"User"}}),
        ),
        (
            "repos/owner/acme/branches/main".to_owned(),
            json!({"name":"main","protected":true,"commit":{"sha":MAIN}}),
        ),
        (
            format!("repos/owner/acme/git/trees/{MAIN}?recursive=1"),
            json!({"truncated":false,"tree":[{"path":PATH,"type":"blob","mode":"100644"}]}),
        ),
        (
            "repos/owner/acme/issues/17".to_owned(),
            json!({"number":17,"state":"open","body":BODY,"labels":[],"assignees":[]}),
        ),
        (
            "repos/owner/acme/issues/17/dependencies/blocked_by?per_page=100&page=1".to_owned(),
            json!([]),
        ),
        (
            "repos/owner/acme/issues/17/events?per_page=100&page=1".to_owned(),
            json!([]),
        ),
        (
            "repos/owner/acme/issues?state=all&per_page=100&page=1".to_owned(),
            json!([]),
        ),
        (
            "repos/owner/acme/pulls?state=all&per_page=100&page=1".to_owned(),
            json!([]),
        ),
        (
            "agents/repos/owner/acme/tasks?is_archived=false&per_page=100&page=1".to_owned(),
            json!({"tasks":[],"total_active_count":0,"total_archived_count":0}),
        ),
        (
            "agents/repos/owner/acme/tasks?is_archived=true&per_page=100&page=1".to_owned(),
            json!({"tasks":[],"total_active_count":0,"total_archived_count":0}),
        ),
        (
            "repos/owner/acme/actions/variables?per_page=100&page=1".to_owned(),
            json!({"total_count":3,"variables":[
                {"name":"ACME_AGENT_WORKFLOWS_ENABLED","value":"true"},
                {"name":"ACME_COORDINATOR_ENABLED","value":"true"},
                {"name":"ACME_CLOUD_ASSIGNMENT_ENABLED","value":"true"}
            ]}),
        ),
        (
            "repos/owner/acme/actions/secrets?per_page=100&page=1".to_owned(),
            json!({"total_count":1,"secrets":[{"name":"GH_PAT"}]}),
        ),
        (
            "repos/owner/acme/actions/workflows/build-cloud.lock.yml".to_owned(),
            json!({"state":"active"}),
        ),
        (
            format!("repos/owner/acme/compare/{MERGE}...{MAIN}"),
            json!({"status":"ahead"}),
        ),
        ("repos/owner/acme/pulls/10".to_owned(), merged_pr(10)),
    ] {
        fake.responses.insert(endpoint, value);
    }
    let config = include_str!("../factory/factory.toml").replace("tylerbutler/acme", "owner/acme")
        + "\n[coordinator]\nautomatic_dispatch = true\nmax_active_tasks = 4\nmax_assignments_per_reconciliation = 1\nmax_attempts = 2\n";
    for (path, content) in [
        (PATH, contract()),
        (SPEC_PATH, SPEC.to_owned()),
        ("factory/factory.toml", config),
    ] {
        fake.files
            .insert((path.to_owned(), MAIN.to_owned()), content.clone());
        fake.files
            .insert((path.to_owned(), MERGE.to_owned()), content);
        fake.responses.insert(
            format!("repos/owner/acme/commits?sha={MAIN}&path={path}&per_page=1"),
            json!([{"sha":MERGE}]),
        );
    }
    fake.responses.insert(
        format!("repos/owner/acme/commits/{MERGE}/pulls?per_page=100&page=1"),
        json!([{"number":10}]),
    );
    fake.responses.insert(
        "repos/owner/acme/pulls/10/files?per_page=100&page=1".to_owned(),
        json!([{"filename":PATH},{"filename":SPEC_PATH},{"filename":"factory/factory.toml"}]),
    );
    fake
}

fn merged_pr(number: u64) -> Value {
    json!({"number":number,"id":number + 1000,"state":"closed","merged":true,"changed_files":3,"merged_at":"2026-10-27T01:00:00Z",
        "merge_commit_sha":MERGE,"merged_by":{"login":"owner","type":"User"},
        "base":{"ref":"main","repo":{"full_name":"owner/acme"}}})
}

fn decision(snapshot: &GitHubSnapshot, issue: u64) -> Eligibility {
    let declarations: Vec<_> = snapshot
        .contracts
        .iter()
        .map(|contract| DispatchDeclaration::from_toml(contract).unwrap())
        .collect();
    plan(&PlannerInput {
        config: &snapshot.config,
        repository: &snapshot.repository,
        owner_login: &snapshot.owner_login,
        authorized_approvers: &snapshot.authorized_approvers,
        kill_switches: &snapshot.kill_switches,
        capacity: &Evidence::Known(CapacityState {
            active_work: 0,
            assignments_this_reconciliation: 0,
        }),
        declarations: &declarations,
        observations: &snapshot.observations,
    })
    .into_iter()
    .find(|row| row.issue == issue)
    .unwrap()
    .eligibility
}

fn closing(issues: &[u64], next: Option<&str>) -> Value {
    json!({"data":{"repository":{"pullRequest":{"closingIssuesReferences":{
        "nodes":issues.iter().map(|number| json!({"number":number,"repository":{"nameWithOwner":"owner/acme"}})).collect::<Vec<_>>(),
        "pageInfo":{"hasNextPage":next.is_some(),"endCursor":next}
    }}}}})
}

fn closed(pr: Option<u64>) -> Value {
    let nodes: Vec<_> = pr.into_iter().map(|number|json!({
        "id":format!("closed-{number}"),"createdAt":"2026-10-27T01:00:00Z",
        "closer":{"__typename":"PullRequest","number":number,"repository":{"nameWithOwner":"owner/acme"}}
    })).collect();
    json!({"data":{"repository":{"issue":{"timelineItems":{
        "nodes":nodes,"pageInfo":{"hasNextPage":false,"endCursor":null}
    }}}}})
}

#[test]
fn observes_protected_main_without_writes_or_recovery_guesses() {
    let fake = fixture();
    let state = GitHubStateReader::new(fake);
    let snapshot = state.observe(&request()).unwrap();
    assert_eq!(snapshot.main_revision, MAIN);
    assert_eq!(snapshot.contracts, vec![contract()]);
    assert_eq!(snapshot.work, Evidence::Known(vec![]));
    assert_eq!(snapshot.resolutions, Evidence::Unknown);
    assert_eq!(snapshot.authorized_approvers, vec!["owner"]);
    assert_eq!(decision(&snapshot, 17), Eligibility::Eligible);
    assert!(
        matches!(&snapshot.observations[0].approval, Evidence::Known(Some(approval))
        if approval.merged_by == "owner" && approval.spec_pull_request == 10)
    );
}

#[test]
fn owner_label_provenance_requires_the_latest_owner_application() {
    for (actor, kind, expected) in [
        ("owner", "User", true),
        ("attacker", "User", false),
        ("owner", "Bot", false),
    ] {
        let mut fake = fixture();
        fake.responses
            .get_mut("repos/owner/acme/issues/17")
            .unwrap()["labels"] = json!([{"name":"ready-for-agent"}]);
        fake.responses.insert("repos/owner/acme/issues/17/events?per_page=100&page=1".to_owned(), json!([
            {"id":1,"event":"labeled","label":{"name":"ready-for-agent"},"actor":{"login":"owner","type":"User"}},
            {"id":2,"event":"unlabeled","label":{"name":"ready-for-agent"},"actor":{"login":"owner","type":"User"}},
            {"id":3,"event":"labeled","label":{"name":"ready-for-agent"},"actor":{"login":actor,"type":kind}}
        ]));
        let snapshot = GitHubStateReader::new(fake).observe(&request()).unwrap();
        assert_eq!(
            matches!(snapshot.owner_ready, Evidence::Known(Some(_))),
            expected
        );
    }
}

fn manual_fixture() -> Fake {
    let mut fake = fixture();
    fake.responses.insert(
        format!("repos/owner/acme/git/trees/{MAIN}?recursive=1"),
        json!({"truncated":false,"tree":[]}),
    );
    fake.responses
        .get_mut("repos/owner/acme/issues/17")
        .unwrap()["labels"] = json!([{"name":"ready-for-agent"}]);
    fake.responses
        .get_mut("repos/owner/acme/issues/17")
        .unwrap()["updated_at"] = json!("2026-10-27T02:00:00Z");
    fake.responses.insert("repos/owner/acme/issues/17/events?per_page=100&page=1".to_owned(), json!([
        {"id":1,"created_at":"2026-10-27T02:00:00Z","event":"labeled","label":{"name":"ready-for-agent"},"actor":{"login":"owner","type":"User"}}
    ]));
    fake
}

#[test]
fn manual_legacy_route_does_not_enroll_an_old_spec_automatically() {
    let automatic = GitHubStateReader::new(manual_fixture())
        .observe(&request())
        .unwrap();
    assert!(automatic.contracts.is_empty());
    assert!(
        GitHubStateReader::manual(manual_fixture())
            .observe(&request())
            .is_err()
    );
}

#[test]
fn manual_reader_preserves_the_reviewed_contract_without_automatic_authority() {
    let mut fake = fixture();
    fake.responses
        .get_mut("repos/owner/acme/issues/17")
        .unwrap()["body"] = json!("Changed scope");
    let manual = GitHubStateReader::manual(fake).observe(&request()).unwrap();
    assert_eq!(manual.contracts, vec![contract()]);
    assert!(!manual.config.automatic_dispatch);
    assert_eq!(
        manual.observations[0].current_issue_body,
        Evidence::Known("Changed scope".to_owned())
    );
}

#[test]
fn reads_paginated_tasks_and_counts_review_waiting_prs() {
    let mut fake = fixture();
    fake.responses.insert(
        "repos/owner/acme/pulls?state=all&per_page=100&page=1".to_owned(),
        json!([{"number":20,"id":1020,"state":"open","merged_at":null}]),
    );
    fake.responses
        .insert("closing:20:null".to_owned(), closing(&[17], None));
    fake.responses.insert("agents/repos/owner/acme/tasks?is_archived=false&per_page=100&page=1".to_owned(),
        json!({"tasks":(0..100).map(|id|json!({"id":format!("old-{id}"),"state":"completed"})).collect::<Vec<_>>(),"total_active_count":101}));
    fake.responses.insert("agents/repos/owner/acme/tasks?is_archived=false&per_page=100&page=2".to_owned(),
        json!({"tasks":[{"id":"waiting","state":"waiting_for_user","artifacts":[{"provider":"github","type":"pull","data":{"id":1020}}]}],"total_active_count":101}));
    let snapshot = GitHubStateReader::new(fake).observe(&request()).unwrap();
    let Evidence::Known(work) = &snapshot.work else {
        panic!("missing capacity")
    };
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].issue, 17);
    assert_eq!(work[0].active_copilot, 1);
    assert_eq!(work[0].open_implementation_prs, 1);
    assert_eq!(
        decision(&snapshot, 17),
        Eligibility::Blocked(ReasonCode::ExistingWorkConflict)
    );
}

#[test]
fn unknown_active_task_binding_and_truncated_lists_block() {
    for task_response in [
        json!({"tasks":[{"id":"unknown","state":"queued","artifacts":[]}],"total_active_count":1}),
        json!({"tasks":[],"total_active_count":1}),
    ] {
        let mut fake = fixture();
        fake.responses.insert(
            "agents/repos/owner/acme/tasks?is_archived=false&per_page=100&page=1".to_owned(),
            task_response,
        );
        assert!(GitHubStateReader::new(fake).observe(&request()).is_err());
    }
}

#[test]
fn closing_reference_pagination_and_graphql_errors_are_not_empty_work() {
    let mut fake = fixture();
    fake.responses.insert(
        "repos/owner/acme/pulls?state=all&per_page=100&page=1".to_owned(),
        json!([{"number":20,"id":1020,"state":"open","merged_at":null}]),
    );
    fake.responses
        .insert("closing:20:null".to_owned(), closing(&[16], Some("next")));
    fake.responses
        .insert("closing:20:\"next\"".to_owned(), closing(&[17], None));
    let snapshot = GitHubStateReader::new(fake).observe(&request()).unwrap();
    assert!(matches!(snapshot.work, Evidence::Known(work) if work.len() == 2));
    let mut fake = fixture();
    fake.responses.insert(
        "repos/owner/acme/pulls?state=all&per_page=100&page=1".to_owned(),
        json!([{"number":20,"id":1020,"state":"open","merged_at":null}]),
    );
    fake.responses.insert(
        "closing:20:null".to_owned(),
        json!({"errors":[{"message":"denied"}]}),
    );
    assert!(GitHubStateReader::new(fake).observe(&request()).is_err());
}

#[test]
fn missing_gates_or_secret_metadata_never_enable_dispatch() {
    for (endpoint, response) in [
        (
            "repos/owner/acme/actions/variables?per_page=100&page=1",
            json!({"total_count":0,"variables":[]}),
        ),
        (
            "repos/owner/acme/actions/secrets?per_page=100&page=1",
            json!({"total_count":0,"secrets":[]}),
        ),
    ] {
        let mut fake = fixture();
        fake.responses.insert(endpoint.to_owned(), response);
        let snapshot = GitHubStateReader::new(fake).observe(&request()).unwrap();
        assert_ne!(decision(&snapshot, 17), Eligibility::Eligible);
    }
}

fn prerequisite_fixture() -> (Fake, AssignmentRequest) {
    let mut fake = fixture();
    fake.responses.get_mut("repos/owner/acme/pulls/10").unwrap()["changed_files"] = json!(4);
    fake.responses
        .insert("closed:16:null".to_owned(), closed(None));
    let dependent = contract().replace(
        "prerequisites = []",
        "prerequisites = [{ issue = 16, owner_acceptance_required = true }]",
    );
    let prerequisite = contract().replace("issue = 17", "issue = 16");
    for revision in [MAIN, MERGE] {
        fake.files
            .insert((PATH.to_owned(), revision.to_owned()), dependent.clone());
        fake.files.insert(
            (
                "factory/dispatch/prerequisite.toml".to_owned(),
                revision.to_owned(),
            ),
            prerequisite.clone(),
        );
    }
    fake.responses
        .get_mut(&format!("repos/owner/acme/git/trees/{MAIN}?recursive=1"))
        .unwrap()["tree"]
        .as_array_mut()
        .unwrap()
        .push(json!({"path":"factory/dispatch/prerequisite.toml","type":"blob","mode":"100644"}));
    fake.responses.insert(
        format!(
            "repos/owner/acme/commits?sha={MAIN}&path=factory/dispatch/prerequisite.toml&per_page=1"
        ),
        json!([{"sha":MERGE}]),
    );
    fake.responses
        .get_mut("repos/owner/acme/pulls/10/files?per_page=100&page=1")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .push(json!({"filename":"factory/dispatch/prerequisite.toml"}));
    fake.responses.insert(
        "repos/owner/acme/issues/16".to_owned(),
        json!({"number":16,"state":"closed","body":BODY,"labels":[],"assignees":[]}),
    );
    fake.responses.insert(
        "repos/owner/acme/issues/16/dependencies/blocked_by?per_page=100&page=1".to_owned(),
        json!([]),
    );
    let request = AssignmentRequest::from_approved_snapshot(&dependent, MAIN).unwrap();
    (fake, request)
}

#[test]
fn closed_issue_and_successful_agent_task_do_not_complete_prerequisites() {
    let (mut fake, request) = prerequisite_fixture();
    fake.responses.insert(
        "repos/owner/acme/pulls?state=all&per_page=100&page=1".to_owned(),
        json!([{"number":20,"state":"closed","merged_at":null}]),
    );
    fake.responses.insert(
        "agents/repos/owner/acme/tasks?is_archived=false&per_page=100&page=1".to_owned(),
        json!({"tasks":[{"id":"done","state":"completed"}],"total_active_count":1}),
    );
    let snapshot = GitHubStateReader::new(fake).observe(&request).unwrap();
    assert_eq!(
        decision(&snapshot, 17),
        Eligibility::Blocked(ReasonCode::PrerequisiteNotMerged)
    );
}

const IMPLEMENTATION: &str = "cccccccccccccccccccccccccccccccccccccccc";
const ACCEPTANCE: &str = "dddddddddddddddddddddddddddddddddddddddd";

fn merged_prerequisite() -> (Fake, AssignmentRequest) {
    let (mut fake, request) = prerequisite_fixture();
    fake.responses.insert(
        format!("repos/owner/acme/compare/{MERGE}...{IMPLEMENTATION}"),
        json!({"status":"ahead"}),
    );
    let mut pr = merged_pr(20);
    pr["merge_commit_sha"] = json!(IMPLEMENTATION);
    fake.responses.insert(
        "repos/owner/acme/pulls?state=all&per_page=100&page=1".to_owned(),
        json!([pr.clone()]),
    );
    fake.responses
        .insert("repos/owner/acme/pulls/20".to_owned(), pr);
    fake.responses.insert(
        format!("repos/owner/acme/compare/{IMPLEMENTATION}...{MAIN}"),
        json!({"status":"ahead"}),
    );
    fake.responses
        .insert("closing:20:null".to_owned(), closing(&[16], None));
    fake.responses
        .insert("closed:16:null".to_owned(), closed(Some(20)));
    (fake, request)
}

#[test]
fn human_merged_prerequisite_still_requires_a_separate_owner_receipt() {
    let (fake, request) = merged_prerequisite();
    let snapshot = GitHubStateReader::new(fake).observe(&request).unwrap();
    assert_eq!(
        decision(&snapshot, 17),
        Eligibility::Blocked(ReasonCode::OwnerAcceptanceMissing)
    );
}

fn accepted_prerequisite() -> (Fake, AssignmentRequest) {
    let (mut fake, request) = merged_prerequisite();
    let path = "factory/dispatch/acceptance/16.json";
    let content = json!({"version":1,"issue":16,"merge_commit":IMPLEMENTATION}).to_string();
    fake.responses
        .get_mut(&format!("repos/owner/acme/git/trees/{MAIN}?recursive=1"))
        .unwrap()["tree"]
        .as_array_mut()
        .unwrap()
        .push(json!({"path":path,"type":"blob","mode":"100644"}));
    for revision in [MAIN, ACCEPTANCE] {
        fake.files
            .insert((path.to_owned(), revision.to_owned()), content.clone());
    }
    fake.responses.insert(
        format!("repos/owner/acme/commits?sha={MAIN}&path={path}&per_page=1"),
        json!([{"sha":ACCEPTANCE}]),
    );
    fake.responses.insert(
        format!("repos/owner/acme/commits/{ACCEPTANCE}/pulls?per_page=100&page=1"),
        json!([{"number":21}]),
    );
    let mut pr = merged_pr(21);
    pr["merge_commit_sha"] = json!(ACCEPTANCE);
    pr["changed_files"] = json!(1);
    fake.responses
        .insert("repos/owner/acme/pulls/21".to_owned(), pr);
    fake.responses.insert(
        "repos/owner/acme/pulls/21/files?per_page=100&page=1".to_owned(),
        json!([{"filename":path}]),
    );
    for (base, head) in [(ACCEPTANCE, MAIN), (IMPLEMENTATION, ACCEPTANCE)] {
        fake.responses.insert(
            format!("repos/owner/acme/compare/{base}...{head}"),
            json!({"status":"ahead"}),
        );
    }
    (fake, request)
}

#[test]
fn owner_merged_acceptance_receipt_releases_the_dependent() {
    let (fake, request) = accepted_prerequisite();
    let snapshot = GitHubStateReader::new(fake).observe(&request).unwrap();
    assert_eq!(decision(&snapshot, 17), Eligibility::Eligible);
    let completion = &snapshot
        .observations
        .iter()
        .find(|row| row.issue == 16)
        .unwrap()
        .completion;
    assert!(matches!(completion, Evidence::Known(Some(completion))
            if completion.acceptance.as_ref().is_some_and(|receipt| receipt.accepted_by == "owner")));
}

#[test]
fn receipt_json_cannot_grant_owner_authority_or_name_a_different_merge() {
    for unauthorized in [false, true] {
        let (mut fake, request) = accepted_prerequisite();
        if unauthorized {
            fake.responses.get_mut("repos/owner/acme/pulls/21").unwrap()["merged_by"] =
                json!({"login":"attacker","type":"User"});
        } else {
            fake.files.insert(
                (
                    "factory/dispatch/acceptance/16.json".to_owned(),
                    MAIN.to_owned(),
                ),
                json!({"version":1,"issue":16,"merge_commit":MAIN,"accepted_by":"owner"})
                    .to_string(),
            );
        }
        assert!(GitHubStateReader::new(fake).observe(&request).is_err());
    }
}

#[test]
fn unauthorized_implementation_merge_and_missing_native_dependency_reads_block() {
    let (mut fake, request) = merged_prerequisite();
    fake.responses.get_mut("repos/owner/acme/pulls/20").unwrap()["merged_by"] =
        json!({"login":"attacker","type":"User"});
    assert!(GitHubStateReader::new(fake).observe(&request).is_err());
    let mut fake = fixture();
    fake.responses
        .remove("repos/owner/acme/issues/17/dependencies/blocked_by?per_page=100&page=1");
    assert!(
        GitHubStateReader::new(fake)
            .observe(&self::request())
            .is_err()
    );
}

#[test]
fn mutable_links_to_an_old_merge_do_not_complete_a_prerequisite() {
    let (mut fake, request) = merged_prerequisite();
    fake.responses
        .insert("closed:16:null".to_owned(), closed(None));
    let snapshot = GitHubStateReader::new(fake).observe(&request).unwrap();
    assert_eq!(
        decision(&snapshot, 17),
        Eligibility::Blocked(ReasonCode::PrerequisiteNotMerged)
    );
}

#[test]
fn retained_copilot_assignee_on_accepted_implementation_does_not_consume_capacity() {
    let (mut fake, request) = merged_prerequisite();
    fake.responses.insert(
        "repos/owner/acme/issues?state=all&per_page=100&page=1".to_owned(),
        json!([{"number":16,"state":"closed","assignees":[{"login":"Copilot"}]}]),
    );
    let snapshot = GitHubStateReader::new(fake).observe(&request).unwrap();
    assert_eq!(snapshot.work, Evidence::Known(vec![]));
}

#[test]
fn owner_review_is_required_for_capacity_configuration() {
    let mut fake = fixture();
    fake.responses.insert(
        format!("repos/owner/acme/commits?sha={MAIN}&path=factory/factory.toml&per_page=1"),
        json!([{"sha":IMPLEMENTATION}]),
    );
    fake.responses.insert(
        format!("repos/owner/acme/commits/{IMPLEMENTATION}/pulls?per_page=100&page=1"),
        json!([]),
    );
    assert!(GitHubStateReader::new(fake).observe(&request()).is_err());
}

#[test]
fn truncated_file_evidence_and_unprotected_merge_ancestry_block_approval() {
    for (endpoint, response) in [
        (
            "repos/owner/acme/pulls/10/files?per_page=100&page=1".to_owned(),
            json!([{"filename":PATH}]),
        ),
        (
            format!("repos/owner/acme/compare/{MERGE}...{MAIN}"),
            json!({"status":"diverged"}),
        ),
    ] {
        let mut fake = fixture();
        fake.responses.insert(endpoint, response);
        assert!(GitHubStateReader::new(fake).observe(&request()).is_err());
    }
}
#[test]
fn rejects_unprotected_or_substituted_main_and_missing_authentication() {
    for (endpoint, value) in [
        ("user".to_owned(), json!({})),
        (
            "repos/owner/acme".to_owned(),
            json!({"full_name":"attacker/acme"}),
        ),
        (
            "repos/owner/acme/branches/main".to_owned(),
            json!({"name":"main","protected":false,"commit":{"sha":MAIN}}),
        ),
        (
            "repos/owner/acme/branches/main".to_owned(),
            json!({"name":"main","protected":true,"commit":{"sha":MERGE}}),
        ),
        (
            format!("repos/owner/acme/git/trees/{MAIN}?recursive=1"),
            json!({"truncated":true,"tree":[]}),
        ),
    ] {
        let mut fake = fixture();
        fake.responses.insert(endpoint, value);
        assert!(GitHubStateReader::new(fake).observe(&request()).is_err());
    }
}

#[test]
fn preserves_changed_body_and_spec_for_claims_to_block() {
    let mut fake = fixture();
    fake.responses
        .get_mut("repos/owner/acme/issues/17")
        .unwrap()["body"] = json!("Changed scope");
    fake.files.insert(
        (SPEC_PATH.to_owned(), MAIN.to_owned()),
        "Changed spec".to_owned(),
    );
    let snapshot = GitHubStateReader::new(fake).observe(&request()).unwrap();
    assert_eq!(
        snapshot.observations[0].current_issue_body,
        Evidence::Known("Changed scope".to_owned())
    );
    assert_eq!(
        snapshot.observations[0].current_spec,
        Evidence::Known("Changed spec".to_owned())
    );
    assert!(!matches!(
        snapshot.observations[0].approval,
        Evidence::Known(Some(_))
    ));
}

#[test]
fn rejects_unauthorized_merge_and_unavailable_capacity_api() {
    let mut fake = fixture();
    fake.responses.get_mut("repos/owner/acme/pulls/10").unwrap()["merged_by"] =
        json!({"login":"agent","type":"Bot"});
    assert!(GitHubStateReader::new(fake).observe(&request()).is_err());
    let mut fake = fixture();
    fake.responses
        .remove("agents/repos/owner/acme/tasks?is_archived=false&per_page=100&page=1");
    assert!(GitHubStateReader::new(fake).observe(&request()).is_err());
}

#[test]
fn a_declaration_without_its_own_authorized_merge_stays_unapproved() {
    let mut fake = fixture();
    fake.responses.insert(
        format!("repos/owner/acme/commits?sha={MAIN}&path={PATH}&per_page=1"),
        json!([{"sha":IMPLEMENTATION}]),
    );
    fake.responses.insert(
        format!("repos/owner/acme/commits/{IMPLEMENTATION}/pulls?per_page=100&page=1"),
        json!([]),
    );
    let snapshot = GitHubStateReader::new(fake).observe(&request()).unwrap();
    assert_eq!(
        decision(&snapshot, 17),
        Eligibility::Blocked(ReasonCode::ApprovalUnknown)
    );
}

#[test]
fn disabled_remote_assignment_workflow_blocks_even_with_enabled_variables() {
    let mut fake = fixture();
    fake.responses.insert(
        "repos/owner/acme/actions/workflows/build-cloud.lock.yml".to_owned(),
        json!({"state":"disabled_manually"}),
    );
    let snapshot = GitHubStateReader::new(fake).observe(&request()).unwrap();
    assert_eq!(
        decision(&snapshot, 17),
        Eligibility::Blocked(ReasonCode::AssignmentKillSwitchActive)
    );
}

#[test]
fn a_new_approved_prerequisite_snapshot_cannot_reuse_old_implementation_or_acceptance() {
    let (mut fake, request) = accepted_prerequisite();
    let revision = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    let path = "factory/dispatch/prerequisite.toml";
    let old = fake.files.get(&(path.to_owned(), MAIN.to_owned())).unwrap();
    let content = old
        .replace(BODY, "New scope.")
        .replace(&content_sha256(BODY), &content_sha256("New scope."));
    fake.files
        .insert((path.to_owned(), MAIN.to_owned()), content.clone());
    fake.files
        .insert((path.to_owned(), revision.to_owned()), content);
    fake.responses
        .get_mut("repos/owner/acme/issues/16")
        .unwrap()["body"] = json!("New scope.");
    fake.responses.insert(
        format!("repos/owner/acme/commits?sha={MAIN}&path={path}&per_page=1"),
        json!([{"sha":revision}]),
    );
    fake.responses.insert(
        format!("repos/owner/acme/commits/{revision}/pulls?per_page=100&page=1"),
        json!([{"number":22}]),
    );
    let mut pr = merged_pr(22);
    pr["merge_commit_sha"] = json!(revision);
    pr["changed_files"] = json!(1);
    fake.responses
        .insert("repos/owner/acme/pulls/22".to_owned(), pr);
    fake.responses.insert(
        "repos/owner/acme/pulls/22/files?per_page=100&page=1".to_owned(),
        json!([{"filename":path}]),
    );
    fake.responses.insert(
        format!("repos/owner/acme/compare/{revision}...{MAIN}"),
        json!({"status":"ahead"}),
    );
    fake.responses.insert(
        format!("repos/owner/acme/compare/{revision}...{IMPLEMENTATION}"),
        json!({"status":"behind"}),
    );
    assert!(GitHubStateReader::new(fake).observe(&request).is_err());
}

fn manual_only_fixture() -> Fake {
    let mut fake = fixture();
    let path = "factory/dispatch/manual/task.toml";
    fake.responses
        .get_mut(&format!("repos/owner/acme/git/trees/{MAIN}?recursive=1"))
        .unwrap()["tree"] = json!([{"path":path,"type":"blob","mode":"100644"}]);
    for revision in [MAIN, MERGE] {
        fake.files
            .insert((path.to_owned(), revision.to_owned()), contract());
    }
    fake.responses.insert(
        format!("repos/owner/acme/commits?sha={MAIN}&path={path}&per_page=1"),
        json!([{"sha":MERGE}]),
    );
    fake.responses
        .get_mut("repos/owner/acme/pulls/10/files?per_page=100&page=1")
        .unwrap()[0]["filename"] = json!(path);
    fake.responses
        .get_mut("repos/owner/acme/issues/17")
        .unwrap()["labels"] = json!([{"name":"ready-for-agent"}]);
    fake.responses.insert("repos/owner/acme/issues/17/events?per_page=100&page=1".to_owned(), json!([
        {"id":1,"event":"labeled","label":{"name":"ready-for-agent"},"actor":{"login":"owner","type":"User"}}
    ]));
    fake
}

#[test]
fn owner_reviewed_manual_snapshots_do_not_enroll_automatic_dispatch() {
    let automatic = GitHubStateReader::new(manual_only_fixture())
        .observe(&request())
        .unwrap();
    assert!(automatic.contracts.is_empty());
    let manual = GitHubStateReader::manual(manual_only_fixture())
        .observe(&request())
        .unwrap();
    assert_eq!(manual.contracts, vec![contract()]);
    assert!(!manual.config.automatic_dispatch);
    assert!(matches!(manual.owner_ready, Evidence::Known(Some(_))));
}

#[derive(Default)]
struct Store(RefCell<Vec<Receipt>>);

struct Session<'a>(&'a Store);

impl ClaimStore for Store {
    type Session<'a> = Session<'a>;

    fn lock(&self, repository: &str) -> Result<Session<'_>, String> {
        assert_eq!(repository, "owner/acme");
        Ok(Session(self))
    }
}

impl ClaimSession for Session<'_> {
    fn load(&mut self) -> Result<Vec<StoredReceipt>, String> {
        Ok(self
            .0
            .0
            .borrow()
            .iter()
            .map(|receipt| StoredReceipt {
                receipt: receipt.clone(),
                provenance: Evidence::Known(ReceiptProvenance {
                    repository: "owner/acme".to_owned(),
                    writer: "github-actions[bot]".to_owned(),
                    protected_main: true,
                    run: receipt.recorded_by.clone(),
                }),
            })
            .collect())
    }

    fn append(&mut self, receipt: &Receipt) -> Result<(), String> {
        self.0.0.borrow_mut().push(receipt.clone());
        Ok(())
    }
}

#[derive(Default)]
struct Transport(RefCell<Vec<AssignmentRequest>>);

impl AssignmentTransport for Transport {
    fn capabilities(&self) -> AssignmentCapabilities {
        AssignmentCapabilities::gh_aw_0_93_5()
    }

    fn assign(&self, request: &AssignmentRequest) -> AssignmentOutcome {
        self.0.borrow_mut().push(request.clone());
        AssignmentOutcome::Accepted
    }
}

fn context() -> ReconcileContext {
    ReconcileContext {
        repository: "owner/acme".to_owned(),
        trusted_workflow: ".github/workflows/coordinator.yml".to_owned(),
        trusted_writer: "github-actions[bot]".to_owned(),
        run: WorkflowRun {
            run_id: 123,
            run_attempt: 1,
            workflow_path: ".github/workflows/coordinator.yml".to_owned(),
            workflow_revision: MAIN.to_owned(),
        },
    }
}

#[test]
fn production_observations_drive_both_claim_routes_with_exact_request_binding() {
    for (reader, route) in [
        (GitHubStateReader::new(fixture()), Route::Automatic),
        (
            GitHubStateReader::manual(manual_only_fixture()),
            Route::Manual,
        ),
    ] {
        let store = Store::default();
        let transport = Transport::default();
        assert_eq!(
            reconcile(&store, &reader, &transport, &context(), &request(), route),
            ReconcileResult::Assigned
        );
        assert_eq!(transport.0.borrow().as_slice(), &[request()]);
        assert_eq!(store.0.borrow().len(), 2);
        assert_eq!(
            reconcile(&store, &reader, &transport, &context(), &request(), route),
            ReconcileResult::Blocked(ClaimBlock::RecoveryRequired),
        );
        assert_eq!(transport.0.borrow().len(), 1);
    }
}

#[test]
fn failed_or_incomplete_reads_never_claim_or_assign() {
    for endpoint in [
        "user",
        "repos/owner/acme",
        "repos/owner/acme/branches/main",
        "repos/owner/acme/issues/17",
        "repos/owner/acme/issues/17/dependencies/blocked_by?per_page=100&page=1",
        "repos/owner/acme/pulls?state=all&per_page=100&page=1",
        "repos/owner/acme/issues?state=all&per_page=100&page=1",
        "repos/owner/acme/actions/variables?per_page=100&page=1",
        "repos/owner/acme/actions/secrets?per_page=100&page=1",
        "repos/owner/acme/actions/workflows/build-cloud.lock.yml",
        "agents/repos/owner/acme/tasks?is_archived=false&per_page=100&page=1",
        "agents/repos/owner/acme/tasks?is_archived=true&per_page=100&page=1",
    ] {
        let mut fake = fixture();
        assert!(fake.responses.remove(endpoint).is_some());
        let store = Store::default();
        let transport = Transport::default();
        assert!(
            matches!(
                reconcile(
                    &store,
                    &GitHubStateReader::new(fake),
                    &transport,
                    &context(),
                    &request(),
                    Route::Automatic
                ),
                ReconcileResult::Blocked(ClaimBlock::GitHub(_))
            ),
            "{endpoint}"
        );
        assert!(transport.0.borrow().is_empty(), "{endpoint}");
        assert!(store.0.borrow().is_empty(), "{endpoint}");
    }
}

#[test]
fn delayed_immutable_closure_events_still_prove_the_merge_binding() {
    let (mut fake, request) = accepted_prerequisite();
    fake.responses.get_mut("closed:16:null").unwrap()["data"]["repository"]["issue"]["timelineItems"]
        ["nodes"][0]["createdAt"] = json!("2026-10-27T01:00:01Z");
    let snapshot = GitHubStateReader::new(fake).observe(&request).unwrap();
    assert_eq!(decision(&snapshot, 17), Eligibility::Eligible);
}

#[test]
fn irrelevant_historical_pr_links_are_not_required_for_capacity() {
    let mut fake = fixture();
    fake.responses.insert(
        "repos/owner/acme/pulls?state=all&per_page=100&page=1".to_owned(),
        json!([merged_pr(20)]),
    );
    let snapshot = GitHubStateReader::new(fake).observe(&request()).unwrap();
    assert_eq!(snapshot.work, Evidence::Known(vec![]));
    assert_eq!(decision(&snapshot, 17), Eligibility::Eligible);
}
