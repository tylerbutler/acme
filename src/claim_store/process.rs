use super::{
    Attestation, ClaimClient, ClaimPolicy, PREDICATE, VerifiedCertificate, WORKFLOW, digest,
    strict_json, text,
};
use crate::claims::WorkflowRun;
use crate::github_state::GitHubClient;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const GH_VERSION: &str = "2.102.0";

struct PrivateWorkspace(PathBuf);

impl PrivateWorkspace {
    fn new() -> Result<Self, String> {
        let target = std::env::current_dir()
            .map_err(|_| "cannot locate the claim workspace")?
            .join("target");
        if !target.exists() {
            std::fs::create_dir(&target).map_err(|_| "cannot create claim workspace parent")?;
        }
        let metadata = std::fs::symlink_metadata(&target)
            .map_err(|_| "cannot inspect claim workspace parent")?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("claim workspace parent must be a regular directory".to_owned());
        }
        let mut random = [0_u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut source| source.read_exact(&mut random))
            .map_err(|_| "cannot create a random claim workspace")?;
        let path = target.join(format!("acme-claim-{}", super::hex(&random)));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&path)
            .map_err(|_| "cannot create private claim workspace")?;
        Ok(Self(path))
    }
}

impl Drop for PrivateWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct ProcessRequest {
    pub program: PathBuf,
    pub arguments: Vec<String>,
    pub stdin: Vec<u8>,
    pub environment: BTreeMap<String, String>,
    pub max_output_bytes: usize,
}

pub trait ProcessRunner {
    fn run(&self, request: &ProcessRequest) -> Result<Vec<u8>, String>;
}

#[derive(Clone, Copy, Default)]
pub struct SystemProcess;

impl ProcessRunner for SystemProcess {
    fn run(&self, request: &ProcessRequest) -> Result<Vec<u8>, String> {
        if !["GH_TOKEN", "GITHUB_TOKEN"].iter().any(|name| {
            request
                .environment
                .get(*name)
                .is_some_and(|token| !token.is_empty())
        }) {
            return Err("claim tools require an isolated job token".to_owned());
        }
        let workspace = PrivateWorkspace::new()?;
        let mut child = Command::new(&request.program)
            .args(&request.arguments)
            .env_clear()
            .envs(&request.environment)
            .env("HOME", &workspace.0)
            .env("GH_CONFIG_DIR", workspace.0.join("gh-config"))
            .env("XDG_CACHE_HOME", workspace.0.join("cache"))
            .current_dir(&workspace.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "cannot start pinned claim-store tool")?;
        let stdin = child.stdin.take().ok_or("claim tool stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("claim tool stdout unavailable")?;
        let input = request.stdin.clone();
        let writer = std::thread::spawn(move || {
            let mut stdin = stdin;
            stdin.write_all(&input)
        });
        let limit = request.max_output_bytes;
        let exceeded = Arc::new(AtomicBool::new(false));
        let output_exceeded = exceeded.clone();
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout
                .take(limit.saturating_add(1) as u64)
                .read_to_end(&mut bytes);
            output_exceeded.store(bytes.len() > limit, Ordering::Relaxed);
            result.map(|_| bytes)
        });
        let started = Instant::now();
        let mut interrupted = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None)
                    if started.elapsed() < Duration::from_secs(60)
                        && !exceeded.load(Ordering::Relaxed) =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => {
                    interrupted = true;
                    let _ = child.kill();
                    break child.wait();
                }
            }
        };
        let write = writer
            .join()
            .map_err(|_| "claim tool input thread failed")?;
        let output = reader
            .join()
            .map_err(|_| "claim tool output thread failed")?
            .map_err(|_| "claim tool output failed")?;
        if interrupted || output.len() > limit {
            return Err("claim tool exceeded its time or output budget".to_owned());
        }
        write.map_err(|_| "claim tool input failed")?;
        if !status.map_err(|_| "claim tool did not settle")?.success() {
            return Err("claim-store tool failed".to_owned());
        }
        std::fs::remove_dir_all(&workspace.0).map_err(|_| "claim workspace cleanup failed")?;
        Ok(output)
    }
}

