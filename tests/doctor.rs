use acme::{FactoryConfig, GitHubError, GitHubTransport, builtin_modules, run_doctor};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const REPOSITORY: &str = "repos/tylerbutler/acme";
const LABELS: &str = "repos/tylerbutler/acme/labels?per_page=100";
const RULESETS: &str = "repos/tylerbutler/acme/rulesets?per_page=100";
const RULESET: &str = "repos/tylerbutler/acme/rulesets/7";
const ACTIONS: &str = "repos/tylerbutler/acme/actions/permissions/workflow";
type ConfigMutation = fn(&mut FactoryConfig);

#[derive(Clone)]
struct FakeGitHub {
    responses: HashMap<String, Vec<Value>>,
    calls: RefCell<Vec<String>>,
    error: Option<GitHubError>,
}

impl FakeGitHub {
    fn matching() -> Self {
        let mut labels: Value =
            serde_json::from_str(include_str!("../.github/labels.yml")).expect("label fixture");
        labels
            .as_array_mut()
            .expect("label list")
            .push(json!({"name": "unmanaged", "color": "ffffff", "description": "ignored"}));
        let ruleset: Value = serde_json::from_str(include_str!("../.github/rulesets/main.json"))
            .expect("ruleset fixture");
        Self {
            responses: HashMap::from([
                (LABELS.to_owned(), vec![labels]),
                (
                    RULESETS.to_owned(),
                    vec![json!([{"id": 7, "name": "main"}])],
                ),
                (RULESET.to_owned(), vec![ruleset]),
                (
                    REPOSITORY.to_owned(),
                    vec![json!({
                        "allow_auto_merge": false,
                        "allow_merge_commit": false,
                        "allow_rebase_merge": false,
                        "allow_squash_merge": true,
                        "delete_branch_on_merge": true,
                        "has_wiki": false
                    })],
                ),
                (
                    ACTIONS.to_owned(),
                    vec![json!({
                        "default_workflow_permissions": "read",
                        "can_approve_pull_request_reviews": false
                    })],
                ),
            ]),
            calls: RefCell::new(Vec::new()),
            error: None,
        }
    }
}

impl GitHubTransport for FakeGitHub {
    fn get(&self, endpoint: &str) -> Result<Vec<Value>, GitHubError> {
        self.calls.borrow_mut().push(endpoint.to_owned());
        if let Some(error) = self.error {
            return Err(error);
        }
        self.responses
            .get(endpoint)
            .cloned()
            .ok_or(GitHubError::ApiFailure)
    }
}

#[test]
fn matching_local_tree_and_paginated_github_state_pass_without_writes() {
    let config =
        FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml")).expect("fixture config");
    let mut github = FakeGitHub::matching();
    let labels = github.responses.get(LABELS).expect("labels").clone();
    let rulesets = github.responses.get(RULESETS).expect("rulesets").clone();
    let mut all_labels = labels[0].as_array().expect("label page").clone();
    let middle = all_labels.split_off(all_labels.len() / 2);
    github.responses.insert(
        LABELS.to_owned(),
        vec![Value::Array(all_labels), Value::Array(middle)],
    );
    github.responses.insert(
        RULESETS.to_owned(),
        vec![json!([{"id": 6, "name": "unmanaged"}]), rulesets[0].clone()],
    );

    let report = run_doctor(&config, Path::new(ROOT), &github);

    assert!(report.is_success(), "{report}");
    assert!(
        github
            .calls
            .borrow()
            .iter()
            .all(|path| { path.starts_with(REPOSITORY) && !path.contains("method=") })
    );
    assert!(
        report
            .to_string()
            .contains("DEFERRED deferred.path-author-enforcement")
    );
    assert!(
        report
            .to_string()
            .contains("DEFERRED deferred.model-separation")
    );
    assert!(
        report
            .to_string()
            .contains("DEFERRED deferred.token-scope-verification")
    );
}

#[test]
fn reports_each_managed_setting_drift() {
    let config =
        FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml")).expect("fixture config");
    for (check_id, mutation) in label_mutations()
        .into_iter()
        .chain(ruleset_mutations())
        .chain(pull_request_mutations())
        .chain(status_check_mutations())
        .chain(repository_mutations())
    {
        let mut github = FakeGitHub::matching();
        mutation.apply(&mut github);
        let report = run_doctor(&config, Path::new(ROOT), &github);
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.id == check_id && !check.passed),
            "expected {check_id} to fail after {mutation:?}\n{report}"
        );
    }
}

