use crate::Evidence;
use crate::assignment::AssignmentRequest;
use crate::claims::{
    AttemptId, ClaimSession, ClaimStore, Receipt, ReceiptProvenance, ReceiptState, RequestBinding,
    StoredReceipt, WorkflowRun,
};
use crate::github_state::{GitHubClient, GitHubStateReader};
use serde::de::{DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

pub mod process;

pub const JOURNAL_REF: &str = "refs/heads/acme-claims";
pub const WORKFLOW: &str = ".github/workflows/dispatch-claims.yml";
pub const POLICY_PATH: &str = "factory/dispatch/claim-store.json";
pub const PREDICATE: &str = "https://github.com/tylerbutler/acme/attestations/claim-entry/v1";

/// Reject duplicate keys before typed deserialization, including keys in nested objects.
pub fn strict_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    struct Unique;
    impl<'de> Visitor<'de> for Unique {
        type Value = Value;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("JSON without duplicate keys")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Value, M::Error> {
            let mut values = serde_json::Map::new();
            while let Some((key, value)) = map.next_entry::<String, UniqueValue>()? {
                if values.insert(key, value.0).is_some() {
                    return Err(serde::de::Error::custom("duplicate JSON key"));
                }
            }
            Ok(Value::Object(values))
        }
        fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Value, S::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element::<UniqueValue>()? {
                values.push(value.0);
            }
            Ok(Value::Array(values))
        }
        fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Value, E> {
            Ok(Value::Bool(value))
        }
        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Value, E> {
            Ok(value.into())
        }
        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Value, E> {
            Ok(value.into())
        }
        fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Value, E> {
            serde_json::Number::from_f64(value)
                .map(Value::Number)
                .ok_or_else(|| E::custom("invalid number"))
        }
        fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Value, E> {
            Ok(Value::String(value.to_owned()))
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
            Ok(Value::Null)
        }
    }
    struct UniqueValue(Value);
    impl<'de> Deserialize<'de> for UniqueValue {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            deserializer.deserialize_any(Unique).map(Self)
        }
    }
    let value: UniqueValue = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    serde_json::from_value(value.0).map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimPolicy {
    pub version: u32,
    pub repository: String,
    pub repository_id: u64,
    pub owner_id: u64,
    pub journal_ref: String,
    pub genesis_oid: String,
    pub workflow_path: String,
    pub max_entries: usize,
    pub max_entry_bytes: usize,
    pub max_bundle_bytes: usize,
    pub max_api_requests: usize,
}

impl ClaimPolicy {
    fn validate(&self, repository: &str) -> Result<(), String> {
        if self.version != 1
            || self.repository != repository
            || self.repository_id == 0
            || self.owner_id == 0
            || self.journal_ref != JOURNAL_REF
            || self.workflow_path != WORKFLOW
            || !oid(&self.genesis_oid)
            || self.max_entries == 0
            || self.max_entry_bytes == 0
            || self.max_bundle_bytes == 0
            || self.max_api_requests == 0
        {
            return Err("unsupported or incomplete claim-store policy".to_owned());
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Genesis {
    version: u32,
    repository: String,
    repository_id: u64,
    owner_id: u64,
    journal_ref: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub version: u32,
    pub sequence: usize,
    pub expected_parent: String,
    pub repository: String,
    pub repository_id: u64,
    pub owner_id: u64,
    pub operation_id: String,
    pub session_nonce: String,
    pub recorded_by: WorkflowRun,
    pub operation: Operation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Acquire {},
    Receipt {
        generation: String,
        receipt: Box<Receipt>,
    },
    ArmAssignment {
        generation: String,
        attempt: AttemptId,
    },
    Release {
        generation: String,
    },
}

pub trait ClaimClient: GitHubClient {
    /// Submit one mutation. Never retry a mutation after an uncertain response.
    fn create_commit(&self, input: &Value) -> Result<String, String>;
}

pub trait Attestation {
    fn sign(&self, entry: &[u8], policy: &ClaimPolicy) -> Result<Vec<u8>, String>;
    fn verify(
        &self,
        entry: &[u8],
        bundle: &[u8],
        policy: &ClaimPolicy,
        run: &WorkflowRun,
    ) -> Result<VerifiedCertificate, String>;
}

#[derive(Clone, Debug)]
pub struct VerifiedCertificate {
    pub issuer: String,
    pub subject_alternative_name: String,
    pub repository_id: String,
    pub owner_id: String,
    pub owner_uri: String,
    pub repository_uri: String,
    pub source_ref: String,
    pub source_digest: String,
    pub signer_uri: String,
    pub signer_digest: String,
    pub build_config_uri: String,
    pub build_config_digest: String,
    pub runner_environment: String,
    pub invocation_uri: String,
}

pub struct BudgetClient<'a, C> {
    client: &'a C,
    remaining: &'a Cell<usize>,
}

impl<C> BudgetClient<'_, C> {
    fn charge(&self) -> Result<(), String> {
        let remaining = self
            .remaining
            .get()
            .checked_sub(1)
            .ok_or("claim-store API request budget exhausted")?;
        self.remaining.set(remaining);
        Ok(())
    }
}

impl<C: GitHubClient> GitHubClient for BudgetClient<'_, C> {
    fn get(&self, endpoint: &str) -> Result<Value, String> {
        self.charge()?;
        self.client.get(endpoint)
    }
    fn file(&self, repository: &str, path: &str, revision: &str) -> Result<String, String> {
        self.charge()?;
        self.client.file(repository, path, revision)
    }
    fn graphql(&self, query: &str) -> Result<Value, String> {
        self.charge()?;
        self.client.graphql(query)
    }
}

