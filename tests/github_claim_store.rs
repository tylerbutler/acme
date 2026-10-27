use acme::assignment::{
    AssignmentCapabilities, AssignmentOutcome, AssignmentPermit, AssignmentRequest,
    AssignmentTransport,
};
use acme::claim_store::process::{ProcessAttestation, ProcessRequest, ProcessRunner};
use acme::claim_store::strict_json;
use acme::claim_store::{
    ClaimClient, ClaimPolicy, Entry, GitHubClaimStore, JOURNAL_REF, Operation, POLICY_PATH,
    PREDICATE, WORKFLOW,
};
use acme::claims::{
    AttemptId, ClaimSession, ClaimStore, Receipt, ReceiptState, RequestBinding, Route, WorkflowRun,
};
use acme::github_state::GitHubClient;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Barrier, Mutex};

const REPO: &str = "owner/acme";
const MAIN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MERGE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const GENESIS: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn b64(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let word = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            result.push(if index > chunk.len() {
                '='
            } else {
                char::from(alphabet[((word >> shift) & 63) as usize])
            });
        }
    }
    result
}

fn unb64(value: &str) -> Vec<u8> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    value
        .as_bytes()
        .chunks(4)
        .flat_map(|chunk| {
            let mut word = 0_u32;
            for byte in chunk {
                word = (word << 6)
                    | u32::try_from(
                        alphabet
                            .iter()
                            .position(|candidate| candidate == byte)
                            .unwrap_or(0),
                    )
                    .unwrap();
            }
            let bytes = word.to_be_bytes();
            let padding = usize::from(chunk[2] == b'=') + usize::from(chunk[3] == b'=');
            bytes[1..4 - padding].to_vec()
        })
        .collect()
}

fn run(id: u64) -> WorkflowRun {
    WorkflowRun {
        run_id: id,
        run_attempt: 1,
        workflow_path: WORKFLOW.to_owned(),
        workflow_revision: MAIN.to_owned(),
    }
}

fn policy() -> ClaimPolicy {
    ClaimPolicy {
        version: 1,
        repository: REPO.to_owned(),
        repository_id: 42,
        owner_id: 7,
        journal_ref: JOURNAL_REF.to_owned(),
        genesis_oid: GENESIS.to_owned(),
        workflow_path: WORKFLOW.to_owned(),
        max_entries: 100,
        max_entry_bytes: 32_768,
        max_bundle_bytes: 65_536,
        max_api_requests: 100_000,
    }
}

#[derive(Default)]
struct Server {
    head: String,
    next: u64,
    snapshots: BTreeMap<String, BTreeMap<String, String>>,
    objects: BTreeMap<String, Value>,
    reads: Vec<String>,
    mutations: Vec<Value>,
    policy: String,
    fail_mutation: Option<(usize, bool)>,
    protected: bool,
    bypass: bool,
    certificate_fault: Option<String>,
    read_fault: Option<String>,
    corrupt_signature: bool,
    pagination: Pagination,
    changed_approval_bytes: Option<&'static str>,
    repository_metadata_fault: Option<&'static str>,
}

#[derive(Default)]
struct Pagination {
    rules: bool,
    files: bool,
}

impl Server {
    fn id(&mut self) -> String {
        self.next += 1;
        format!("{:040x}", self.next)
    }
    fn blob(&mut self, bytes: &[u8]) -> String {
        let id = self.id();
        self.objects.insert(
            format!("git/blobs/{id}"),
            json!({
                "sha":id,"size":bytes.len(),"encoding":"base64","content":b64(bytes)
            }),
        );
        id
    }
    fn commit(
        &mut self,
        parent: Option<&str>,
        files: BTreeMap<String, String>,
        fixed: Option<&str>,
    ) -> String {
        let id = fixed.map_or_else(|| self.id(), str::to_owned);
        let tree_id = self.id();
        let mut directories = BTreeSet::new();
        let mut rows = Vec::new();
        for (path, blob) in &files {
            rows.push(json!({"path":path,"type":"blob","mode":"100644","sha":blob}));
            let mut path = path.as_str();
            while let Some((directory, _)) = path.rsplit_once('/') {
                directories.insert(directory.to_owned());
                path = directory;
            }
        }
        for directory in directories {
            rows.push(json!({"path":directory,"type":"tree","mode":"040000","sha":self.id()}));
        }
        rows.reverse();
        self.objects.insert(
            format!("git/trees/{tree_id}?recursive=1"),
            json!({"sha":tree_id,"truncated":false,"tree":rows}),
        );
        self.objects.insert(
            format!("git/commits/{id}"),
            json!({
                "sha":id,"tree":{"sha":tree_id},
                "parents":parent.map_or_else(Vec::new, |sha| vec![json!({"sha":sha})])
            }),
        );
        self.snapshots.insert(id.clone(), files);
        self.head.clone_from(&id);
        id
    }
    fn new() -> Self {
        let mut server = Self {
            next: 100,
            policy: serde_json::to_string(&policy()).unwrap(),
            protected: true,
            ..Self::default()
        };
        let genesis = json!({"version":1,"repository":REPO,"repository_id":42,
            "owner_id":7,"journal_ref":JOURNAL_REF});
        let blob = server.blob(&serde_json::to_vec(&genesis).unwrap());
        server.commit(
            None,
            BTreeMap::from([("journal/genesis.json".to_owned(), blob)]),
            Some(GENESIS),
        );
        server
    }
    fn entries(&self) -> Vec<Entry> {
        let files = &self.snapshots[&self.head];
        files
            .iter()
            .filter(|(path, _)| path.ends_with("/entry.json"))
            .map(|(_, blob)| {
                strict_json(&unb64(
                    self.objects[&format!("git/blobs/{blob}")]["content"]
                        .as_str()
                        .unwrap(),
                ))
                .unwrap()
            })
            .collect()
    }
}

fn corrupt_directory(server: &mut Server, tree: &str, fault: &str) {
    let id = server.id();
    let rows = server
        .objects
        .get_mut(&format!("git/trees/{tree}?recursive=1"))
        .unwrap()["tree"]
        .as_array_mut()
        .unwrap();
    if fault == "duplicate_directory" {
        let duplicate = rows
            .iter()
            .find(|row| {
                row["type"] == "tree"
                    && row["path"]
                        .as_str()
                        .unwrap()
                        .starts_with("journal/entries/")
            })
            .unwrap()
            .clone();
        rows.push(duplicate);
    } else {
        let sequence = if fault == "extra_directory" {
            "00000000000000000099"
        } else {
            "aaaaaaaaaaaaaaaaaaaa"
        };
        rows.push(json!({"path":format!("journal/entries/{sequence}"),
            "type":"tree","mode":"040000","sha":id}));
    }
}

#[derive(Clone)]
struct Api {
    server: Arc<Mutex<Server>>,
    barrier: Option<Arc<Barrier>>,
}

impl Api {
    fn new() -> Self {
        Self {
            server: Arc::new(Mutex::new(Server::new())),
            barrier: None,
        }
    }
}