fn label_mutations() -> [(&'static str, Mutation); 2] {
    [
        (
            "github.label.needs_spec",
            Mutation::Label("color", json!("000000")),
        ),
        (
            "github.label.needs_spec",
            Mutation::Label("description", json!("changed")),
        ),
    ]
}

fn ruleset_mutations() -> [(&'static str, Mutation); 7] {
    [
        (
            "github.ruleset.name",
            Mutation::Ruleset(&["name"], json!("other")),
        ),
        (
            "github.ruleset.target",
            Mutation::Ruleset(&["target"], json!("tag")),
        ),
        (
            "github.ruleset.enforcement",
            Mutation::Ruleset(&["enforcement"], json!("disabled")),
        ),
        (
            "github.ruleset.no-bypass",
            Mutation::Ruleset(&["bypass_actors"], json!([{"actor_id": 1}])),
        ),
        (
            "github.ruleset.branch-conditions",
            Mutation::Ruleset(&["conditions", "ref_name", "include"], json!([])),
        ),
        (
            "github.ruleset.branch-conditions",
            Mutation::Ruleset(
                &["conditions", "ref_name", "exclude"],
                json!(["refs/heads/main"]),
            ),
        ),
        (
            "github.ruleset.rule-types",
            Mutation::RemoveRule("deletion"),
        ),
    ]
}

fn pull_request_mutations() -> [(&'static str, Mutation); 7] {
    [
        (
            "github.ruleset.pull-request.required_approving_review_count",
            Mutation::PullRequest("required_approving_review_count", json!(1)),
        ),
        (
            "github.ruleset.pull-request.dismiss_stale_reviews_on_push",
            Mutation::PullRequest("dismiss_stale_reviews_on_push", json!(false)),
        ),
        (
            "github.ruleset.pull-request.require_code_owner_review",
            Mutation::PullRequest("require_code_owner_review", json!(true)),
        ),
        (
            "github.ruleset.pull-request.require_last_push_approval",
            Mutation::PullRequest("require_last_push_approval", json!(true)),
        ),
        (
            "github.ruleset.pull-request.require_extra_approval_for_unattributed_changes",
            Mutation::PullRequest(
                "require_extra_approval_for_unattributed_changes",
                json!(true),
            ),
        ),
        (
            "github.ruleset.pull-request.required_review_thread_resolution",
            Mutation::PullRequest("required_review_thread_resolution", json!(false)),
        ),
        (
            "github.ruleset.pull-request.merge-methods",
            Mutation::PullRequest("allowed_merge_methods", json!(["merge"])),
        ),
    ]
}

const fn status_check_mutations() -> [(&'static str, Mutation); 3] {
    [
        (
            "github.ruleset.status-checks.strict_required_status_checks_policy",
            Mutation::StatusCheck("strict_required_status_checks_policy", json!(false)),
        ),
        (
            "github.ruleset.status-checks.do_not_enforce_on_create",
            Mutation::StatusCheck("do_not_enforce_on_create", json!(true)),
        ),
        (
            "github.ruleset.required-ci",
            Mutation::RequiredContext("OTHER"),
        ),
    ]
}

fn repository_mutations() -> [(&'static str, Mutation); 8] {
    [
        (
            "github.repository.allow_auto_merge",
            Mutation::Repository("allow_auto_merge", json!(true)),
        ),
        (
            "github.repository.allow_merge_commit",
            Mutation::Repository("allow_merge_commit", json!(true)),
        ),
        (
            "github.repository.allow_rebase_merge",
            Mutation::Repository("allow_rebase_merge", json!(true)),
        ),
        (
            "github.repository.allow_squash_merge",
            Mutation::Repository("allow_squash_merge", json!(false)),
        ),
        (
            "github.repository.delete_branch_on_merge",
            Mutation::Repository("delete_branch_on_merge", json!(false)),
        ),
        (
            "github.repository.has_wiki",
            Mutation::Repository("has_wiki", json!(true)),
        ),
        (
            "github.actions-defaults.default_workflow_permissions",
            Mutation::Actions("default_workflow_permissions", json!("write")),
        ),
        (
            "github.actions-defaults.can_approve_pull_request_reviews",
            Mutation::Actions("can_approve_pull_request_reviews", json!(true)),
        ),
    ]
}

#[test]
fn duplicate_managed_rulesets_fail_and_unrelated_rulesets_are_ignored() {
    let config =
        FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml")).expect("fixture config");
    let mut github = FakeGitHub::matching();
    github.responses.insert(
        RULESETS.to_owned(),
        vec![json!([
            {"id": 7, "name": "main"},
            {"id": 8, "name": "main"},
            {"id": 9, "name": "unmanaged"}
        ])],
    );

    let report = run_doctor(&config, Path::new(ROOT), &github);

    assert!(!report.is_success());
    assert!(report.checks.iter().any(|check| {
        check.id == "github.ruleset.unique-main" && check.observed == "found 2 rulesets named main"
    }));
    assert!(!github.calls.borrow().iter().any(|path| path == RULESET));
}

#[test]
fn missing_or_duplicate_managed_labels_fail() {
    let config =
        FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml")).expect("fixture config");
    for labels in [
        json!([]),
        json!([
            {"name": "needs-spec", "color": "D4C5F9", "description": "first"},
            {"name": "needs-spec", "color": "D4C5F9", "description": "second"}
        ]),
    ] {
        let mut github = FakeGitHub::matching();
        github.responses.insert(LABELS.to_owned(), vec![labels]);
        let report = run_doctor(&config, Path::new(ROOT), &github);
        assert!(
            report
                .checks
                .iter()
                .any(|check| { check.id == "github.label.needs_spec" && !check.passed }),
            "{report}"
        );
    }
}

#[test]
fn authentication_and_permission_errors_are_unsuccessful_without_leaking_values() {
    let config =
        FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml")).expect("fixture config");
    for (error, observed) in [
        (
            GitHubError::AuthenticationRequired,
            "authentication required",
        ),
        (GitHubError::PermissionDenied, "insufficient permission"),
        (GitHubError::ApiFailure, "GitHub API request failed"),
    ] {
        let mut github = FakeGitHub::matching();
        github.error = Some(error);
        let report = run_doctor(&config, Path::new(ROOT), &github);
        assert!(!report.is_success());
        assert!(report.to_string().contains(observed));
        assert!(!report.to_string().contains("GH_PAT"));
    }
}

#[test]
fn invalid_repository_identity_does_not_contact_github() {
    let mut config =
        FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml")).expect("fixture config");
    config.repository = "owner/repo?method=DELETE".to_owned();
    let github = FakeGitHub::matching();

    let report = run_doctor(&config, Path::new(ROOT), &github);

    assert!(!report.is_success());
    assert!(github.calls.borrow().is_empty());
    assert!(report.to_string().contains("github.repository-identity"));
}

#[test]
fn module_configuration_must_enable_declared_bindings() {
    let cases: [(&str, ConfigMutation); 5] = [
        ("local.module.ci.config-binding", disable_ci),
        ("local.module.labels.config-binding", disable_intake),
        ("local.module.issue-forms.config-binding", disable_intake),
        (
            "local.module.labels.config-binding",
            remove_labels_dependency,
        ),
        (
            "local.module.ruleset.config-binding",
            change_merge_implementation,
        ),
    ];
    for (check_id, mutate_config) in cases {
        let mut config = FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml"))
            .expect("fixture config");
        mutate_config(&mut config);
        let report = run_doctor(&config, Path::new(ROOT), &FakeGitHub::matching());
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.id == check_id && !check.passed),
            "{report}"
        );
    }
}

fn disable_ci(config: &mut FactoryConfig) {
    config.modules.ci.enabled = false;
}

fn disable_intake(config: &mut FactoryConfig) {
    config.modules.intake.enabled = false;
}

fn remove_labels_dependency(config: &mut FactoryConfig) {
    config
        .modules
        .intake
        .needs
        .retain(|dependency| dependency != "labels");
}

fn change_merge_implementation(config: &mut FactoryConfig) {
    "auto".clone_into(&mut config.modules.merge.implementation);
}

#[test]
fn local_contract_drift_is_named_and_never_rewritten() {
    let config =
        FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml")).expect("fixture config");
    let root = temporary_root();
    copy_assets(&root);
    let form = root.join(".github/ISSUE_TEMPLATE/idea.yml");
    fs::write(&form, "name: incomplete\n").expect("change temporary form");
    let action = root.join(".github/actions/setup-rust/action.yml");
    let changed_action = fs::read_to_string(&action)
        .expect("read temporary action")
        .replace("# ratchet:", "# missing-ratchet:");
    fs::write(&action, &changed_action).expect("change temporary action");
    let workflow = root.join(".github/workflows/ci.yml");
    let changed_workflow = fs::read_to_string(&workflow)
        .expect("read temporary workflow")
        .replace("cargo fmt --all -- --check", "cargo fmt");
    fs::write(&workflow, &changed_workflow).expect("change temporary workflow");

    let report = run_doctor(&config, &root, &FakeGitHub::matching());

    assert!(report.checks.iter().any(|check| {
        check.id == "local.asset._github_issue_template_idea_yml" && !check.passed
    }));
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.id == "local.issue-form.idea" && !check.passed })
    );
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.id == "local.ci.action-pins" && !check.passed })
    );
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.id == "local.ci.commands" && !check.passed })
    );
    assert_eq!(
        fs::read(form).expect("form stays unchanged"),
        b"name: incomplete\n"
    );
    assert!(
        fs::read_to_string(action)
            .expect("action stays unchanged")
            .contains("# missing-ratchet:")
    );
    assert!(
        fs::read_to_string(workflow)
            .expect("workflow stays unchanged")
            .contains("cargo fmt\n")
    );
    fs::remove_dir_all(root).expect("remove temporary tree");
}