pub struct GitHubClaimStore<C, A> {
    client: C,
    attestation: A,
    repository: String,
    run: WorkflowRun,
    nonce: String,
    remaining: Cell<usize>,
    used: Cell<bool>,
}

impl GitHubClaimStore<process::GhJobClient, process::ProcessAttestation> {
    pub fn production(
        root: std::path::PathBuf,
        node: std::path::PathBuf,
        gh: std::path::PathBuf,
        repository: &str,
        run: WorkflowRun,
    ) -> Result<Self, String> {
        let client = process::GhJobClient::new(repository, gh.clone(), process::SystemProcess)?;
        let attestation = process::ProcessAttestation::new(root, node, gh, process::SystemProcess)?;
        Self::new(client, attestation, repository, run)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Owner {
    generation: String,
    nonce: String,
    run: WorkflowRun,
}

struct Loaded {
    head: String,
    sequence: usize,
    owner: Option<Owner>,
    receipts: Vec<StoredReceipt>,
    entries: Vec<(Entry, Vec<u8>, Vec<u8>, String)>,
    pending: Option<AttemptId>,
    armed: Option<AttemptId>,
    arms: BTreeSet<String>,
    uncertain: bool,
}

struct CommitSnapshot {
    oid: String,
    parent: String,
    files: BTreeMap<String, String>,
}

impl<C: ClaimClient, A: Attestation> GitHubClaimStore<C, A> {
    pub fn new(
        client: C,
        attestation: A,
        repository: &str,
        run: WorkflowRun,
    ) -> Result<Self, String> {
        if repository.split('/').count() != 2
            || repository.split('/').any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            })
            || run.run_id == 0
            || run.run_attempt == 0
            || run.workflow_path != WORKFLOW
            || !oid(&run.workflow_revision)
        {
            return Err("unsupported claim producer identity".to_owned());
        }
        let mut random = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut random))
            .map_err(|_| "cannot create a fresh producer nonce")?;
        let nonce = hex(&random);
        Ok(Self {
            client,
            attestation,
            repository: repository.to_owned(),
            run,
            nonce,
            remaining: Cell::new(0),
            used: Cell::new(false),
        })
    }

    const fn api(&self) -> BudgetClient<'_, C> {
        BudgetClient {
            client: &self.client,
            remaining: &self.remaining,
        }
    }

    fn policy(&self, revision: &str) -> Result<ClaimPolicy, String> {
        let api = self.api();
        let prefix = format!("repos/{}", self.repository);
        let metadata = api.get(&prefix)?;
        let owner = text(&metadata, "/owner/login")?;
        if metadata["full_name"] != self.repository
            || metadata["owner"]["type"] != "User"
            || metadata["default_branch"] != "main"
            || metadata["private"] != false
            || metadata["visibility"] != "public"
        {
            return Err("unsupported claim-store repository".to_owned());
        }
        let reader = GitHubStateReader::new(self.api());
        let main = reader.branch(&prefix)?;
        reader.ancestor(&prefix, revision, &main)?;
        let bytes = api.file(&self.repository, POLICY_PATH, revision)?;
        let policy: ClaimPolicy = strict_json(bytes.as_bytes())?;
        policy.validate(&self.repository)?;
        if metadata["id"].as_u64() != Some(policy.repository_id)
            || metadata["owner"]["id"].as_u64() != Some(policy.owner_id)
        {
            return Err("claim-store repository identity changed".to_owned());
        }
        let approval =
            reader.file_approval(&self.repository, POLICY_PATH, &bytes, owner, revision)?;
        let approved = api.file(
            &self.repository,
            POLICY_PATH,
            text(&approval, "/merge_commit_sha")?,
        )?;
        if approved.as_bytes() != bytes.as_bytes() {
            return Err("claim policy differs from its exact owner-approved bytes".to_owned());
        }
        if reader.branch(&prefix)? != main {
            return Err("protected main moved during policy verification".to_owned());
        }
        Ok(policy)
    }

    fn protections(&self, policy: &ClaimPolicy) -> Result<(), String> {
        let api = self.api();
        let prefix = format!("repos/{}", self.repository);
        let repository = api.get(&prefix)?;
        if repository["id"].as_u64() != Some(policy.repository_id)
            || repository["owner"]["id"].as_u64() != Some(policy.owner_id)
            || repository["full_name"] != self.repository
            || repository["owner"]["type"] != "User"
            || repository["private"] != false
            || repository["visibility"] != "public"
        {
            return Err("repository identity does not match claim policy".to_owned());
        }
        let mut sources = BTreeSet::new();
        let mut required = BTreeSet::new();
        for page in 1.. {
            let rules = api.get(&format!(
                "{prefix}/rules/branches/acme-claims?per_page=100&page={page}"
            ))?;
            let rows = rules.as_array().ok_or("missing effective branch rules")?;
            for rule in rows {
                let kind = text(rule, "/type")?;
                if matches!(
                    kind,
                    "deletion" | "non_fast_forward" | "required_linear_history"
                ) {
                    required.insert(kind.to_owned());
                    let id = rule["ruleset_id"]
                        .as_u64()
                        .filter(|id| *id > 0)
                        .ok_or("effective permanence rule has no ruleset")?;
                    sources.insert((
                        id,
                        kind.to_owned(),
                        text(rule, "/ruleset_source_type")?.to_owned(),
                        text(rule, "/ruleset_source")?.to_owned(),
                    ));
                }
            }
            if rows.len() < 100 {
                break;
            }
        }
        if required.len() != 3 {
            return Err("journal permanence rules are incomplete".to_owned());
        }
        for (id, rule_kind, kind, source) in sources {
            if kind != "Repository" || source != self.repository {
                return Err("unsupported journal ruleset source".to_owned());
            }
            let ruleset = api.get(&format!("{prefix}/rulesets/{id}"))?;
            if ruleset["id"] != id
                || ruleset["enforcement"] != "active"
                || ruleset["target"] != "branch"
                || ruleset["bypass_actors"]
                    .as_array()
                    .is_none_or(|actors| !actors.is_empty())
                || ruleset["rules"]
                    .as_array()
                    .is_none_or(|rules| !rules.iter().any(|rule| rule["type"] == rule_kind))
            {
                return Err("journal permanence rules allow bypass or lack evidence".to_owned());
            }
        }
        Ok(())
    }

    fn head(&self) -> Result<String, String> {
        let value = self.api().get(&format!(
            "repos/{}/git/ref/heads/acme-claims",
            self.repository
        ))?;
        let sha = text(&value, "/object/sha")?;
        if value["ref"] != JOURNAL_REF || value["object"]["type"] != "commit" || !oid(sha) {
            return Err("invalid journal branch reference".to_owned());
        }
        Ok(sha.to_owned())
    }

    fn tree(&self, tree: &str, policy: &ClaimPolicy) -> Result<BTreeMap<String, String>, String> {
        let value = self.api().get(&format!(
            "repos/{}/git/trees/{tree}?recursive=1",
            self.repository
        ))?;
        if value["sha"] != tree || value["truncated"] != false {
            return Err("missing or truncated journal tree".to_owned());
        }
        let rows = value["tree"]
            .as_array()
            .ok_or("missing journal tree objects")?;
        if rows.len() > policy.max_entries.saturating_mul(4).saturating_add(4) {
            return Err("journal tree exceeds entry budget".to_owned());
        }
        let mut paths = BTreeSet::new();
        let mut directories = BTreeSet::new();
        let mut files = BTreeMap::new();
        for row in rows {
            let path = text(row, "/path")?;
            let sha = text(row, "/sha")?;
            if !paths.insert(path.to_owned())
                || !oid(sha)
                || path
                    .split('/')
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
            {
                return Err("invalid journal tree object".to_owned());
            }
            match (text(row, "/type")?, text(row, "/mode")?) {
                ("tree", "040000") => {
                    directories.insert(path.to_owned());
                    let sequence_directory =
                        path.strip_prefix("journal/entries/")
                            .is_some_and(|sequence| {
                                sequence.len() == 20
                                    && sequence.bytes().all(|byte| byte.is_ascii_digit())
                                    && sequence.parse::<usize>().is_ok_and(|number| number > 0)
                            });
                    if path != "journal" && path != "journal/entries" && !sequence_directory {
                        return Err("unexpected journal directory".to_owned());
                    }
                }
                ("blob", "100644") => {
                    files.insert(path.to_owned(), sha.to_owned());
                }
                _ => return Err("unsupported journal object type or mode".to_owned()),
            }
        }
        let mut expected_directories = BTreeSet::new();
        for path in files.keys() {
            let mut parent = path.as_str();
            while let Some((prefix, _)) = parent.rsplit_once('/') {
                if !paths.contains(prefix) {
                    return Err("journal tree omits a directory".to_owned());
                }
                expected_directories.insert(prefix.to_owned());
                parent = prefix;
            }
        }
        if directories != expected_directories {
            return Err("journal tree contains an extra empty directory".to_owned());
        }
        Ok(files)
    }

    fn blob(&self, sha: &str, limit: usize) -> Result<Vec<u8>, String> {
        let value = self
            .api()
            .get(&format!("repos/{}/git/blobs/{sha}", self.repository))?;
        let size = value["size"].as_u64().ok_or("missing blob size")?;
        if value["sha"] != sha
            || value["encoding"] != "base64"
            || size > limit as u64
            || value.get("truncated").is_some_and(|flag| flag != false)
        {
            return Err("invalid or oversized journal blob".to_owned());
        }
        let bytes = decode64(text(&value, "/content")?)?;
        if bytes.len() as u64 != size {
            return Err("truncated journal blob".to_owned());
        }
        Ok(bytes)
    }

    fn provenance(
        &self,
        entry: &Entry,
        raw: &[u8],
        bundle: &[u8],
        policy: &ClaimPolicy,
    ) -> Result<ReceiptProvenance, String> {
        let run = &entry.recorded_by;
        if run.run_id == 0
            || run.run_attempt == 0
            || run.workflow_path != WORKFLOW
            || !oid(&run.workflow_revision)
        {
            return Err("invalid recorded workflow identity".to_owned());
        }
        let cert = self.attestation.verify(raw, bundle, policy, run)?;
        let repository_uri = format!("https://github.com/{}", self.repository);
        let owner_uri = format!(
            "https://github.com/{}",
            self.repository
                .split_once('/')
                .ok_or("invalid repository identity")?
                .0
        );
        let signer = format!("{repository_uri}/{WORKFLOW}@refs/heads/main");
        let invocation = format!(
            "{repository_uri}/actions/runs/{}/attempts/{}",
            run.run_id, run.run_attempt
        );
        if cert.issuer != "https://token.actions.githubusercontent.com"
            || cert.subject_alternative_name != signer
            || cert.repository_id != policy.repository_id.to_string()
            || cert.owner_id != policy.owner_id.to_string()
            || cert.owner_uri != owner_uri
            || cert.repository_uri != repository_uri
            || cert.source_ref != "refs/heads/main"
            || cert.source_digest != run.workflow_revision
            || cert.signer_uri != signer
            || cert.signer_digest != run.workflow_revision
            || cert.build_config_uri != signer
            || cert.build_config_digest != run.workflow_revision
            || cert.runner_environment != "github-hosted"
            || cert.invocation_uri != invocation
        {
            return Err("verified certificate has unauthorized producer provenance".to_owned());
        }
        let metadata = self.api().get(&format!(
            "repos/{}/actions/runs/{}/attempts/{}",
            self.repository, run.run_id, run.run_attempt
        ))?;
        if metadata["id"] != run.run_id
            || metadata["run_attempt"] != run.run_attempt
            || metadata["path"] != WORKFLOW
            || metadata["head_sha"] != run.workflow_revision
            || metadata["head_branch"] != "main"
            || metadata["repository"]["id"].as_u64() != Some(policy.repository_id)
            || metadata["repository"]["full_name"] != self.repository
            || metadata["repository"]["owner"]["id"].as_u64() != Some(policy.owner_id)
        {
            return Err("certified workflow does not match Actions metadata".to_owned());
        }
        let mut historical = self.policy(&run.workflow_revision)?;
        // Use current budgets after verifying the historical policy's approval.
        historical.max_entries = policy.max_entries;
        historical.max_entry_bytes = policy.max_entry_bytes;
        historical.max_bundle_bytes = policy.max_bundle_bytes;
        historical.max_api_requests = policy.max_api_requests;
        if historical != *policy {
            return Err("historical claim policy does not match installation".to_owned());
        }
        let owner_metadata = self.api().get(&format!("repos/{}", self.repository))?;
        let owner = text(&owner_metadata, "/owner/login")?;
        let workflow = self
            .api()
            .file(&self.repository, WORKFLOW, &run.workflow_revision)?;
        let approval = GitHubStateReader::new(self.api()).file_approval(
            &self.repository,
            WORKFLOW,
            &workflow,
            owner,
            &run.workflow_revision,
        )?;
        let approved = self.api().file(
            &self.repository,
            WORKFLOW,
            text(&approval, "/merge_commit_sha")?,
        )?;
        if approved.as_bytes() != workflow.as_bytes() {
            return Err("claim workflow differs from its exact owner-approved bytes".to_owned());
        }
        Ok(ReceiptProvenance {
            repository: self.repository.clone(),
            writer: signer,
            protected_main: true,
            run: run.clone(),
        })
    }

    fn ancestry(&self, policy: &ClaimPolicy, head: &str) -> Result<Vec<CommitSnapshot>, String> {
        let mut current = head.to_owned();
        let mut seen = BTreeSet::new();
        let mut commits = Vec::new();
        loop {
            if commits.len() > policy.max_entries || !seen.insert(current.clone()) {
                return Err("journal ancestry exceeds budget or contains a cycle".to_owned());
            }
            let commit = self
                .api()
                .get(&format!("repos/{}/git/commits/{current}", self.repository))?;
            if commit["sha"] != current {
                return Err("journal commit identity mismatch".to_owned());
            }
            let tree = text(&commit, "/tree/sha")?;
            if !oid(tree) {
                return Err("invalid journal tree identity".to_owned());
            }
            let files = self.tree(tree, policy)?;
            let parents = commit["parents"]
                .as_array()
                .ok_or("missing journal parents")?;
            if current == policy.genesis_oid {
                if !parents.is_empty()
                    || files.len() != 1
                    || !files.contains_key("journal/genesis.json")
                {
                    return Err("invalid owner-approved journal genesis".to_owned());
                }
                let genesis: Genesis = strict_json(
                    &self.blob(&files["journal/genesis.json"], policy.max_entry_bytes)?,
                )?;
                if genesis.version != 1
                    || genesis.repository != policy.repository
                    || genesis.repository_id != policy.repository_id
                    || genesis.owner_id != policy.owner_id
                    || genesis.journal_ref != JOURNAL_REF
                {
                    return Err("genesis identity does not match owner policy".to_owned());
                }
                commits.push(CommitSnapshot {
                    oid: current,
                    parent: String::new(),
                    files,
                });
                break;
            }
            if parents.len() != 1 {
                return Err("journal ancestry is missing or nonlinear".to_owned());
            }
            let parent = text(&parents[0], "/sha")?.to_owned();
            if !oid(&parent) {
                return Err("invalid journal parent".to_owned());
            }
            commits.push(CommitSnapshot {
                oid: current,
                parent: parent.clone(),
                files,
            });
            current = parent;
        }
        commits.reverse();
        Ok(commits)
    }

    fn load_all(&self, policy: &ClaimPolicy) -> Result<Loaded, String> {
        self.protections(policy)?;
        let head = self.head()?;
        let mut commits = self.ancestry(policy, &head)?;
        let mut loaded = Loaded {
            head,
            sequence: 0,
            owner: None,
            receipts: Vec::new(),
            entries: Vec::new(),
            pending: None,
            armed: None,
            arms: BTreeSet::new(),
            uncertain: false,
        };
        let mut prior = commits.remove(0).files;
        let mut operations = BTreeSet::new();
        for CommitSnapshot {
            oid: commit,
            parent,
            mut files,
        } in commits
        {
            loaded.sequence += 1;
            let directory = format!("journal/entries/{:020}", loaded.sequence);
            let entry_sha = files
                .remove(&format!("{directory}/entry.json"))
                .ok_or("missing journal entry")?;
            let bundle_sha = files
                .remove(&format!("{directory}/entry.sigstore.json"))
                .ok_or("missing journal bundle")?;
            if files != prior {
                return Err("journal commit changes paths beyond its two additions".to_owned());
            }
            let raw = self.blob(&entry_sha, policy.max_entry_bytes)?;
            let bundle = self.blob(&bundle_sha, policy.max_bundle_bytes)?;
            if !raw.ends_with(b"\n") {
                return Err("journal entry lacks final LF".to_owned());
            }
            let entry: Entry = strict_json(&raw)?;
            if entry.version != 1
                || entry.sequence != loaded.sequence
                || entry.expected_parent != parent
                || entry.repository != policy.repository
                || entry.repository_id != policy.repository_id
                || entry.owner_id != policy.owner_id
                || entry.operation_id.is_empty()
                || entry.operation_id.len() > 128
                || entry.session_nonce.len() != 64
                || !entry
                    .session_nonce
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || !operations.insert((
                    entry.recorded_by.run_id,
                    entry.recorded_by.run_attempt,
                    entry.session_nonce.clone(),
                    entry.operation_id.clone(),
                ))
            {
                return Err("invalid or repeated journal envelope".to_owned());
            }
            let provenance = self.provenance(&entry, &raw, &bundle, policy)?;
            fold(&mut loaded, &entry, &raw, provenance)?;
            loaded.entries.push((entry, raw, bundle, commit));
            prior.insert(format!("{directory}/entry.json"), entry_sha);
            prior.insert(format!("{directory}/entry.sigstore.json"), bundle_sha);
        }
        crate::claims::validate_journal(
            &loaded.receipts,
            &crate::claims::ReconcileContext {
                repository: self.repository.clone(),
                trusted_workflow: WORKFLOW.to_owned(),
                trusted_writer: format!(
                    "https://github.com/{}/{WORKFLOW}@refs/heads/main",
                    self.repository
                ),
                run: self.run.clone(),
            },
        )
        .map_err(|error| format!("invalid receipt journal: {error:?}"))?;
        if self.head()? != loaded.head {
            return Err("journal head moved during complete load".to_owned());
        }
        Ok(loaded)
    }
}