impl GitHubClient for Api {
    fn get(&self, endpoint: &str) -> Result<Value, String> {
        let mut server = self.server.lock().unwrap();
        server.reads.push(endpoint.to_owned());
        let path = endpoint
            .strip_prefix(&format!("repos/{REPO}"))
            .ok_or("foreign repository")?;
        if server
            .read_fault
            .as_ref()
            .is_some_and(|fault| path.contains(fault))
        {
            return Err("injected missing API evidence".to_owned());
        }
        if path.is_empty() {
            let mut repository = json!({"id":42,"full_name":REPO,"default_branch":"main",
                "private":false,"visibility":"public",
                "owner":{"id":7,"login":"owner","type":"User"}});
            match server.repository_metadata_fault {
                Some("private") => repository["private"] = json!(true),
                Some("missing_private") => {
                    repository.as_object_mut().unwrap().remove("private");
                }
                Some("nonboolean_private") => repository["private"] = json!("false"),
                Some("private_visibility") => repository["visibility"] = json!("private"),
                Some("missing_visibility") => {
                    repository.as_object_mut().unwrap().remove("visibility");
                }
                _ => {}
            }
            return Ok(repository);
        }
        if path == "/branches/main" {
            return Ok(json!({"name":"main","protected":true,"commit":{"sha":MAIN}}));
        }
        if path.starts_with("/compare/") {
            return Ok(json!({"status":"ahead"}));
        }
        if path.starts_with("/commits?") {
            return Ok(json!([{"sha":MERGE}]));
        }
        if path == format!("/commits/{MERGE}/pulls?per_page=100&page=1") {
            return Ok(json!([{"number":1}]));
        }
        if path == "/pulls/1" {
            return Ok(
                json!({"number":1,"merged":true,"state":"closed","merged_at":"2026-10-27",
                "merge_commit_sha":MERGE,"merged_by":{"type":"User","login":"owner"},
                "base":{"ref":"main","repo":{"full_name":REPO}},
                "changed_files":if server.pagination.files { 102 } else { 2 }}),
            );
        }
        if path.starts_with("/pulls/1/files?") {
            if !server.pagination.files || path.ends_with("page=2") {
                return Ok(json!([{"filename":POLICY_PATH},{"filename":WORKFLOW}]));
            }
            return Ok(Value::Array(
                (0..100)
                    .map(|id| json!({"filename":format!("other-{id}")}))
                    .collect(),
            ));
        }
        if path.starts_with("/rules/branches/acme-claims?") {
            if !server.protected {
                return Ok(json!([]));
            }
            let rules = [
                json!({"type":"deletion","ruleset_id":9,"ruleset_source_type":"Repository","ruleset_source":REPO}),
                json!({"type":"non_fast_forward","ruleset_id":9,"ruleset_source_type":"Repository","ruleset_source":REPO}),
                json!({"type":"required_linear_history","ruleset_id":9,"ruleset_source_type":"Repository","ruleset_source":REPO}),
            ];
            if server.pagination.rules && path.ends_with("page=1") {
                let mut rows: Vec<Value> = (0..99)
                    .map(|id| json!({"type":format!("other-{id}")}))
                    .collect();
                rows.push(rules[0].clone());
                return Ok(Value::Array(rows));
            }
            return Ok(Value::Array(if server.pagination.rules {
                rules[1..].to_vec()
            } else {
                rules.to_vec()
            }));
        }
        if path == "/rulesets/9" {
            return Ok(json!({"id":9,"enforcement":"active","target":"branch",
                "bypass_actors":if server.bypass { json!([{"actor_id":7}]) } else { json!([]) },
                "rules":[{"type":"deletion"},{"type":"non_fast_forward"},{"type":"required_linear_history"}]}));
        }
        if let Some(suffix) = path.strip_prefix("/actions/runs/") {
            let parts: Vec<_> = suffix.split('/').collect();
            let id: u64 = parts[0].parse().unwrap();
            let attempt: u32 = parts[2].parse().unwrap();
            return Ok(
                json!({"id":id,"run_attempt":attempt,"path":WORKFLOW,"head_sha":MAIN,
                "head_branch":"main","status":"completed","conclusion":"cancelled",
                "repository":{"id":42,"full_name":REPO,"owner":{"id":7}}}),
            );
        }
        if path == "/git/ref/heads/acme-claims" {
            return Ok(json!({"ref":JOURNAL_REF,"object":{"type":"commit","sha":server.head}}));
        }
        server
            .objects
            .get(path.trim_start_matches('/'))
            .cloned()
            .ok_or_else(|| format!("unavailable fake endpoint {path}"))
    }

    fn file(&self, repository: &str, path: &str, revision: &str) -> Result<String, String> {
        assert_eq!(repository, REPO);
        assert!([MAIN, MERGE].contains(&revision));
        let mut server = self.server.lock().unwrap();
        server.reads.push(format!("{path}@{revision}"));
        let content = match path {
            POLICY_PATH => server.policy.clone(),
            WORKFLOW => "owner-approved deterministic producer\n".to_owned(),
            _ => return Err("unexpected file".to_owned()),
        };
        if revision == MERGE && server.changed_approval_bytes == Some(path) {
            Ok(content.replace('\n', "\r\n"))
        } else {
            Ok(content)
        }
    }

    fn graphql(&self, _: &str) -> Result<Value, String> {
        panic!("unexpected GraphQL read");
    }
}

impl ClaimClient for Api {
    fn create_commit(&self, input: &Value) -> Result<String, String> {
        if let Some(barrier) = &self.barrier {
            barrier.wait();
        }
        let mut server = self.server.lock().unwrap();
        server.mutations.push(input.clone());
        let fail = server
            .fail_mutation
            .is_some_and(|(number, _)| number == server.mutations.len());
        if fail && !server.fail_mutation.unwrap().1 {
            return Err("mutation outcome unknown".to_owned());
        }
        let parent = input["expectedHeadOid"].as_str().unwrap();
        if parent != server.head {
            return Err("expected-head conflict".to_owned());
        }
        assert_eq!(input["branch"]["repositoryNameWithOwner"], REPO);
        assert_eq!(input["branch"]["branchName"], "acme-claims");
        let mut files = server.snapshots[parent].clone();
        let additions = input["fileChanges"]["additions"].as_array().unwrap();
        assert_eq!(additions.len(), 2);
        for addition in additions {
            let path = addition["path"].as_str().unwrap();
            assert!(!files.contains_key(path));
            let blob = server.blob(&unb64(addition["contents"].as_str().unwrap()));
            files.insert(path.to_owned(), blob);
        }
        let commit = server.commit(Some(parent), files, None);
        drop(server);
        if fail {
            Err("response lost after commit".to_owned())
        } else {
            Ok(commit)
        }
    }
}

#[derive(Clone)]
struct Processes(Arc<Mutex<Server>>);