#[derive(Debug)]
enum Mutation {
    Label(&'static str, Value),
    Ruleset(&'static [&'static str], Value),
    PullRequest(&'static str, Value),
    StatusCheck(&'static str, Value),
    RequiredContext(&'static str),
    RemoveRule(&'static str),
    Repository(&'static str, Value),
    Actions(&'static str, Value),
}

impl Mutation {
    fn apply(&self, github: &mut FakeGitHub) {
        match self {
            Self::Label(field, value) => {
                let page = &mut github.responses.get_mut(LABELS).expect("labels")[0];
                let label = page
                    .as_array_mut()
                    .expect("label list")
                    .iter_mut()
                    .find(|label| label["name"] == "needs-spec")
                    .expect("managed label");
                label[field] = value.clone();
            }
            Self::Ruleset(path, value) => {
                set_path(
                    &mut github.responses.get_mut(RULESET).expect("ruleset")[0],
                    path,
                    value.clone(),
                );
            }
            Self::PullRequest(field, value) => {
                set_rule_parameter(
                    &mut github.responses.get_mut(RULESET).expect("ruleset")[0],
                    "pull_request",
                    field,
                    value.clone(),
                );
            }
            Self::StatusCheck(field, value) => {
                set_rule_parameter(
                    &mut github.responses.get_mut(RULESET).expect("ruleset")[0],
                    "required_status_checks",
                    field,
                    value.clone(),
                );
            }
            Self::RequiredContext(context) => {
                let checks = rule_parameters(
                    &mut github.responses.get_mut(RULESET).expect("ruleset")[0],
                    "required_status_checks",
                );
                checks["required_status_checks"][0]["context"] = json!(context);
            }
            Self::RemoveRule(kind) => {
                let rules = github.responses.get_mut(RULESET).expect("ruleset")[0]["rules"]
                    .as_array_mut()
                    .expect("rules");
                rules.retain(|rule| rule["type"] != *kind);
            }
            Self::Repository(field, value) => {
                github.responses.get_mut(REPOSITORY).expect("repository")[0][field] = value.clone();
            }
            Self::Actions(field, value) => {
                github.responses.get_mut(ACTIONS).expect("Actions")[0][field] = value.clone();
            }
        }
    }
}

fn rule_parameters<'a>(ruleset: &'a mut Value, kind: &str) -> &'a mut Value {
    let rules = ruleset["rules"].as_array_mut().expect("rules");
    let rule = rules
        .iter_mut()
        .find(|rule| rule["type"] == kind)
        .expect("rule");
    &mut rule["parameters"]
}

fn set_rule_parameter(ruleset: &mut Value, kind: &str, field: &str, value: Value) {
    rule_parameters(ruleset, kind)[field] = value;
}

fn set_path(value: &mut Value, path: &[&str], new_value: Value) {
    let (last, parents) = path.split_last().expect("non-empty JSON path");
    let parent = parents.iter().fold(value, |parent, key| &mut parent[*key]);
    parent[*last] = new_value;
}

fn copy_assets(root: &Path) {
    for module in builtin_modules() {
        for asset in module.assets {
            let path = root.join(asset.output_path);
            fs::create_dir_all(path.parent().expect("asset parent")).expect("asset directory");
            fs::write(path, asset.contents).expect("copy module asset");
        }
    }
}

fn temporary_root() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("acme-doctor-{}-{timestamp}", std::process::id()))
}