fn fold(
    loaded: &mut Loaded,
    entry: &Entry,
    raw: &[u8],
    provenance: ReceiptProvenance,
) -> Result<(), String> {
    if loaded.uncertain {
        return Err("unknown assignment cannot be settled by this protocol".to_owned());
    }
    if matches!(entry.operation, Operation::Acquire {}) {
        if loaded.owner.is_some() {
            return Err("journal ownership is already held".to_owned());
        }
        loaded.owner = Some(Owner {
            generation: digest(raw),
            nonce: entry.session_nonce.clone(),
            run: entry.recorded_by.clone(),
        });
        return Ok(());
    }
    let owner = loaded
        .owner
        .as_ref()
        .ok_or("operation has no journal owner")?;
    let generation = match &entry.operation {
        Operation::Receipt { generation, .. }
        | Operation::ArmAssignment { generation, .. }
        | Operation::Release { generation } => generation,
        Operation::Acquire {} => unreachable!(),
    };
    if generation != &owner.generation
        || owner.nonce != entry.session_nonce
        || owner.run != entry.recorded_by
    {
        return Err("journal operation belongs to another producer or generation".to_owned());
    }
    match &entry.operation {
        Operation::Receipt { receipt, .. } => {
            if receipt.version != 2 || receipt.recorded_by != entry.recorded_by {
                return Err("receipt version or recording identity is invalid".to_owned());
            }
            let binding = &receipt.attempt.binding;
            let request = AssignmentRequest::from_approved_snapshot(
                &binding.approved_contract,
                &binding.approved_main_revision,
            )?;
            if RequestBinding::from(&request) != *binding
                || receipt.attempt.number == 0
                || binding.repository != entry.repository
                || receipt.attempt.run.workflow_revision != binding.approved_main_revision
            {
                return Err("receipt request binding is invalid".to_owned());
            }
            match receipt.state {
                ReceiptState::Pending => {
                    if loaded.pending.is_some() || receipt.attempt.run != entry.recorded_by {
                        return Err(
                            "pending receipt conflicts with an unsettled attempt".to_owned()
                        );
                    }
                    loaded.pending = Some(receipt.attempt.clone());
                }
                ReceiptState::Accepted | ReceiptState::NotAssigned | ReceiptState::Unknown => {
                    if loaded.pending.as_ref() != Some(&receipt.attempt) {
                        return Err("outcome does not match the pending attempt".to_owned());
                    }
                    if receipt.state == ReceiptState::Unknown {
                        loaded.uncertain = true;
                    } else {
                        loaded.pending = None;
                        loaded.armed = None;
                    }
                }
                ReceiptState::Completed => {
                    if loaded.pending.is_some() {
                        return Err("completion cannot settle uncertain ownership".to_owned());
                    }
                }
            }
            loaded.receipts.push(StoredReceipt {
                receipt: receipt.as_ref().clone(),
                provenance: Evidence::Known(provenance),
            });
        }
        Operation::ArmAssignment { attempt, .. } => {
            let key = serde_json::to_string(attempt).map_err(|error| error.to_string())?;
            if loaded.pending.as_ref() != Some(attempt)
                || loaded.armed.is_some()
                || !loaded.arms.insert(key)
            {
                return Err("assignment arm has no unique pending attempt".to_owned());
            }
            loaded.armed = Some(attempt.clone());
        }
        Operation::Release { .. } => {
            if loaded.pending.is_some() || loaded.armed.is_some() {
                return Err("release has an unsettled attempt".to_owned());
            }
            loaded.owner = None;
        }
        Operation::Acquire {} => unreachable!(),
    }
    Ok(())
}