impl ProcessRunner for Processes {
    fn run(&self, request: &ProcessRequest) -> Result<Vec<u8>, String> {
        assert!(!request.environment.contains_key("GH_PAT"));
        assert!(!request.environment.contains_key("NODE_OPTIONS"));
        if request.program.ends_with("node") {
            assert!(!request.environment.contains_key("GH_TOKEN"));
            assert_eq!(
                request.arguments,
                [format!(
                    "{}/scripts/claim-attestation/sign.cjs",
                    env!("CARGO_MANIFEST_DIR")
                )]
            );
            let entry: Entry = strict_json(&request.stdin)?;
            return Ok(
                serde_json::to_vec(&json!({"entry_sha256":sha(&request.stdin),
                "run":entry.recorded_by,"signature":"valid-test-signature"}))
                .unwrap(),
            );
        }
        if request.arguments == ["--version"] {
            return Ok(b"gh version 2.102.0 (2026-09-30)\n".to_vec());
        }
        let artifact = std::fs::read(&request.arguments[2]).unwrap();
        let arg = |name| {
            request
                .arguments
                .iter()
                .position(|argument| argument == name)
                .map(|index| request.arguments[index + 1].clone())
                .unwrap()
        };
        for flag in [
            "--signer-digest",
            "--source-digest",
            "--source-ref",
            "--predicate-type",
            "--repo",
            "--bundle",
        ] {
            assert!(!arg(flag).is_empty());
        }
        assert!(
            request
                .arguments
                .contains(&"--deny-self-hosted-runners".to_owned())
        );
        assert!(!request.arguments.contains(&"--no-public-good".to_owned()));
        let bundle: Value = strict_json(&std::fs::read(arg("--bundle")).unwrap())?;
        let entry: Entry = strict_json(&artifact)?;
        assert_eq!(
            arg("--signer-workflow"),
            format!("github.com/{REPO}/{WORKFLOW}@refs/heads/main")
        );
        assert_eq!(arg("--signer-digest"), entry.recorded_by.workflow_revision);
        assert_eq!(arg("--source-digest"), entry.recorded_by.workflow_revision);
        assert_eq!(arg("--source-ref"), "refs/heads/main");
        assert_eq!(arg("--repo"), REPO);
        assert_eq!(arg("--hostname"), "github.com");
        assert_eq!(arg("--predicate-type"), PREDICATE);
        assert_eq!(
            arg("--cert-oidc-issuer"),
            "https://token.actions.githubusercontent.com"
        );
        assert_eq!(arg("--digest-alg"), "sha256");
        assert_eq!(arg("--format"), "json");
        let server = self.0.lock().unwrap();
        if server.corrupt_signature
            || bundle["entry_sha256"] != sha(&artifact)
            || bundle["signature"] != "valid-test-signature"
        {
            return Err("invalid cryptographic signature".to_owned());
        }
        let signer = format!("https://github.com/{REPO}/{WORKFLOW}@refs/heads/main");
        let mut certificate = json!({
            "issuer":"https://token.actions.githubusercontent.com",
            "subjectAlternativeName":signer,
            "sourceRepositoryIdentifier":"42","sourceRepositoryOwnerIdentifier":"7",
            "sourceRepositoryOwnerURI":"https://github.com/owner",
            "sourceRepositoryURI":format!("https://github.com/{REPO}"),
            "sourceRepositoryRef":"refs/heads/main","sourceRepositoryDigest":entry.recorded_by.workflow_revision,
            "buildSignerURI":signer,"buildSignerDigest":entry.recorded_by.workflow_revision,
            "buildConfigURI":signer,"buildConfigDigest":entry.recorded_by.workflow_revision,
            "runnerEnvironment":"github-hosted",
            "runInvocationURI":format!("https://github.com/{REPO}/actions/runs/{}/attempts/{}",
                entry.recorded_by.run_id, entry.recorded_by.run_attempt)
        });
        if let Some(field) = &server.certificate_fault {
            certificate[field] = json!("forged");
        }
        drop(server);
        Ok(serde_json::to_vec(&json!([{"verificationResult":{
            "signature":{"certificate":certificate},
            "verifiedTimestamps":[{"type":"Tlog","timestamp":"2026-10-27T20:00:00Z"}],
            "statement":{"subject":[{"name":"entry.json","digest":{"sha256":sha(&artifact)}}],
                "predicateType":PREDICATE,"predicate":{}}
        }}]))
        .unwrap())
    }
}

type Store = GitHubClaimStore<Api, ProcessAttestation<Processes>>;

fn store(api: &Api, id: u64) -> Store {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    GitHubClaimStore::new(
        api.clone(),
        ProcessAttestation::new(
            root,
            PathBuf::from("/fake/node"),
            PathBuf::from("/fake/gh"),
            Processes(api.server.clone()),
        )
        .unwrap(),
        REPO,
        run(id),
    )
    .unwrap()
}

fn request() -> AssignmentRequest {
    let contract = format!(
        r#"version = 1
repository = "{REPO}"
issue = 17
kind = "implementation"
prerequisites = []
[spec]
path = "docs/specs/0011-github-claim-store.md"
sha256 = "{}"
[issue_body]
content = "approved"
sha256 = "{}"
"#,
        acme::content_sha256("spec"),
        acme::content_sha256("approved")
    );
    AssignmentRequest::from_approved_snapshot(&contract, MAIN).unwrap()
}

fn pending(id: u64) -> Receipt {
    Receipt {
        version: 2,
        attempt: AttemptId {
            binding: RequestBinding::from(&request()),
            number: 1,
            run: run(id),
        },
        route: Route::Automatic,
        state: ReceiptState::Pending,
        recovery: None,
        recorded_by: run(id),
    }
}

struct Transport {
    api: Api,
    calls: Mutex<usize>,
    outcome: AssignmentOutcome,
}

impl AssignmentTransport for Transport {
    fn capabilities(&self) -> AssignmentCapabilities {
        AssignmentCapabilities::gh_aw_0_93_5()
    }
    fn assign(&self, permit: AssignmentPermit<'_>) -> AssignmentOutcome {
        let entries = self.api.server.lock().unwrap().entries();
        assert!(matches!(
            entries.last().unwrap().operation,
            Operation::ArmAssignment { .. }
        ));
        assert_eq!(permit.request(), &request());
        assert_eq!(permit.attempt(), &pending(1).attempt);
        assert_eq!(permit.generation().len(), 64);
        *self.calls.lock().unwrap() += 1;
        self.outcome.clone()
    }
}

#[test]
fn actual_store_acquires_verifies_receipts_arms_and_releases_only_after_durable_outcome() {
    let api = Api::new();
    let store = store(&api, 1);
    let mut held = store.lock(REPO).unwrap();
    assert!(held.load().unwrap().is_empty());
    let mut receipt = pending(1);
    held.append(&receipt).unwrap();
    held.arm_assignment(&receipt.attempt).unwrap();
    let transport = Transport {
        api: api.clone(),
        calls: Mutex::new(0),
        outcome: AssignmentOutcome::Accepted,
    };
    assert_eq!(
        acme::assignment::handoff(&transport, &request(), &receipt.attempt, &mut held),
        Ok(AssignmentOutcome::Accepted)
    );
    receipt.state = ReceiptState::Accepted;
    held.append(&receipt).unwrap();
    held.release().unwrap();
    assert!(held.load().is_err());
    assert!(held.append(&receipt).is_err());
    assert!(held.release().is_err());
    assert_eq!(*transport.calls.lock().unwrap(), 1);
    let mut server = api.server.lock().unwrap();
    assert_eq!(server.entries().len(), 5);
    assert!(
        matches!(server.entries()[3].operation, Operation::Receipt { ref receipt, .. }
        if receipt.state == ReceiptState::Accepted)
    );
    assert!(matches!(
        server.entries()[4].operation,
        Operation::Release { .. }
    ));
    server.pagination.rules = true;
    server.pagination.files = true;
    drop(server);
    let next = store_for(&api, 2);
    let mut next = next.lock(REPO).unwrap();
    let receipts = next.load().unwrap();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].receipt.state, ReceiptState::Pending);
    assert_eq!(receipts[1].receipt.state, ReceiptState::Accepted);
    next.release().unwrap();
}

fn store_for(api: &Api, id: u64) -> Store {
    store(api, id)
}

#[test]
fn independent_manual_automatic_and_same_run_producers_cannot_adopt_the_cas_winner() {
    for same_run in [false, true] {
        let mut api = Api::new();
        api.barrier = Some(Arc::new(Barrier::new(2)));
        let first = api.clone();
        let second = api.clone();
        let a = std::thread::spawn(move || {
            let store = store(&first, 1);
            store.lock(REPO).is_ok()
        });
        let b = std::thread::spawn(move || {
            let store = store(&second, if same_run { 1 } else { 2 });
            store.lock(REPO).is_ok()
        });
        let winners = usize::from(a.join().unwrap()) + usize::from(b.join().unwrap());
        assert_eq!(winners, 1);
        let server = api.server.lock().unwrap();
        assert_eq!(server.mutations.len(), 2);
        assert_eq!(server.entries().len(), 1);
        assert_eq!(
            server.mutations[0]["expectedHeadOid"],
            server.mutations[1]["expectedHeadOid"]
        );
        let nonce_a: Entry = strict_json(&unb64(
            server.mutations[0]["fileChanges"]["additions"][0]["contents"]
                .as_str()
                .unwrap(),
        ))
        .unwrap();
        let nonce_b: Entry = strict_json(&unb64(
            server.mutations[1]["fileChanges"]["additions"][0]["contents"]
                .as_str()
                .unwrap(),
        ))
        .unwrap();
        assert_ne!(nonce_a.session_nonce, nonce_b.session_nonce);
        drop(server);
        api.barrier = None;
        assert!(store(&api, 999).lock(REPO).is_err());
    }
}