fn environment(signing: bool) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for name in ["PATH", "SSL_CERT_FILE", "SSL_CERT_DIR"] {
        if let Ok(value) = std::env::var(name) {
            env.insert(name.to_owned(), value);
        }
    }
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if signing {
            env.insert("GITHUB_TOKEN".to_owned(), token);
        } else {
            env.insert("GH_TOKEN".to_owned(), token);
        }
    }
    if !signing {
        env.insert("GH_PROMPT_DISABLED".to_owned(), "1".to_owned());
        env.insert("GH_HOST".to_owned(), "github.com".to_owned());
    }
    if signing {
        for name in [
            "ACTIONS_ID_TOKEN_REQUEST_URL",
            "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            "GITHUB_ACTIONS",
            "GITHUB_REPOSITORY",
            "GITHUB_REPOSITORY_ID",
            "GITHUB_REPOSITORY_OWNER",
            "GITHUB_REPOSITORY_OWNER_ID",
            "GITHUB_SERVER_URL",
            "GITHUB_REF",
            "GITHUB_SHA",
            "GITHUB_RUN_ID",
            "GITHUB_RUN_ATTEMPT",
            "GITHUB_WORKFLOW_REF",
            "GITHUB_WORKFLOW_SHA",
            "RUNNER_ENVIRONMENT",
        ] {
            if let Ok(value) = std::env::var(name) {
                env.insert(name.to_owned(), value);
            }
        }
    }
    env
}

pub struct GhJobClient<P = SystemProcess> {
    runner: P,
    gh: PathBuf,
    repository: String,
    token: String,
}

impl<P: ProcessRunner> GhJobClient<P> {
    pub fn new(repository: &str, gh: PathBuf, runner: P) -> Result<Self, String> {
        let token = std::env::var("GITHUB_TOKEN")
            .map_err(|_| "claim-store client requires GITHUB_TOKEN")?;
        Self::with_job_token(repository, gh, runner, token)
    }

    /// Supply only the job token. Do not supply a user token.
    pub fn with_job_token(
        repository: &str,
        gh: PathBuf,
        runner: P,
        token: String,
    ) -> Result<Self, String> {
        if !gh.is_absolute()
            || repository.split('/').count() != 2
            || repository.split('/').any(|part| {
                part.is_empty()
                    || !part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            })
            || token.is_empty()
        {
            return Err("gh path or journal repository is invalid".to_owned());
        }
        Ok(Self {
            runner,
            gh,
            repository: repository.to_owned(),
            token,
        })
    }

    fn run(&self, arguments: Vec<String>, stdin: Vec<u8>) -> Result<Vec<u8>, String> {
        let mut env = environment(false);
        env.insert("GH_TOKEN".to_owned(), self.token.clone());
        self.runner.run(&ProcessRequest {
            program: self.gh.clone(),
            arguments,
            stdin,
            environment: env,
            max_output_bytes: 16 * 1024 * 1024,
        })
    }

    fn api(&self, mut arguments: Vec<String>, stdin: Vec<u8>) -> Result<Vec<u8>, String> {
        let version = self.run(vec!["--version".to_owned()], Vec::new())?;
        if !String::from_utf8_lossy(&version).starts_with(&format!("gh version {GH_VERSION} ")) {
            return Err("unsupported journal client version".to_owned());
        }
        let mut base = vec![
            "api".to_owned(),
            "--hostname".to_owned(),
            "github.com".to_owned(),
            "-H".to_owned(),
            "X-GitHub-Api-Version: 2026-03-10".to_owned(),
        ];
        base.append(&mut arguments);
        self.run(base, stdin)
    }
}