pub struct GitHubClaimSession<'a, C, A> {
    store: &'a GitHubClaimStore<C, A>,
    policy: ClaimPolicy,
    owner: Owner,
    head: String,
    sequence: usize,
    operation: usize,
    poisoned: bool,
    closed: bool,
    reserved: bool,
    returned_state: Option<ReceiptState>,
}

impl<C: ClaimClient, A: Attestation> ClaimStore for GitHubClaimStore<C, A> {
    type Session<'a>
        = GitHubClaimSession<'a, C, A>
    where
        Self: 'a;
    fn lock(&self, repository: &str) -> Result<Self::Session<'_>, String> {
        if self.used.replace(true) || repository != self.repository {
            return Err("producer cannot resume or reuse journal ownership".to_owned());
        }
        self.remaining.set(1);
        let candidate =
            self.api()
                .file(&self.repository, POLICY_PATH, &self.run.workflow_revision)?;
        let candidate: ClaimPolicy = strict_json(candidate.as_bytes())?;
        candidate.validate(&self.repository)?;
        // Apply the candidate's request limit before approval reads.
        // Do not use its trust anchors until approval succeeds.
        self.remaining.set(candidate.max_api_requests - 1);
        let policy = self.policy(&self.run.workflow_revision)?;
        if candidate != policy {
            return Err("claim policy changed during approval verification".to_owned());
        }
        let main =
            GitHubStateReader::new(self.api()).branch(&format!("repos/{}", self.repository))?;
        if self.policy(&main)? != policy {
            return Err("claim policy changed after the producer source revision".to_owned());
        }
        self.protections(&policy)?;
        let loaded = self.load_all(&policy)?;
        if loaded.owner.is_some() {
            return Err("journal is busy; ownership never expires".to_owned());
        }
        let mut session = GitHubClaimSession {
            store: self,
            policy,
            owner: Owner {
                generation: String::new(),
                nonce: self.nonce.clone(),
                run: self.run.clone(),
            },
            head: loaded.head,
            sequence: loaded.sequence,
            operation: 0,
            poisoned: false,
            closed: false,
            reserved: false,
            returned_state: None,
        };
        let generation = session.write(Operation::Acquire {})?;
        session.owner.generation = generation;
        session.current()?;
        Ok(session)
    }
}