#[test]
fn cancellation_drop_terminal_status_and_new_nonce_never_release_or_take_over() {
    let api = Api::new();
    let original = store(&api, 1);
    let held = original.lock(REPO).unwrap();
    drop(held);
    assert!(original.lock(REPO).is_err());
    for id in [1, 2, u64::MAX] {
        assert!(store(&api, id).lock(REPO).is_err());
    }
    assert_eq!(api.server.lock().unwrap().entries().len(), 1);
}

#[test]
fn forged_signature_and_each_verified_certificate_identity_fail_closed() {
    for field in [
        "issuer",
        "subjectAlternativeName",
        "sourceRepositoryOwnerURI",
        "sourceRepositoryIdentifier",
        "sourceRepositoryOwnerIdentifier",
        "sourceRepositoryURI",
        "sourceRepositoryRef",
        "sourceRepositoryDigest",
        "buildSignerURI",
        "buildSignerDigest",
        "buildConfigURI",
        "buildConfigDigest",
        "runnerEnvironment",
        "runInvocationURI",
    ] {
        let api = Api::new();
        api.server.lock().unwrap().certificate_fault = Some(field.to_owned());
        assert!(store(&api, 1).lock(REPO).is_err(), "{field}");
        assert!(api.server.lock().unwrap().mutations.is_empty());
    }
    let api = Api::new();
    api.server.lock().unwrap().corrupt_signature = true;
    assert!(store(&api, 1).lock(REPO).is_err());
    assert!(api.server.lock().unwrap().mutations.is_empty());
}

#[test]
fn private_or_incomplete_visibility_metadata_blocks_before_signing_or_acquisition() {
    for fault in [
        "private",
        "missing_private",
        "nonboolean_private",
        "private_visibility",
        "missing_visibility",
    ] {
        let api = Api::new();
        api.server.lock().unwrap().repository_metadata_fault = Some(fault);
        assert!(store(&api, 1).lock(REPO).is_err(), "{fault}");
        let server = api.server.lock().unwrap();
        assert!(server.mutations.is_empty(), "{fault}");
        assert_eq!(
            server.reads,
            [format!("{POLICY_PATH}@{MAIN}"), format!("repos/{REPO}")],
            "{fault}"
        );
        assert!(server.entries().is_empty(), "{fault}");
    }
}

#[test]
fn missing_protection_bypass_and_incomplete_approval_block_before_acquisition() {
    for fault in ["protection", "bypass", "/pulls/1/files", "/compare/"] {
        let api = Api::new();
        {
            let mut server = api.server.lock().unwrap();
            match fault {
                "protection" => server.protected = false,
                "bypass" => server.bypass = true,
                other => server.read_fault = Some(other.to_owned()),
            }
        }
        assert!(store(&api, 1).lock(REPO).is_err(), "{fault}");
        assert!(api.server.lock().unwrap().mutations.is_empty());
    }
}

#[test]
fn committed_then_timeout_is_confirmed_once_but_unresolved_append_poison_retains_owner() {
    for commits in [false, true] {
        let api = Api::new();
        let owner = store(&api, 1);
        let mut held = owner.lock(REPO).unwrap();
        {
            let mut server = api.server.lock().unwrap();
            server.fail_mutation = Some((2, commits));
        }
        assert_eq!(held.append(&pending(1)).is_ok(), commits);
        if !commits {
            assert!(held.append(&pending(1)).is_err());
            assert!(held.release().is_err());
        }
        drop(held);
        assert!(store(&api, 2).lock(REPO).is_err());
        assert_eq!(api.server.lock().unwrap().mutations.len(), 2);
    }
}

struct BudgetRevisionClient(Api);

impl GitHubClient for BudgetRevisionClient {
    fn get(&self, endpoint: &str) -> Result<Value, String> {
        let path = endpoint.strip_prefix(&format!("repos/{REPO}")).unwrap();
        if path == "/branches/main" {
            return Ok(json!({"name":"main","protected":true,"commit":{"sha":MERGE}}));
        }
        if path.starts_with(&format!("/commits?sha={MAIN}&")) {
            return Ok(json!([{"sha":MAIN}]));
        }
        if path == format!("/commits/{MAIN}/pulls?per_page=100&page=1") {
            return Ok(json!([{"number":1}]));
        }
        if path == format!("/commits/{MERGE}/pulls?per_page=100&page=1") {
            return Ok(json!([{"number":2}]));
        }
        if path == "/pulls/1" || path == "/pulls/2" {
            let mut pr = self.0.get(&format!("repos/{REPO}/pulls/1"))?;
            pr["number"] = json!(if path.ends_with('1') { 1 } else { 2 });
            pr["merge_commit_sha"] = json!(if path.ends_with('1') { MAIN } else { MERGE });
            return Ok(pr);
        }
        if path.starts_with("/pulls/2/files?") {
            return self.0.get(&endpoint.replace("/pulls/2/", "/pulls/1/"));
        }
        let mut value = self.0.get(endpoint)?;
        if path.starts_with("/actions/runs/2/") {
            value["head_sha"] = json!(MERGE);
        }
        Ok(value)
    }

    fn file(&self, repository: &str, path: &str, revision: &str) -> Result<String, String> {
        let bytes = self.0.file(repository, path, revision)?;
        if path == POLICY_PATH && revision == MERGE {
            let mut policy: ClaimPolicy = strict_json(bytes.as_bytes())?;
            policy.max_entries += 1;
            policy.max_entry_bytes += 1;
            policy.max_bundle_bytes += 1;
            policy.max_api_requests += 1;
            return serde_json::to_string(&policy).map_err(|error| error.to_string());
        }
        Ok(bytes)
    }

    fn graphql(&self, query: &str) -> Result<Value, String> {
        self.0.graphql(query)
    }
}

impl ClaimClient for BudgetRevisionClient {
    fn create_commit(&self, input: &Value) -> Result<String, String> {
        self.0.create_commit(input)
    }
}

#[test]
fn owner_approved_budget_update_preserves_approved_historical_entries() {
    let api = Api::new();
    let original = store(&api, 1);
    original.lock(REPO).unwrap().release().unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut producer = run(2);
    producer.workflow_revision = MERGE.to_owned();
    let updated = GitHubClaimStore::new(
        BudgetRevisionClient(api.clone()),
        ProcessAttestation::new(
            root,
            PathBuf::from("/fake/node"),
            PathBuf::from("/fake/gh"),
            Processes(api.server.clone()),
        )
        .unwrap(),
        REPO,
        producer,
    )
    .unwrap();
    updated.lock(REPO).unwrap().release().unwrap();
    assert_eq!(api.server.lock().unwrap().mutations.len(), 4);
}

#[test]
fn release_timeout_irreversibly_closes_original_before_any_new_acquisition() {
    for commits in [false, true] {
        let api = Api::new();
        let owner = store(&api, 1);
        let mut held = owner.lock(REPO).unwrap();
        {
            let mut server = api.server.lock().unwrap();
            server.fail_mutation = Some((2, commits));
        }
        assert_eq!(held.release().is_ok(), commits);
        assert!(held.append(&pending(1)).is_err());
        assert!(held.release().is_err());
        assert!(
            acme::assignment::handoff(
                &Transport {
                    api: api.clone(),
                    calls: Mutex::new(0),
                    outcome: AssignmentOutcome::Accepted
                },
                &request(),
                &pending(1).attempt,
                &mut held
            )
            .is_err()
        );
        assert_eq!(store(&api, 2).lock(REPO).is_ok(), commits);
    }
}