impl<P: ProcessRunner> GitHubClient for GhJobClient<P> {
    fn get(&self, endpoint: &str) -> Result<Value, String> {
        let prefix = format!("repos/{}", self.repository);
        if endpoint != prefix && !endpoint.starts_with(&format!("{prefix}/")) {
            return Err("unsupported journal read endpoint".to_owned());
        }
        strict_json(&self.api(
            vec!["--method".to_owned(), "GET".to_owned(), endpoint.to_owned()],
            Vec::new(),
        )?)
    }
    fn file(&self, repository: &str, path: &str, revision: &str) -> Result<String, String> {
        if repository != self.repository
            || !super::oid(revision)
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err("journal file read must use an immutable revision".to_owned());
        }
        let endpoint = format!("repos/{repository}/contents/{path}?ref={revision}");
        let bytes = self.api(
            vec![
                "--method".to_owned(),
                "GET".to_owned(),
                "-H".to_owned(),
                "Accept: application/vnd.github.raw+json".to_owned(),
                endpoint,
            ],
            Vec::new(),
        )?;
        String::from_utf8(bytes).map_err(|_| "non-UTF-8 approved file".to_owned())
    }
    fn graphql(&self, query: &str) -> Result<Value, String> {
        if !query.starts_with("query ") {
            return Err("unsupported journal GraphQL read".to_owned());
        }
        let result: Value = strict_json(
            &self.api(
                vec!["graphql".to_owned(), "--input".to_owned(), "-".to_owned()],
                serde_json::to_vec(&serde_json::json!({"query":query}))
                    .map_err(|error| error.to_string())?,
            )?,
        )?;
        if result.get("errors").is_some() {
            return Err("incomplete GraphQL journal evidence".to_owned());
        }
        Ok(result)
    }
}

impl<P: ProcessRunner> ClaimClient for GhJobClient<P> {
    fn create_commit(&self, input: &Value) -> Result<String, String> {
        let additions = input
            .pointer("/fileChanges/additions")
            .and_then(Value::as_array)
            .ok_or("missing journal additions")?;
        if input["branch"]["branchName"] != "acme-claims"
            || input["branch"]["repositoryNameWithOwner"] != self.repository
            || input.as_object().is_none_or(|fields| {
                fields.len() != 5
                    || ![
                        "branch",
                        "expectedHeadOid",
                        "message",
                        "clientMutationId",
                        "fileChanges",
                    ]
                    .iter()
                    .all(|key| fields.contains_key(*key))
            })
            || input["branch"]
                .as_object()
                .is_none_or(|fields| fields.len() != 2)
            || !input["expectedHeadOid"].as_str().is_some_and(super::oid)
            || additions.len() != 2
            || input["fileChanges"]
                .as_object()
                .is_none_or(|map| map.len() != 1)
            || input["message"]
                .as_object()
                .is_none_or(|fields| fields.len() != 1 || !fields.contains_key("headline"))
            || input["clientMutationId"]
                .as_str()
                .is_none_or(|id| id.is_empty() || id.len() > 128)
            || input["message"]["headline"]
                != format!(
                    "claim journal operation {}",
                    input["clientMutationId"].as_str().unwrap_or_default()
                )
        {
            return Err("write exceeds the fixed journal boundary".to_owned());
        }
        let paths: Vec<&str> = additions
            .iter()
            .map(|addition| text(addition, "/path"))
            .collect::<Result<_, _>>()?;
        let first = paths[0]
            .strip_suffix("/entry.json")
            .ok_or("invalid journal entry path")?;
        let sequence = first
            .strip_prefix("journal/entries/")
            .ok_or("invalid journal sequence path")?;
        if sequence.len() != 20
            || !sequence.bytes().all(|byte| byte.is_ascii_digit())
            || sequence
                .parse::<u64>()
                .ok()
                .is_none_or(|number| number == 0)
            || paths[1] != format!("{first}/entry.sigstore.json")
        {
            return Err("write exceeds the next journal entry pair".to_owned());
        }
        for addition in additions {
            if addition.as_object().is_none_or(|fields| fields.len() != 2) {
                return Err("unsupported journal addition fields".to_owned());
            }
            super::decode64(text(addition, "/contents")?)?;
        }
        let query = "mutation($input: CreateCommitOnBranchInput!) { createCommitOnBranch(input: $input) { commit { oid } } }";
        let request = serde_json::json!({"query":query,"variables":{"input":input}});
        let result: Value = strict_json(&self.api(
            vec!["graphql".to_owned(), "--input".to_owned(), "-".to_owned()],
            serde_json::to_vec(&request).map_err(|error| error.to_string())?,
        )?)?;
        if result.get("errors").is_some() {
            return Err("journal mutation outcome is uncertain".to_owned());
        }
        let sha = text(&result, "/data/createCommitOnBranch/commit/oid")?;
        if !super::oid(sha) {
            return Err("invalid journal mutation acknowledgement".to_owned());
        }
        Ok(sha.to_owned())
    }
}