impl<C: ClaimClient, A: Attestation> GitHubClaimSession<'_, C, A> {
    fn usable(&self) -> Result<(), String> {
        if self.poisoned || self.closed {
            return Err("claim session is poisoned or closed".to_owned());
        }
        Ok(())
    }

    fn current(&mut self) -> Result<Loaded, String> {
        self.usable()?;
        let result = self.store.load_all(&self.policy).and_then(|loaded| {
            if loaded.head != self.head || loaded.owner.as_ref() != Some(&self.owner) {
                return Err("claim session lost its exact journal ownership".to_owned());
            }
            Ok(loaded)
        });
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    fn write(&mut self, operation: Operation) -> Result<String, String> {
        self.usable()?;
        let mut before = if matches!(operation, Operation::Acquire {}) {
            self.store.load_all(&self.policy)?
        } else {
            self.current()?
        };
        self.operation += 1;
        let entry = Entry {
            version: 1,
            sequence: self.sequence + 1,
            expected_parent: self.head.clone(),
            repository: self.policy.repository.clone(),
            repository_id: self.policy.repository_id,
            owner_id: self.policy.owner_id,
            operation_id: format!("{}-{}", self.store.nonce, self.operation),
            session_nonce: self.store.nonce.clone(),
            recorded_by: self.store.run.clone(),
            operation,
        };
        let mut raw = serde_json::to_vec(&entry).map_err(|error| error.to_string())?;
        raw.push(b'\n');
        let validation = fold(
            &mut before,
            &entry,
            &raw,
            ReceiptProvenance {
                repository: self.policy.repository.clone(),
                writer: format!(
                    "https://github.com/{}/{WORKFLOW}@refs/heads/main",
                    self.policy.repository
                ),
                protected_main: true,
                run: self.store.run.clone(),
            },
        )
        .and_then(|()| {
            crate::claims::validate_journal(
                &before.receipts,
                &crate::claims::ReconcileContext {
                    repository: self.policy.repository.clone(),
                    trusted_workflow: WORKFLOW.to_owned(),
                    trusted_writer: format!(
                        "https://github.com/{}/{WORKFLOW}@refs/heads/main",
                        self.policy.repository
                    ),
                    run: self.store.run.clone(),
                },
            )
            .map(|_| ())
            .map_err(|error| format!("invalid proposed receipt: {error:?}"))
        });
        if let Err(error) = validation {
            self.poisoned = true;
            return Err(error);
        }
        let result = self.write_exact(&entry, &raw);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }

    fn write_exact(&mut self, entry: &Entry, raw: &[u8]) -> Result<String, String> {
        if entry.sequence > self.policy.max_entries || raw.len() > self.policy.max_entry_bytes {
            return Err("claim entry budget exhausted".to_owned());
        }
        // Count the job-identity request used by the signing helper.
        self.store.api().charge()?;
        let bundle = self.store.attestation.sign(raw, &self.policy)?;
        if bundle.len() > self.policy.max_bundle_bytes {
            return Err("claim bundle budget exhausted".to_owned());
        }
        self.store.provenance(entry, raw, &bundle, &self.policy)?;
        let directory = format!("journal/entries/{:020}", entry.sequence);
        let input = serde_json::json!({
            "branch": { "repositoryNameWithOwner": self.policy.repository, "branchName": "acme-claims" },
            "expectedHeadOid": self.head,
            "message": { "headline": format!("claim journal operation {}", entry.operation_id) },
            "clientMutationId": entry.operation_id,
            "fileChanges": { "additions": [
                { "path": format!("{directory}/entry.json"), "contents": encode64(raw) },
                { "path": format!("{directory}/entry.sigstore.json"), "contents": encode64(&bundle) }
            ] }
        });
        self.store.api().charge()?;
        if matches!(entry.operation, Operation::Release { .. }) {
            self.closed = true;
        }
        let mutation = self.store.client.create_commit(&input);
        let loaded = self.store.load_all(&self.policy).map_err(|error| {
            format!(
                "unresolved journal operation {}: {error}",
                entry.operation_id
            )
        })?;
        let matching = loaded
            .entries
            .iter()
            .find(|(candidate, bytes, saved_bundle, _)| {
                candidate == entry && bytes == raw && saved_bundle == &bundle
            })
            .ok_or_else(|| {
                format!(
                    "unresolved journal operation {}; retain ownership",
                    entry.operation_id
                )
            })?;
        if mutation.as_ref().is_ok_and(|sha| sha != &matching.3)
            || matching.0.expected_parent != self.head
            || loaded.head != matching.3
        {
            return Err("journal acknowledgement does not match the exact append".to_owned());
        }
        self.head = loaded.head;
        self.sequence = loaded.sequence;
        Ok(digest(raw))
    }
}