#[test]
fn unknown_assignment_retains_ownership_and_permit_cannot_be_used_twice() {
    let api = Api::new();
    let owner = store(&api, 1);
    let mut held = owner.lock(REPO).unwrap();
    let mut receipt = pending(1);
    held.append(&receipt).unwrap();
    held.arm_assignment(&receipt.attempt).unwrap();
    let transport = Transport {
        api: api.clone(),
        calls: Mutex::new(0),
        outcome: AssignmentOutcome::Unknown,
    };
    assert_eq!(
        acme::assignment::handoff(&transport, &request(), &receipt.attempt, &mut held),
        Ok(AssignmentOutcome::Unknown)
    );
    receipt.state = ReceiptState::Unknown;
    held.append(&receipt).unwrap();
    assert!(held.release().is_err());
    assert!(
        acme::assignment::handoff(&transport, &request(), &receipt.attempt, &mut held).is_err()
    );
    assert_eq!(*transport.calls.lock().unwrap(), 1);
    assert!(store(&api, 2).lock(REPO).is_err());
}

#[test]
fn unknown_or_unreturned_reservation_cannot_be_settled_by_caller_receipts() {
    for reserve_only in [false, true] {
        for state in [ReceiptState::Accepted, ReceiptState::NotAssigned] {
            let api = Api::new();
            let owner = store(&api, 1);
            let mut held = owner.lock(REPO).unwrap();
            let mut receipt = pending(1);
            held.append(&receipt).unwrap();
            held.arm_assignment(&receipt.attempt).unwrap();
            if reserve_only {
                held.reserve_assignment(&receipt.attempt, &request())
                    .unwrap();
            } else {
                let transport = Transport {
                    api: api.clone(),
                    calls: Mutex::new(0),
                    outcome: AssignmentOutcome::Unknown,
                };
                assert_eq!(
                    acme::assignment::handoff(&transport, &request(), &receipt.attempt, &mut held),
                    Ok(AssignmentOutcome::Unknown)
                );
                assert_eq!(*transport.calls.lock().unwrap(), 1);
            }
            receipt.state = state;
            assert!(held.append(&receipt).is_err());
            assert!(held.release().is_err());
            drop(held);
            assert!(store(&api, 2).lock(REPO).is_err());
            assert_eq!(api.server.lock().unwrap().mutations.len(), 3);
        }
    }
}

#[test]
fn ownership_or_protection_loss_prevents_the_guarded_transport() {
    for protection in [false, true] {
        let api = Api::new();
        let owner = store(&api, 1);
        let mut held = owner.lock(REPO).unwrap();
        let receipt = pending(1);
        held.append(&receipt).unwrap();
        held.arm_assignment(&receipt.attempt).unwrap();
        {
            let mut server = api.server.lock().unwrap();
            if protection {
                server.protected = false;
            } else {
                server.head = GENESIS.to_owned();
            }
        }
        let transport = Transport {
            api: api.clone(),
            calls: Mutex::new(0),
            outcome: AssignmentOutcome::Accepted,
        };
        assert!(
            acme::assignment::handoff(&transport, &request(), &receipt.attempt, &mut held).is_err()
        );
        assert_eq!(*transport.calls.lock().unwrap(), 0);
        assert!(held.release().is_err());
    }
}

#[test]
fn incomplete_immutable_objects_and_tree_changes_never_become_an_empty_journal() {
    for fault in [
        "ancestor",
        "bundle",
        "truncated_tree",
        "truncated_blob",
        "mode",
        "extra_directory",
        "nondecimal_directory",
        "duplicate_directory",
        "delete",
    ] {
        let api = Api::new();
        let owner = store(&api, 1);
        let mut held = owner.lock(REPO).unwrap();
        held.release().unwrap();
        {
            let mut server = api.server.lock().unwrap();
            let revision = server.head.clone();
            let tree = server.objects[&format!("git/commits/{revision}")]["tree"]["sha"]
                .as_str()
                .unwrap()
                .to_owned();
            let files = server.snapshots[&revision].clone();
            let bundle = files
                .iter()
                .find(|(path, _)| path.ends_with(".sigstore.json"))
                .unwrap()
                .1
                .clone();
            match fault {
                "ancestor" => {
                    server.objects.remove(&format!("git/commits/{GENESIS}"));
                }
                "bundle" => {
                    server.objects.remove(&format!("git/blobs/{bundle}"));
                }
                "truncated_tree" => {
                    server
                        .objects
                        .get_mut(&format!("git/trees/{tree}?recursive=1"))
                        .unwrap()["truncated"] = json!(true);
                }
                "truncated_blob" => {
                    server
                        .objects
                        .get_mut(&format!("git/blobs/{bundle}"))
                        .unwrap()["size"] = json!(1);
                }
                "mode" => {
                    let rows = server
                        .objects
                        .get_mut(&format!("git/trees/{tree}?recursive=1"))
                        .unwrap()["tree"]
                        .as_array_mut()
                        .unwrap();
                    rows.iter_mut().find(|row| row["type"] == "blob").unwrap()["mode"] =
                        json!("100755");
                }
                "extra_directory" | "nondecimal_directory" | "duplicate_directory" => {
                    corrupt_directory(&mut server, &tree, fault);
                }
                "delete" => {
                    server
                        .objects
                        .get_mut(&format!("git/trees/{tree}?recursive=1"))
                        .unwrap()["tree"]
                        .as_array_mut()
                        .unwrap()
                        .retain(|row| row["path"] != "journal/genesis.json");
                }
                _ => unreachable!(),
            }
            drop(server);
        }
        let before = api.server.lock().unwrap().mutations.len();
        assert!(store(&api, 2).lock(REPO).is_err(), "{fault}");
        assert_eq!(
            api.server.lock().unwrap().mutations.len(),
            before,
            "{fault}"
        );
    }
}

#[test]
fn missing_approval_pages_and_each_owner_budget_fail_closed() {
    let api = Api::new();
    {
        let mut server = api.server.lock().unwrap();
        server.pagination.files = true;
        server.read_fault = Some("files?per_page=100&page=2".to_owned());
    }
    assert!(store(&api, 1).lock(REPO).is_err());
    for field in [
        "max_entries",
        "max_entry_bytes",
        "max_bundle_bytes",
        "max_api_requests",
    ] {
        let api = Api::new();
        let mut values = serde_json::to_value(policy()).unwrap();
        values[field] = json!(0);
        api.server.lock().unwrap().policy = values.to_string();
        assert!(store(&api, 1).lock(REPO).is_err(), "{field}");
        assert!(api.server.lock().unwrap().mutations.is_empty());
    }
    let api = Api::new();
    let mut values = serde_json::to_value(policy()).unwrap();
    values["max_api_requests"] = json!(5);
    api.server.lock().unwrap().policy = values.to_string();
    assert!(store(&api, 1).lock(REPO).is_err());
    assert!(api.server.lock().unwrap().mutations.is_empty());
    assert_eq!(api.server.lock().unwrap().reads.len(), 5);
}

#[test]
fn duplicate_or_unknown_policy_fields_and_operation_fields_are_rejected() {
    let api = Api::new();
    let policy = serde_json::to_string(&policy()).unwrap();
    api.server.lock().unwrap().policy = policy.replacen('{', "{\"version\":1,", 1);
    assert!(store(&api, 1).lock(REPO).is_err());
    for extra in [
        json!({"kind":"acquire","unexpected":true}),
        json!({"kind":"acquire","generation":"forged"}),
    ] {
        assert!(serde_json::from_value::<Operation>(extra).is_err());
    }
}