pub struct ProcessAttestation<P = SystemProcess> {
    runner: P,
    root: PathBuf,
    node: PathBuf,
    gh: PathBuf,
}

impl<P: ProcessRunner> ProcessAttestation<P> {
    pub fn new(root: PathBuf, node: PathBuf, gh: PathBuf, runner: P) -> Result<Self, String> {
        if !root.is_absolute() || !node.is_absolute() || !gh.is_absolute() {
            return Err("claim tool paths must be absolute".to_owned());
        }
        Ok(Self {
            runner,
            root,
            node,
            gh,
        })
    }
    fn request(
        &self,
        program: &Path,
        arguments: Vec<String>,
        stdin: Vec<u8>,
        signing: bool,
        max_output_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        self.runner.run(&ProcessRequest {
            program: program.to_owned(),
            arguments,
            stdin,
            environment: environment(signing),
            max_output_bytes,
        })
    }
    fn check_gh(&self) -> Result<(), String> {
        let version = self.request(
            &self.gh,
            vec!["--version".to_owned()],
            Vec::new(),
            false,
            4096,
        )?;
        let expected = format!("gh version {GH_VERSION} ");
        if !String::from_utf8_lossy(&version).starts_with(&expected) {
            return Err("unsupported attestation verifier version".to_owned());
        }
        Ok(())
    }
}

impl<P: ProcessRunner> Attestation for ProcessAttestation<P> {
    fn sign(&self, entry: &[u8], policy: &ClaimPolicy) -> Result<Vec<u8>, String> {
        let result = self.request(
            &self.node,
            vec![
                self.root
                    .join("scripts/claim-attestation/sign.cjs")
                    .to_string_lossy()
                    .into_owned(),
            ],
            entry.to_owned(),
            true,
            policy.max_bundle_bytes,
        )?;
        strict_json::<Value>(&result)?;
        Ok(result)
    }