impl<C: ClaimClient, A: Attestation> ClaimSession for GitHubClaimSession<'_, C, A> {
    fn load(&mut self) -> Result<Vec<StoredReceipt>, String> {
        self.current().map(|loaded| loaded.receipts)
    }
    fn append(&mut self, receipt: &Receipt) -> Result<(), String> {
        if self.reserved && self.returned_state != Some(receipt.state) {
            self.poisoned = true;
            return Err("receipt does not match the synchronous assignment outcome".to_owned());
        }
        self.write(Operation::Receipt {
            generation: self.owner.generation.clone(),
            receipt: Box::new(receipt.clone()),
        })?;
        if matches!(
            receipt.state,
            ReceiptState::Accepted | ReceiptState::NotAssigned
        ) {
            self.reserved = false;
            self.returned_state = None;
        }
        if receipt.state == ReceiptState::Unknown {
            self.poisoned = true;
        }
        Ok(())
    }
    fn arm_assignment(&mut self, attempt: &AttemptId) -> Result<(), String> {
        self.write(Operation::ArmAssignment {
            generation: self.owner.generation.clone(),
            attempt: attempt.clone(),
        })?;
        Ok(())
    }
    fn assignment_returned(
        &mut self,
        returned: crate::assignment::AssignmentReturn,
    ) -> Result<(), String> {
        self.usable()?;
        if !self.reserved
            || self.returned_state.is_some()
            || returned.generation() != self.owner.generation
        {
            self.poisoned = true;
            return Err("assignment outcome is absent or already recorded".to_owned());
        }
        self.returned_state = Some(match returned.outcome() {
            crate::assignment::AssignmentOutcome::Accepted => ReceiptState::Accepted,
            crate::assignment::AssignmentOutcome::NotAssigned => ReceiptState::NotAssigned,
            crate::assignment::AssignmentOutcome::Unknown => ReceiptState::Unknown,
        });
        Ok(())
    }
    fn reserve_assignment(
        &mut self,
        attempt: &AttemptId,
        request: &AssignmentRequest,
    ) -> Result<String, String> {
        let result = (|| {
            let loaded = self.current()?;
            self.store.protections(&self.policy)?;
            self.current()?;
            if self.reserved
                || loaded.armed.as_ref() != Some(attempt)
                || loaded.pending.as_ref() != Some(attempt)
                || attempt.binding != RequestBinding::from(request)
                || attempt.run != self.owner.run
            {
                return Err(
                    "assignment permit is absent, used, or bound to another attempt".to_owned(),
                );
            }
            self.reserved = true;
            Ok(self.owner.generation.clone())
        })();
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn release(&mut self) -> Result<(), String> {
        let loaded = self.current()?;
        if self.reserved || loaded.pending.is_some() || loaded.armed.is_some() {
            self.poisoned = true;
            return Err("unsettled assignment retains journal ownership".to_owned());
        }
        let operation = Operation::Release {
            generation: self.owner.generation.clone(),
        };
        let result = self.write(operation);
        self.closed = true;
        result.map(|_| ())
    }
}