#[test]
fn unresolved_arm_or_outcome_write_cannot_be_hidden_by_release() {
    for number in [3, 4] {
        let api = Api::new();
        let owner = store(&api, 1);
        let mut held = owner.lock(REPO).unwrap();
        let mut receipt = pending(1);
        held.append(&receipt).unwrap();
        api.server.lock().unwrap().fail_mutation = Some((number, false));
        if number == 3 {
            assert!(held.arm_assignment(&receipt.attempt).is_err());
        } else {
            held.arm_assignment(&receipt.attempt).unwrap();
            let transport = Transport {
                api: api.clone(),
                calls: Mutex::new(0),
                outcome: AssignmentOutcome::Accepted,
            };
            assert_eq!(
                acme::assignment::handoff(&transport, &request(), &receipt.attempt, &mut held),
                Ok(AssignmentOutcome::Accepted)
            );
            receipt.state = ReceiptState::Accepted;
            assert!(held.append(&receipt).is_err());
        }
        assert!(held.release().is_err());
        assert!(store(&api, 2).lock(REPO).is_err());
        drop(held);
        assert!(store(&api, 3).lock(REPO).is_err());
        assert_eq!(api.server.lock().unwrap().mutations.len(), number);
    }
}

fn rewrite_entry(server: &mut Server, sequence: usize, edit: impl FnOnce(&mut Value)) {
    let files = server.snapshots[&server.head].clone();
    let path = format!("journal/entries/{sequence:020}");
    let blob = &files[&format!("{path}/entry.json")];
    let mut entry: Value = strict_json(&unb64(
        server.objects[&format!("git/blobs/{blob}")]["content"]
            .as_str()
            .unwrap(),
    ))
    .unwrap();
    edit(&mut entry);
    let mut bytes = serde_json::to_vec(&entry).unwrap();
    bytes.push(b'\n');
    let saved = server
        .objects
        .get_mut(&format!("git/blobs/{blob}"))
        .unwrap();
    saved["content"] = json!(b64(&bytes));
    saved["size"] = json!(bytes.len());
    let bundle_blob = &files[&format!("{path}/entry.sigstore.json")];
    let mut bundle: Value = strict_json(&unb64(
        server.objects[&format!("git/blobs/{bundle_blob}")]["content"]
            .as_str()
            .unwrap(),
    ))
    .unwrap();
    bundle["entry_sha256"] = json!(sha(&bytes));
    let bytes = serde_json::to_vec(&bundle).unwrap();
    let saved = server
        .objects
        .get_mut(&format!("git/blobs/{bundle_blob}"))
        .unwrap();
    saved["content"] = json!(b64(&bytes));
    saved["size"] = json!(bytes.len());
}

#[test]
fn valid_signatures_cannot_authorize_duplicate_operations_or_invalid_ownership_history() {
    for fault in [
        "operation",
        "sequence",
        "parent",
        "nonce",
        "generation",
        "unknown",
    ] {
        let api = Api::new();
        let owner = store(&api, 1);
        let mut held = owner.lock(REPO).unwrap();
        held.release().unwrap();
        {
            let mut server = api.server.lock().unwrap();
            let first_id = server.entries()[0].operation_id.clone();
            rewrite_entry(&mut server, 2, |entry| match fault {
                "operation" => entry["operation_id"] = json!(first_id),
                "sequence" => entry["sequence"] = json!(1),
                "parent" => entry["expected_parent"] = json!(MAIN),
                "nonce" => entry["session_nonce"] = json!("d".repeat(64)),
                "generation" => entry["operation"]["generation"] = json!("e".repeat(64)),
                "unknown" => entry["operation"]["extra"] = json!(true),
                _ => unreachable!(),
            });
            drop(server);
        }
        assert!(store(&api, 2).lock(REPO).is_err(), "{fault}");
        assert_eq!(api.server.lock().unwrap().mutations.len(), 2);
    }
}

#[test]
fn signatures_bind_exact_bytes_not_reserialized_json() {
    let api = Api::new();
    let owner = store(&api, 1);
    let mut held = owner.lock(REPO).unwrap();
    held.release().unwrap();
    {
        let mut server = api.server.lock().unwrap();
        let files = server.snapshots[&server.head].clone();
        let entry = files
            .iter()
            .find(|(path, _)| path.ends_with("/entry.json"))
            .unwrap()
            .1
            .clone();
        let saved = server
            .objects
            .get_mut(&format!("git/blobs/{entry}"))
            .unwrap();
        let mut bytes = unb64(saved["content"].as_str().unwrap());
        bytes.insert(1, b' ');
        saved["content"] = json!(b64(&bytes));
        saved["size"] = json!(bytes.len());
        drop(server);
    }
    assert!(store(&api, 2).lock(REPO).is_err());
}
struct StateReader {
    api: Api,
    reads: Mutex<usize>,
    change: Option<&'static str>,
}

impl acme::claims::GitHubState for StateReader {
    fn observe(&self, request: &AssignmentRequest) -> Result<acme::claims::GitHubSnapshot, String> {
        let mut reads = self.reads.lock().unwrap();
        *reads += 1;
        let final_read = *reads == 2;
        drop(reads);
        let declaration = request.declaration();
        let mut snapshot = acme::claims::GitHubSnapshot {
            repository: REPO.to_owned(),
            main_revision: MAIN.to_owned(),
            owner_login: "owner".to_owned(),
            authorized_approvers: vec!["owner".to_owned()],
            config: acme::CoordinatorConfig {
                automatic_dispatch: true,
                max_active_tasks: 1,
                max_assignments_per_reconciliation: 2,
                max_attempts: 2,
            },
            kill_switches: acme::KillSwitches {
                global: acme::Evidence::Known(false),
                coordinator: acme::Evidence::Known(false),
                assignment: acme::Evidence::Known(false),
            },
            contracts: vec![request.approved_contract().to_owned()],
            observations: vec![acme::TaskObservation {
                issue: declaration.issue,
                current_spec: acme::Evidence::Known("spec".to_owned()),
                current_issue_body: acme::Evidence::Known("approved".to_owned()),
                issue_state: acme::Evidence::Known(acme::IssueState::Open),
                approval: acme::Evidence::Known(Some(acme::ApprovalEvidence {
                    repository: REPO.to_owned(),
                    issue: declaration.issue,
                    spec_path: declaration.spec.path.clone(),
                    spec_sha256: declaration.spec.sha256.clone(),
                    issue_body_sha256: declaration.issue_body.sha256.clone(),
                    kind: declaration.kind.clone(),
                    prerequisites: vec![],
                    spec_pull_request: 1,
                    merged_by: "owner".to_owned(),
                    main_revision: MAIN.to_owned(),
                    present_on_protected_main: true,
                })),
                native_prerequisites: acme::Evidence::Known(vec![]),
                existing_work: acme::Evidence::Known(acme::ExistingWork::default()),
                completion: acme::Evidence::Known(None),
            }],
            work: acme::Evidence::Known(vec![]),
            resolutions: acme::Evidence::Unknown,
            owner_ready: acme::Evidence::Known(Some(acme::claims::OwnerReady {
                repository: REPO.to_owned(),
                issue: declaration.issue,
                applied_by: "owner".to_owned(),
                label_present: true,
            })),
        };
        if matches!(self.change, Some("existing_work" | "initial_capacity")) {
            snapshot.work = acme::Evidence::Known(vec![acme::claims::ActualWork {
                issue: if self.change == Some("existing_work") {
                    17
                } else {
                    99
                },
                active_copilot: 1,
                open_implementation_prs: 0,
            }]);
        }
        if self.change == Some("initial_scope") {
            snapshot.observations[0].current_issue_body =
                acme::Evidence::Known("changed".to_owned());
        }
        if final_read {
            match self.change {
                Some("scope") => {
                    snapshot.observations[0].current_issue_body =
                        acme::Evidence::Known("changed".to_owned());
                }
                Some("gate") => snapshot.kill_switches.assignment = acme::Evidence::Known(true),
                Some("protection") => self.api.server.lock().unwrap().protected = false,
                Some("capacity") => {
                    snapshot.work = acme::Evidence::Known(vec![acme::claims::ActualWork {
                        issue: 99,
                        active_copilot: 1,
                        open_implementation_prs: 0,
                    }]);
                }
                _ => {}
            }
        }
        Ok(snapshot)
    }
}

