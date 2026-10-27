use acme::claim_store::ClaimClient;
use acme::claim_store::process::{
    GhJobClient, ProcessRequest, ProcessRunner, SystemProcess, parse_verification,
};
use acme::github_state::GitHubClient;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const REPO: &str = "owner/acme";
const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

type CapturedCalls = Arc<Mutex<Vec<(Vec<String>, Vec<u8>)>>>;

struct FakeProcess {
    calls: CapturedCalls,
    fail: bool,
}

impl ProcessRunner for FakeProcess {
    fn run(&self, request: &ProcessRequest) -> Result<Vec<u8>, String> {
        assert_eq!(request.environment["GH_TOKEN"], "offline-job-token");
        for forbidden in ["GH_PAT", "GITHUB_TOKEN", "HOME", "NODE_OPTIONS"] {
            assert!(!request.environment.contains_key(forbidden));
        }
        assert!(request.max_output_bytes > 0);
        self.calls
            .lock()
            .unwrap()
            .push((request.arguments.clone(), request.stdin.clone()));
        if request.arguments == ["--version"] {
            return Ok(b"gh version 2.102.0 (2026-09-30)\n".to_vec());
        }
        assert_eq!(request.arguments[0], "api");
        if self.fail {
            return Err("lost response".to_owned());
        }
        if request.arguments.contains(&"graphql".to_owned()) {
            let body: Value = serde_json::from_slice(&request.stdin).unwrap();
            assert!(
                body["query"]
                    .as_str()
                    .unwrap()
                    .starts_with("mutation($input: CreateCommitOnBranchInput!)")
            );
            assert_eq!(body["variables"]["input"]["expectedHeadOid"], HEAD);
            return Ok(serde_json::to_vec(
                &json!({"data":{"createCommitOnBranch":{"commit":{"oid":HEAD}}}}),
            )
            .unwrap());
        }
        assert!(request.arguments.contains(&"GET".to_owned()));
        Ok(b"{}".to_vec())
    }
}

fn input() -> Value {
    json!({
        "branch":{"repositoryNameWithOwner":REPO,"branchName":"acme-claims"},
        "expectedHeadOid":HEAD,
        "message":{"headline":"claim journal operation offline-operation"},
        "clientMutationId":"offline-operation",
        "fileChanges":{"additions":[
            {"path":"journal/entries/00000000000000000001/entry.json","contents":"e30K"},
            {"path":"journal/entries/00000000000000000001/entry.sigstore.json","contents":"e30K"}
        ]}
    })
}

#[test]
fn concrete_job_client_sends_one_expected_head_mutation_with_only_job_identity() {
    for fail in [false, true] {
        let calls = CapturedCalls::default();
        let client = GhJobClient::with_job_token(
            REPO,
            PathBuf::from("/pinned/gh"),
            FakeProcess {
                calls: calls.clone(),
                fail,
            },
            "offline-job-token".to_owned(),
        )
        .unwrap();
        assert_eq!(client.create_commit(&input()).is_ok(), !fail);
        let captured = calls.lock().unwrap();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].0, ["--version"]);
        assert_eq!(captured[1].0.last().unwrap(), "-");
        drop(captured);
    }
}

#[test]
fn concrete_client_rejects_unconditional_or_out_of_scope_mutations_before_any_process() {
    for fault in [
        "head",
        "branch",
        "repository",
        "path",
        "deletion",
        "extra",
        "mode",
        "metadata",
        "message",
    ] {
        let calls = CapturedCalls::default();
        let client = GhJobClient::with_job_token(
            REPO,
            PathBuf::from("/pinned/gh"),
            FakeProcess {
                calls: calls.clone(),
                fail: false,
            },
            "offline-job-token".to_owned(),
        )
        .unwrap();
        let mut mutation = input();
        match fault {
            "head" => {
                mutation.as_object_mut().unwrap().remove("expectedHeadOid");
            }
            "branch" => mutation["branch"]["branchName"] = json!("main"),
            "repository" => mutation["branch"]["repositoryNameWithOwner"] = json!("other/acme"),
            "path" => mutation["fileChanges"]["additions"][1]["path"] = json!("README.md"),
            "deletion" => mutation["fileChanges"]["deletions"] = json!([{"path":"README.md"}]),
            "extra" => mutation["unexpected"] = json!(true),
            "mode" => mutation["fileChanges"]["additions"][0]["mode"] = json!("100755"),
            "metadata" => {
                mutation.as_object_mut().unwrap().remove("message");
                mutation["author"] = json!({"name":"untrusted"});
            }
            "message" => mutation["message"]["body"] = json!("untrusted"),
            _ => unreachable!(),
        }
        assert!(client.create_commit(&mutation).is_err(), "{fault}");
        assert!(calls.lock().unwrap().is_empty(), "{fault}");
        assert!(client.get("repos/other/acme").is_err());
        assert!(
            client
                .file(REPO, "factory/dispatch/claim-store.json", "main")
                .is_err()
        );
    }
}