    fn verify(
        &self,
        entry: &[u8],
        bundle: &[u8],
        policy: &ClaimPolicy,
        run: &WorkflowRun,
    ) -> Result<VerifiedCertificate, String> {
        self.check_gh()?;
        strict_json::<Value>(bundle)?;
        let workspace = PrivateWorkspace::new()?;
        let scratch = &workspace.0;
        let result = (|| {
            let artifact = scratch.join("entry.json");
            let saved = scratch.join("entry.sigstore.json");
            std::fs::write(&artifact, entry).map_err(|_| "cannot write exact entry bytes")?;
            std::fs::write(&saved, bundle).map_err(|_| "cannot write saved attestation bundle")?;
            let args = vec![
                "attestation".to_owned(),
                "verify".to_owned(),
                artifact.to_string_lossy().into_owned(),
                "--bundle".to_owned(),
                saved.to_string_lossy().into_owned(),
                "--repo".to_owned(),
                policy.repository.clone(),
                "--hostname".to_owned(),
                "github.com".to_owned(),
                "--signer-workflow".to_owned(),
                format!(
                    "github.com/{}/{WORKFLOW}@refs/heads/main",
                    policy.repository
                ),
                "--signer-digest".to_owned(),
                run.workflow_revision.clone(),
                "--source-ref".to_owned(),
                "refs/heads/main".to_owned(),
                "--source-digest".to_owned(),
                run.workflow_revision.clone(),
                "--predicate-type".to_owned(),
                PREDICATE.to_owned(),
                "--cert-oidc-issuer".to_owned(),
                "https://token.actions.githubusercontent.com".to_owned(),
                "--deny-self-hosted-runners".to_owned(),
                "--digest-alg".to_owned(),
                "sha256".to_owned(),
                "--format".to_owned(),
                "json".to_owned(),
            ];
            let output = self.request(
                &self.gh,
                args,
                Vec::new(),
                false,
                policy
                    .max_bundle_bytes
                    .saturating_add(policy.max_entry_bytes)
                    .saturating_mul(4),
            )?;
            parse_verification(&output, entry)
        })();
        let cleanup = std::fs::remove_dir_all(scratch);
        if cleanup.is_err() {
            return Err("attestation workspace cleanup failed".to_owned());
        }
        result
    }
}

pub fn parse_verification(output: &[u8], entry: &[u8]) -> Result<VerifiedCertificate, String> {
    let output: Value = strict_json(output)?;
    let results = output
        .as_array()
        .ok_or("missing cryptographic verification results")?;
    if results.len() != 1 {
        return Err("ambiguous cryptographic verification results".to_owned());
    }
    let result = &results[0]["verificationResult"];
    if result["verifiedTimestamps"]
        .as_array()
        .is_none_or(Vec::is_empty)
    {
        return Err("attestation has no verified witnessed time".to_owned());
    }
    let subjects = result["statement"]["subject"]
        .as_array()
        .ok_or("missing signed subject")?;
    if subjects.len() != 1
        || subjects[0]["name"] != "entry.json"
        || subjects[0]["digest"]
            .as_object()
            .is_none_or(|digests| digests.len() != 1)
        || subjects[0]["digest"]["sha256"] != digest(entry)
        || result["statement"]["predicateType"] != PREDICATE
        || result["statement"]["predicate"]
            .as_object()
            .is_none_or(|map| !map.is_empty())
    {
        return Err("attestation does not bind the exact raw entry".to_owned());
    }
    let cert = &result["signature"]["certificate"];
    Ok(VerifiedCertificate {
        issuer: text(cert, "/issuer")?.to_owned(),
        subject_alternative_name: text(cert, "/subjectAlternativeName")?.to_owned(),
        repository_id: text(cert, "/sourceRepositoryIdentifier")?.to_owned(),
        owner_id: text(cert, "/sourceRepositoryOwnerIdentifier")?.to_owned(),
        owner_uri: text(cert, "/sourceRepositoryOwnerURI")?.to_owned(),
        repository_uri: text(cert, "/sourceRepositoryURI")?.to_owned(),
        source_ref: text(cert, "/sourceRepositoryRef")?.to_owned(),
        source_digest: text(cert, "/sourceRepositoryDigest")?.to_owned(),
        signer_uri: text(cert, "/buildSignerURI")?.to_owned(),
        signer_digest: text(cert, "/buildSignerDigest")?.to_owned(),
        build_config_uri: text(cert, "/buildConfigURI")?.to_owned(),
        build_config_digest: text(cert, "/buildConfigDigest")?.to_owned(),
        runner_environment: text(cert, "/runnerEnvironment")?.to_owned(),
        invocation_uri: text(cert, "/runInvocationURI")?.to_owned(),
    })
}