fn reconcile_context(id: u64) -> acme::claims::ReconcileContext {
    acme::claims::ReconcileContext {
        repository: REPO.to_owned(),
        trusted_workflow: WORKFLOW.to_owned(),
        trusted_writer: format!("https://github.com/{REPO}/{WORKFLOW}@refs/heads/main"),
        run: run(id),
    }
}

#[test]
fn actual_reconciliation_uses_the_guarded_store_for_both_routes_and_settled_release() {
    for route in [Route::Automatic, Route::Manual] {
        let api = Api::new();
        let owner = store(&api, 1);
        let state = StateReader {
            api: api.clone(),
            reads: Mutex::new(0),
            change: None,
        };
        let transport = Transport {
            api: api.clone(),
            calls: Mutex::new(0),
            outcome: AssignmentOutcome::Accepted,
        };
        assert_eq!(
            acme::claims::reconcile(
                &owner,
                &state,
                &transport,
                &reconcile_context(1),
                &request(),
                route
            ),
            acme::claims::ReconcileResult::Assigned
        );
        assert_eq!(*transport.calls.lock().unwrap(), 1);
        let entries = api.server.lock().unwrap().entries();
        assert!(
            matches!(entries[3].operation, Operation::Receipt { ref receipt, .. }
            if receipt.state == ReceiptState::Accepted)
        );
        assert!(matches!(entries[4].operation, Operation::Release { .. }));
        assert!(store(&api, 2).lock(REPO).is_ok());
    }
}

#[test]
fn initial_scope_or_capacity_block_releases_without_receipts_or_assignment() {
    for change in ["initial_scope", "initial_capacity"] {
        let api = Api::new();
        let owner = store(&api, 1);
        let state = StateReader {
            api: api.clone(),
            reads: Mutex::new(0),
            change: Some(change),
        };
        let transport = Transport {
            api: api.clone(),
            calls: Mutex::new(0),
            outcome: AssignmentOutcome::Accepted,
        };
        let result = acme::claims::reconcile(
            &owner,
            &state,
            &transport,
            &reconcile_context(1),
            &request(),
            Route::Automatic,
        );
        if change == "initial_capacity" {
            assert_eq!(
                result,
                acme::claims::ReconcileResult::Blocked(acme::claims::ClaimBlock::CapacityExhausted)
            );
        } else {
            assert!(matches!(result, acme::claims::ReconcileResult::Blocked(_)));
        }
        assert_eq!(*transport.calls.lock().unwrap(), 0);
        let entries = api.server.lock().unwrap().entries();
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0].operation, Operation::Acquire {}));
        assert!(matches!(entries[1].operation, Operation::Release { .. }));
        assert!(owner.lock(REPO).is_err());
        store(&api, 2).lock(REPO).unwrap().release().unwrap();
    }
}

#[test]
fn uncertain_pending_write_preserves_operation_error_without_cleanup_release() {
    let api = Api::new();
    api.server.lock().unwrap().fail_mutation = Some((2, false));
    let owner = store(&api, 1);
    let state = StateReader {
        api: api.clone(),
        reads: Mutex::new(0),
        change: None,
    };
    let transport = Transport {
        api: api.clone(),
        calls: Mutex::new(0),
        outcome: AssignmentOutcome::Accepted,
    };
    let result = acme::claims::reconcile(
        &owner,
        &state,
        &transport,
        &reconcile_context(1),
        &request(),
        Route::Automatic,
    );
    let acme::claims::ReconcileResult::Blocked(acme::claims::ClaimBlock::Store(error)) = result
    else {
        panic!("expected the original unresolved store operation");
    };
    assert!(error.starts_with("unresolved journal operation "));
    assert!(error.contains("retain ownership"));
    assert_eq!(*transport.calls.lock().unwrap(), 0);
    assert_eq!(api.server.lock().unwrap().mutations.len(), 2);
    assert_eq!(api.server.lock().unwrap().entries().len(), 1);
    assert!(store(&api, 2).lock(REPO).is_err());
}

#[test]
fn final_scope_gate_or_capacity_change_records_no_call_before_settled_release() {
    for change in ["scope", "gate", "capacity"] {
        let api = Api::new();
        let owner = store(&api, 1);
        let state = StateReader {
            api: api.clone(),
            reads: Mutex::new(0),
            change: Some(change),
        };
        let transport = Transport {
            api: api.clone(),
            calls: Mutex::new(0),
            outcome: AssignmentOutcome::Accepted,
        };
        assert!(matches!(
            acme::claims::reconcile(
                &owner,
                &state,
                &transport,
                &reconcile_context(1),
                &request(),
                Route::Automatic
            ),
            acme::claims::ReconcileResult::Blocked(_)
        ));
        assert_eq!(*transport.calls.lock().unwrap(), 0, "{change}");
        let entries = api.server.lock().unwrap().entries();
        assert!(
            matches!(entries[3].operation, Operation::Receipt { ref receipt, .. }
            if receipt.state == ReceiptState::NotAssigned)
        );
        assert!(matches!(entries[4].operation, Operation::Release { .. }));
        assert!(store(&api, 2).lock(REPO).is_ok(), "{change}");
    }
}

#[test]
fn production_reconciliation_unknown_outcome_and_protection_loss_retain_ownership() {
    for change in [None, Some("protection")] {
        let api = Api::new();
        let owner = store(&api, 1);
        let state = StateReader {
            api: api.clone(),
            reads: Mutex::new(0),
            change,
        };
        let transport = Transport {
            api: api.clone(),
            calls: Mutex::new(0),
            outcome: AssignmentOutcome::Unknown,
        };
        let result = acme::claims::reconcile(
            &owner,
            &state,
            &transport,
            &reconcile_context(1),
            &request(),
            Route::Manual,
        );
        if change.is_none() {
            assert_eq!(result, acme::claims::ReconcileResult::Uncertain);
            assert_eq!(*transport.calls.lock().unwrap(), 1);
        } else {
            assert!(matches!(result, acme::claims::ReconcileResult::Blocked(_)));
            assert_eq!(*transport.calls.lock().unwrap(), 0);
        }
        api.server.lock().unwrap().protected = true;
        assert!(store(&api, 2).lock(REPO).is_err());
        assert!(
            !api.server
                .lock()
                .unwrap()
                .entries()
                .iter()
                .any(|entry| matches!(entry.operation, Operation::Release { .. }))
        );
    }
}

#[test]
fn accepted_work_counts_against_another_issue_and_preflight_capacity_blocks_release_safely() {
    let api = Api::new();
    let owner = store(&api, 1);
    let state = StateReader {
        api: api.clone(),
        reads: Mutex::new(0),
        change: None,
    };
    let transport = Transport {
        api: api.clone(),
        calls: Mutex::new(0),
        outcome: AssignmentOutcome::Accepted,
    };
    assert_eq!(
        acme::claims::reconcile(
            &owner,
            &state,
            &transport,
            &reconcile_context(1),
            &request(),
            Route::Automatic
        ),
        acme::claims::ReconcileResult::Assigned
    );
    let next_request = AssignmentRequest::from_approved_snapshot(
        &request()
            .approved_contract()
            .replace("issue = 17", "issue = 18"),
        MAIN,
    )
    .unwrap();
    let owner = store(&api, 2);
    let state = StateReader {
        api: api.clone(),
        reads: Mutex::new(0),
        change: Some("existing_work"),
    };
    assert!(matches!(
        acme::claims::reconcile(
            &owner,
            &state,
            &transport,
            &reconcile_context(2),
            &next_request,
            Route::Automatic
        ),
        acme::claims::ReconcileResult::Blocked(_)
    ));
    assert_eq!(*transport.calls.lock().unwrap(), 1);
    let entries = api.server.lock().unwrap().entries();
    assert!(matches!(
        entries.last().unwrap().operation,
        Operation::Release { .. }
    ));
    assert_eq!(
        entries
            .iter()
            .filter(|entry| matches!(entry.operation, Operation::ArmAssignment { .. }))
            .count(),
        1
    );
    assert!(store(&api, 3).lock(REPO).is_ok());
}