#[test]
fn unsupported_tool_versions_block_before_any_api_request() {
    struct WrongVersion;
    impl ProcessRunner for WrongVersion {
        fn run(&self, request: &ProcessRequest) -> Result<Vec<u8>, String> {
            assert_eq!(request.arguments, ["--version"]);
            Ok(b"gh version 2.101.0 (unsupported)\n".to_vec())
        }
    }
    let client = GhJobClient::with_job_token(
        REPO,
        PathBuf::from("/unpinned/gh"),
        WrongVersion,
        "offline-job-token".to_owned(),
    )
    .unwrap();
    assert!(client.create_commit(&input()).is_err());
    assert!(client.get("repos/owner/acme").is_err());
}

#[test]
fn real_process_boundary_limits_output_and_waits_for_the_child() {
    for (limit, token_name) in [
        (16, "GH_TOKEN"),
        (20_000, "GH_TOKEN"),
        (20_000, "GITHUB_TOKEN"),
    ] {
        let request = ProcessRequest {
            program: PathBuf::from("/usr/bin/python3"),
            arguments: vec![
                "-c".to_owned(),
                "import os, stat, sys; assert stat.S_IMODE(os.stat('.').st_mode) == 0o700; assert os.environ['HOME'] == os.getcwd(); sys.stdout.write('x' * 20000)".to_owned(),
            ],
            stdin: Vec::new(),
            environment: BTreeMap::from([(token_name.to_owned(), "offline-job-token".to_owned())]),
            max_output_bytes: limit,
        };
        let result = SystemProcess.run(&request);
        assert_eq!(result.is_ok(), limit == 20_000);
        if let Ok(bytes) = result {
            assert_eq!(bytes.len(), 20_000);
        }
    }
}

#[test]
fn verification_parser_rejects_asserted_predicate_identity_missing_timestamps_and_raw_digest_changes()
 {
    let bytes = b"{ \"entry\": true }\n";
    let certificate = json!({
        "issuer":"https://token.actions.githubusercontent.com",
        "subjectAlternativeName":"workflow",
        "sourceRepositoryIdentifier":"42","sourceRepositoryOwnerIdentifier":"7",
        "sourceRepositoryOwnerURI":"https://github.com/owner",
        "sourceRepositoryURI":"https://github.com/owner/acme",
        "sourceRepositoryRef":"refs/heads/main","sourceRepositoryDigest":HEAD,
        "buildSignerURI":"workflow","buildSignerDigest":HEAD,
        "buildConfigURI":"workflow","buildConfigDigest":HEAD,
        "runnerEnvironment":"github-hosted","runInvocationURI":"run/attempt"
    });
    let result = json!([{"verificationResult":{
        "signature":{"certificate":certificate},
        "verifiedTimestamps":[{"type":"Tlog"}],
        "statement":{"subject":[{"name":"entry.json","digest":{"sha256":format!("{:x}", Sha256::digest(bytes))}}],
            "predicateType":acme::claim_store::PREDICATE,"predicate":{}}
    }}]);
    assert!(parse_verification(&serde_json::to_vec(&result).unwrap(), bytes).is_ok());
    for fault in ["predicate", "timestamps", "certificate", "digest"] {
        let mut changed = result.clone();
        match fault {
            "predicate" => {
                changed[0]["verificationResult"]["statement"]["predicate"] = certificate.clone();
            }
            "timestamps" => changed[0]["verificationResult"]["verifiedTimestamps"] = json!([]),
            "certificate" => {
                changed[0]["verificationResult"]["signature"]["certificate"] = json!({});
            }
            "digest" => {
                changed[0]["verificationResult"]["statement"]["subject"][0]["digest"]["sha256"] =
                    json!(format!("{:x}", Sha256::digest(b"{\"entry\":true}\n")));
            }
            _ => unreachable!(),
        }
        assert!(
            parse_verification(&serde_json::to_vec(&changed).unwrap(), bytes).is_err(),
            "{fault}"
        );
    }
}