fn text<'a>(value: &'a Value, path: &str) -> Result<&'a str, String> {
    value
        .pointer(path)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string {path}"))
}
fn oid(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut output, byte| {
        write!(&mut output, "{byte:02x}").expect("writing to a string cannot fail");
        output
    })
}
fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(crate) fn encode64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let word = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for shift in [18, 12, 6, 0] {
            let position = (18 - shift) / 6;
            if position > chunk.len() {
                encoded.push('=');
            } else {
                encoded.push(char::from(TABLE[((word >> shift) & 63) as usize]));
            }
        }
    }
    encoded
}

pub(crate) fn decode64(value: &str) -> Result<Vec<u8>, String> {
    let clean: Vec<u8> = value
        .bytes()
        .filter(|byte| !matches!(byte, b'\r' | b'\n'))
        .collect();
    if clean.len() % 4 != 0 {
        return Err("invalid base64 object".to_owned());
    }
    let mut bytes = Vec::new();
    for (index, chunk) in clean.chunks(4).enumerate() {
        let mut word = 0_u32;
        let mut padding = 0;
        for &byte in chunk {
            let digit = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    padding += 1;
                    0
                }
                _ => return Err("invalid base64 object".to_owned()),
            };
            if padding > 0 && byte != b'=' {
                return Err("invalid base64 padding".to_owned());
            }
            word = (word << 6) | u32::from(digit);
        }
        if padding > 2 || (padding > 0 && index + 1 != clean.len() / 4) {
            return Err("invalid base64 padding".to_owned());
        }
        let word_bytes = word.to_be_bytes();
        bytes.push(word_bytes[1]);
        if padding < 2 {
            bytes.push(word_bytes[2]);
        }
        if padding == 0 {
            bytes.push(word_bytes[3]);
        }
    }
    if encode64(&bytes) != String::from_utf8(clean).map_err(|_| "invalid base64")? {
        return Err("noncanonical base64 object".to_owned());
    }
    Ok(bytes)
}