#[test]
fn another_producer_cannot_assign_while_the_original_synchronous_handler_is_alive() {
    struct BlockingTransport {
        entered: std::sync::mpsc::Sender<()>,
        finish: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl AssignmentTransport for BlockingTransport {
        fn capabilities(&self) -> AssignmentCapabilities {
            AssignmentCapabilities::gh_aw_0_93_5()
        }
        fn assign(&self, permit: AssignmentPermit<'_>) -> AssignmentOutcome {
            assert_eq!(permit.request(), &request());
            self.entered.send(()).unwrap();
            self.finish.lock().unwrap().recv().unwrap();
            AssignmentOutcome::Unknown
        }
    }
    let api = Api::new();
    let first = api.clone();
    let (entered, observed) = std::sync::mpsc::channel();
    let (finish, release_handler) = std::sync::mpsc::channel();
    let producer = std::thread::spawn(move || {
        let owner = store(&first, 1);
        let state = StateReader {
            api: first,
            reads: Mutex::new(0),
            change: None,
        };
        acme::claims::reconcile(
            &owner,
            &state,
            &BlockingTransport {
                entered,
                finish: Mutex::new(release_handler),
            },
            &reconcile_context(1),
            &request(),
            Route::Manual,
        )
    });
    observed
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let contender = store(&api, 2);
    let state = StateReader {
        api: api.clone(),
        reads: Mutex::new(0),
        change: None,
    };
    let transport = Transport {
        api: api.clone(),
        calls: Mutex::new(0),
        outcome: AssignmentOutcome::Accepted,
    };
    assert!(matches!(
        acme::claims::reconcile(
            &contender,
            &state,
            &transport,
            &reconcile_context(2),
            &request(),
            Route::Automatic
        ),
        acme::claims::ReconcileResult::Blocked(_)
    ));
    assert_eq!(*transport.calls.lock().unwrap(), 0);
    finish.send(()).unwrap();
    assert_eq!(
        producer.join().unwrap(),
        acme::claims::ReconcileResult::Uncertain
    );
    assert!(store(&api, 3).lock(REPO).is_err());
}

#[test]
fn a_reserved_call_is_consumed_once_and_unknown_work_cannot_be_settled_locally() {
    for outcome in [AssignmentOutcome::Accepted, AssignmentOutcome::Unknown] {
        let api = Api::new();
        let owner = store(&api, 1);
        let mut held = owner.lock(REPO).unwrap();
        let mut receipt = pending(1);
        held.append(&receipt).unwrap();
        held.arm_assignment(&receipt.attempt).unwrap();
        let transport = Transport {
            api: api.clone(),
            calls: Mutex::new(0),
            outcome: outcome.clone(),
        };
        assert_eq!(
            acme::assignment::handoff(&transport, &request(), &receipt.attempt, &mut held),
            Ok(outcome.clone())
        );
        if outcome == AssignmentOutcome::Unknown {
            receipt.state = ReceiptState::Unknown;
            held.append(&receipt).unwrap();
            receipt.state = ReceiptState::Accepted;
            assert!(held.append(&receipt).is_err());
        }
        assert!(
            acme::assignment::handoff(&transport, &request(), &receipt.attempt, &mut held).is_err()
        );
        assert_eq!(*transport.calls.lock().unwrap(), 1);
        assert!(held.release().is_err());
        assert!(store(&api, 2).lock(REPO).is_err());
    }
}

#[test]
fn policy_and_workflow_approval_require_exact_bytes_not_normalized_hashes() {
    for path in [POLICY_PATH, WORKFLOW] {
        let api = Api::new();
        {
            let mut server = api.server.lock().unwrap();
            server.policy.push('\n');
            server.changed_approval_bytes = Some(path);
            drop(server);
        }
        let result = store(&api, 1).lock(REPO).err().unwrap();
        assert!(
            result.contains("exact owner-approved bytes"),
            "{path}: {result}"
        );
        assert!(api.server.lock().unwrap().mutations.is_empty());
    }
}

#[test]
fn malformed_candidate_receipts_are_rejected_before_any_journal_mutation() {
    for fault in [
        "attempt",
        "version",
        "snapshot",
        "outcome",
        "route",
        "recovery",
        "completion",
    ] {
        let api = Api::new();
        let owner = store(&api, 1);
        let mut held = owner.lock(REPO).unwrap();
        let mut receipt = pending(1);
        match fault {
            "attempt" => receipt.attempt.number = 9,
            "version" => receipt.version = 1,
            "snapshot" => receipt.attempt.binding.approved_contract = "substituted".to_owned(),
            "outcome" => receipt.state = ReceiptState::Accepted,
            "route" | "recovery" | "completion" => {
                held.append(&receipt).unwrap();
                receipt.state = ReceiptState::NotAssigned;
                if fault == "route" {
                    receipt.route = Route::Manual;
                } else if fault == "recovery" {
                    receipt.recovery = Some(acme::claims::AttemptResolution {
                        attempt: receipt.attempt.clone(),
                        outcome: acme::claims::RecoveryOutcome::NeverAssigned,
                    });
                } else {
                    held.append(&receipt).unwrap();
                    receipt.state = ReceiptState::Completed;
                    held.append(&receipt).unwrap();
                }
            }
            _ => unreachable!(),
        }
        let before = api.server.lock().unwrap().mutations.len();
        assert!(held.append(&receipt).is_err(), "{fault}");
        assert_eq!(
            api.server.lock().unwrap().mutations.len(),
            before,
            "{fault}"
        );
        assert!(held.release().is_err(), "{fault}");
        assert!(store(&api, 2).lock(REPO).is_err(), "{fault}");
    }
}

#[test]
fn authenticated_receipt_history_is_fully_validated_before_acquisition() {
    for fault in ["attempt", "route", "recovery"] {
        let api = Api::new();
        let owner = store(&api, 1);
        let mut held = owner.lock(REPO).unwrap();
        let mut receipt = pending(1);
        held.append(&receipt).unwrap();
        receipt.state = ReceiptState::NotAssigned;
        held.append(&receipt).unwrap();
        held.release().unwrap();
        let mut server = api.server.lock().unwrap();
        if fault == "attempt" {
            for sequence in [2, 3] {
                rewrite_entry(&mut server, sequence, |entry| {
                    entry["operation"]["receipt"]["attempt"]["number"] = json!(9);
                });
            }
        } else {
            rewrite_entry(&mut server, 3, |entry| {
                if fault == "route" {
                    entry["operation"]["receipt"]["route"] = json!("manual");
                } else {
                    entry["operation"]["receipt"]["recovery"] =
                        serde_json::to_value(acme::claims::AttemptResolution {
                            attempt: pending(1).attempt,
                            outcome: acme::claims::RecoveryOutcome::NeverAssigned,
                        })
                        .unwrap();
                }
            });
        }
        let before = server.mutations.len();
        drop(server);
        let error = store(&api, 2).lock(REPO).err().unwrap();
        assert!(
            error.contains("invalid receipt journal"),
            "{fault}: {error}"
        );
        assert_eq!(
            api.server.lock().unwrap().mutations.len(),
            before,
            "{fault}"
        );
    }
}

#[test]
fn strict_json_rejects_duplicate_keys_at_every_depth() {
    for input in [
        r#"{"version":1,"version":1}"#,
        r#"{"operation":{"kind":"acquire","kind":"release"}}"#,
        r#"{"items":[{"id":1,"id":2}]}"#,
    ] {
        assert!(strict_json::<serde_json::Value>(input.as_bytes()).is_err());
    }
    assert!(strict_json::<serde_json::Value>(br#"{"items":[{"id":1}]}"#).is_ok());
}
