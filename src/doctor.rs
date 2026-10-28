use crate::{ConfigBinding, FactoryConfig, builtin_modules};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

mod repair;
pub use repair::{checkout_repository, run_doctor_fix};

pub trait GitHubTransport {
    fn get(&self, endpoint: &str) -> Result<Vec<Value>, GitHubError>;

    fn write(
        &self,
        _method: GitHubMethod,
        _endpoint: &str,
        _body: &Value,
    ) -> Result<(), GitHubError> {
        Err(GitHubError::PermissionDenied)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubMethod {
    Post,
    Patch,
    Put,
}

impl GitHubMethod {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Post => "POST",
            Self::Patch => "PATCH",
            Self::Put => "PUT",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHubError {
    AuthenticationRequired,
    PermissionDenied,
    ApiFailure,
}

impl GitHubError {
    const fn description(self) -> &'static str {
        match self {
            Self::AuthenticationRequired => "authentication required",
            Self::PermissionDenied => "insufficient permission",
            Self::ApiFailure => "GitHub API request failed",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Check {
    pub id: String,
    pub passed: bool,
    pub expected: String,
    pub observed: String,
    pub repair: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}

impl DoctorReport {
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }
}

impl std::fmt::Display for DoctorReport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for check in &self.checks {
            let status = if check.passed { "PASS" } else { "FAIL" };
            writeln!(
                formatter,
                "{status} {} | expected: {:?} | observed: {:?} | repair: {}",
                check.id, check.expected, check.observed, check.repair
            )?;
        }
        writeln!(
            formatter,
            "DEFERRED deferred.path-author-enforcement: not implemented."
        )?;
        writeln!(
            formatter,
            "DEFERRED deferred.model-separation: not enforced."
        )?;
        writeln!(
            formatter,
            "DEFERRED deferred.token-scope-verification: token metadata is not verification."
        )
    }
}

#[must_use]
pub fn run_doctor(
    config: &FactoryConfig,
    root: &Path,
    github: &impl GitHubTransport,
) -> DoctorReport {
    let mut report = DoctorReport { checks: Vec::new() };
    check_local(config, root, &mut report);
    let Some((owner, repository)) = split_repository(&config.repository) else {
        push(
            &mut report,
            "github.repository-identity",
            false,
            "owner/repository",
            &config.repository,
            "Set repository to a GitHub owner/repository name in factory.toml.",
        );
        return report;
    };

    let prefix = format!("repos/{owner}/{repository}");
    check_labels(github, &prefix, &mut report);
    check_ruleset(github, &prefix, &mut report);
    check_repository_settings(github, &prefix, &mut report);
    check_actions_settings(github, &prefix, &mut report);
    report
}

fn check_local(config: &FactoryConfig, root: &Path, report: &mut DoctorReport) {
    for module in builtin_modules() {
        let manifest = module.manifest();
        let name = manifest
            .as_ref()
            .map_or("invalid-manifest", |value| value.name.as_str());
        push(
            report,
            &format!("local.module.{name}"),
            manifest.is_ok(),
            "valid built-in module manifest",
            if manifest.is_ok() {
                "valid built-in module manifest"
            } else {
                "invalid built-in module manifest"
            },
            "Review the module manifest against the M2 contract.",
        );
        let binding_valid = manifest
            .as_ref()
            .is_ok_and(|manifest| module_binding_matches(config, module.binding, &manifest.name));
        push(
            report,
            &format!("local.module.{name}.config-binding"),
            binding_valid,
            "module configuration enables its declared binding",
            if binding_valid {
                "module configuration matches its manifest"
            } else {
                "module configuration does not match its manifest"
            },
            "Review the module configuration and manifest together.",
        );

        for asset in module.assets {
            let path = root.join(asset.output_path);
            let contents = fs::read(&path);
            let passed = contents
                .as_ref()
                .is_ok_and(|actual| actual.as_slice() == asset.contents);
            push(
                report,
                &format!("local.asset.{}", stable_path(asset.output_path)),
                passed,
                "matches the bundled module asset",
                if passed {
                    "matches the bundled module asset".to_owned()
                } else if contents.is_err() {
                    "file is missing or unreadable".to_owned()
                } else {
                    "file differs from the bundled module asset".to_owned()
                },
                "Review the local change against its module asset; doctor does not rewrite files.",
            );
        }
    }

    check_ci_contract(config, root, report);
    check_action_pins(root, report);
    check_issue_forms(root, report);
}

fn check_ci_contract(config: &FactoryConfig, root: &Path, report: &mut DoctorReport) {
    let path = root.join(".github/workflows/ci.yml");
    let workflow = fs::read_to_string(path).unwrap_or_default();
    let checks = &config.modules.ci.required_checks;
    let checks_valid = !checks.is_empty()
        && checks
            .iter()
            .all(|check| workflow.contains(&format!("name: {check}")));
    push(
        report,
        "local.ci.required-checks",
        checks_valid,
        "configured required checks appear in the CI workflow",
        format!("{checks:?}"),
        "Review the CI workflow and modules.ci.required_checks together.",
    );

    let commands = &config.modules.ci.commands;
    let commands_valid = !commands.is_empty()
        && commands
            .iter()
            .all(|command| workflow.contains(command.as_str()));
    push(
        report,
        "local.ci.commands",
        commands_valid,
        "all configured CI commands appear in the CI workflow",
        if commands_valid {
            "all configured CI commands are present".to_owned()
        } else {
            "one or more configured CI commands are missing".to_owned()
        },
        "Review the CI workflow and modules.ci.commands together.",
    );
}

fn module_binding_matches(config: &FactoryConfig, binding: ConfigBinding, name: &str) -> bool {
    match binding {
        ConfigBinding::ModuleSection("modules.ci") => name == "ci" && config.modules.ci.enabled,
        ConfigBinding::EnabledDependency("modules.intake") => {
            config.modules.intake.enabled
                && config.modules.intake.needs.iter().any(|need| need == name)
        }
        ConfigBinding::HumanMerge => {
            name == "ruleset"
                && config.modules.merge.enabled
                && config.modules.merge.implementation == "human"
        }
        ConfigBinding::ModuleSection(_) | ConfigBinding::EnabledDependency(_) => false,
    }
}

fn check_action_pins(root: &Path, report: &mut DoctorReport) {
    let mut action_lines: Vec<String> = Vec::new();
    for path in [
        ".github/workflows/ci.yml",
        ".github/actions/setup-rust/action.yml",
    ] {
        if let Ok(contents) = fs::read_to_string(root.join(path)) {
            action_lines.extend(
                contents
                    .lines()
                    .filter(|line| {
                        let line = line.trim_start();
                        line.starts_with("uses:") || line.starts_with("- uses:")
                    })
                    .map(str::to_owned),
            );
        }
    }
    let pinned = !action_lines.is_empty()
        && action_lines.iter().all(|line| {
            let Some((_, action_reference)) = line.split_once("uses:") else {
                return false;
            };
            let action_reference = action_reference
                .split('#')
                .next()
                .unwrap_or_default()
                .trim();
            if action_reference.starts_with("./") {
                return true;
            }
            let Some((reference, ratchet)) = line.split_once("# ratchet:") else {
                return false;
            };
            let Some((_, version)) = reference
                .split_once("uses:")
                .and_then(|(_, reference)| reference.trim().split_once('@'))
            else {
                return false;
            };
            let sha = version.trim();
            sha.len() == 40
                && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
                && !ratchet.trim().is_empty()
        });
    push(
        report,
        "local.ci.action-pins",
        pinned,
        "all CI actions use SHA pins and ratchet comments",
        if pinned {
            "all action references are pinned"
        } else {
            "one or more action references are unpinned"
        },
        "Review action references and ratchet comments in the CI assets.",
    );
}

fn check_issue_forms(root: &Path, report: &mut DoctorReport) {
    let required_fields: [(&str, &[&str]); 3] = [
        ("idea.yml", &["goal", "acceptance", "non-goals"]),
        (
            "bug.yml",
            &["goal", "reproduction", "acceptance", "non-goals"],
        ),
        (
            "spec-request.yml",
            &["goal", "acceptance", "non-goals", "risks"],
        ),
    ];
    let form_assets: Vec<_> = builtin_modules()
        .iter()
        .filter(|module| module.binding == ConfigBinding::EnabledDependency("modules.intake"))
        .flat_map(|module| module.assets)
        .filter(|asset| asset.output_path.starts_with(".github/ISSUE_TEMPLATE/"))
        .collect();
    let mut actual_names: Vec<_> = form_assets
        .iter()
        .filter_map(|asset| asset.output_path.rsplit('/').next())
        .collect();
    actual_names.sort_unstable();
    let mut expected_names = vec!["bug.yml", "config.yml", "idea.yml", "spec-request.yml"];
    expected_names.sort_unstable();
    let inventory_valid = actual_names == expected_names;
    push(
        report,
        "local.issue-forms.inventory",
        inventory_valid,
        format!("{expected_names:?}"),
        format!("{actual_names:?}"),
        "Review the issue-form module inventory.",
    );

    for (filename, fields) in required_fields {
        let path = root.join(".github/ISSUE_TEMPLATE").join(filename);
        let contents = fs::read_to_string(&path);
        let valid = contents.as_ref().is_ok_and(|form| {
            fields
                .iter()
                .all(|field| has_required_form_field(form, field))
        });
        push(
            report,
            &format!("local.issue-form.{}", filename.trim_end_matches(".yml")),
            valid,
            format!("required fields {fields:?}"),
            if contents.is_err() {
                "form is missing or unreadable".to_owned()
            } else if valid {
                format!("required fields {fields:?} are present")
            } else {
                "one or more required fields are missing or optional".to_owned()
            },
            "Review the issue form against its module asset.",
        );
    }

    let config_form = fs::read_to_string(root.join(".github/ISSUE_TEMPLATE/config.yml"));
    let config_valid = config_form.is_ok_and(|form| form.contains("blank_issues_enabled: false"));
    push(
        report,
        "local.issue-form.config",
        config_valid,
        "blank_issues_enabled: false",
        if config_valid {
            "blank issues are disabled"
        } else {
            "blank issue behavior differs or the file is missing"
        },
        "Review the issue-form configuration against its module asset.",
    );
}

fn has_required_form_field(form: &str, field: &str) -> bool {
    let mut in_field = false;
    for line in form.lines() {
        if line.trim_start().starts_with("- type:") {
            if in_field {
                return false;
            }
            in_field = false;
        }
        if line.trim() == format!("id: {field}") {
            in_field = true;
        } else if in_field && line.trim() == "required: true" {
            return true;
        }
    }
    false
}

fn check_labels(github: &impl GitHubTransport, prefix: &str, report: &mut DoctorReport) {
    let Ok(Value::Array(expected)) = asset_json(".github/labels.yml") else {
        push(
            report,
            "github.labels.expected",
            false,
            "valid bundled label data",
            "invalid bundled label data",
            "Review the labels module asset.",
        );
        return;
    };
    let pages = match github.get(&format!("{prefix}/labels?per_page=100")) {
        Ok(pages) => pages,
        Err(error) => {
            api_failure(report, "github.labels.api", error);
            return;
        }
    };
    let observed: Vec<_> = pages
        .iter()
        .flat_map(|page| page.as_array().into_iter().flatten().cloned())
        .collect();

    for label in expected {
        let name = label["name"].as_str().unwrap_or("unknown");
        let matches: Vec<_> = observed
            .iter()
            .filter(|candidate| {
                candidate["name"]
                    .as_str()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
            })
            .collect();
        let expected_fields = json!({
            "name": name,
            "color": label["color"],
            "description": label["description"]
        });
        let actual_fields = matches.first().map(|label| {
            json!({
                "name": label["name"],
                "color": label["color"],
                "description": label["description"]
            })
        });
        let valid = matches.len() == 1
            && actual_fields.as_ref().is_some_and(|actual| {
                actual["name"] == expected_fields["name"]
                    && actual["description"] == expected_fields["description"]
                    && semantic_equal(&expected_fields["color"], &actual["color"])
            });
        push(
            report,
            &format!("github.label.{}", stable_path(name)),
            valid,
            json_description(&expected_fields),
            actual_fields.as_ref().map_or_else(
                || format!("found {} matching labels", matches.len()),
                json_description,
            ),
            "Restore the managed label name, color, and description.",
        );
    }
}

fn check_ruleset(github: &impl GitHubTransport, prefix: &str, report: &mut DoctorReport) {
    let pages = match github.get(&format!("{prefix}/rulesets?per_page=100")) {
        Ok(pages) => pages,
        Err(error) => {
            api_failure(report, "github.ruleset.api", error);
            return;
        }
    };
    let rulesets: Vec<_> = pages
        .iter()
        .flat_map(|page| page.as_array().into_iter().flatten().cloned())
        .collect();
    let matches: Vec<_> = rulesets
        .iter()
        .filter(|ruleset| ruleset["name"].as_str() == Some("main"))
        .collect();
    if matches.len() != 1 {
        push(
            report,
            "github.ruleset.unique-main",
            false,
            "exactly one ruleset named main",
            format!("found {} rulesets named main", matches.len()),
            "Resolve duplicate or missing main rulesets by human-reviewed settings changes.",
        );
        return;
    }
    push(
        report,
        "github.ruleset.unique-main",
        true,
        "exactly one ruleset named main",
        "found exactly one ruleset named main",
        "No repair needed.",
    );
    let Some(id) = matches[0]["id"].as_u64() else {
        push(
            report,
            "github.ruleset.details",
            false,
            "a ruleset ID",
            "the main ruleset has no usable ID",
            "Review the GitHub API response and repository permissions.",
        );
        return;
    };
    let pages = match github.get(&format!("{prefix}/rulesets/{id}")) {
        Ok(pages) => pages,
        Err(error) => {
            api_failure(report, "github.ruleset.details", error);
            return;
        }
    };
    let Some(actual) = single_object(&pages) else {
        push(
            report,
            "github.ruleset.details",
            false,
            "one ruleset object",
            "invalid or ambiguous ruleset response",
            "Retry the read and review repository permissions if the failure continues.",
        );
        return;
    };
    let Ok(expected) = asset_json(".github/rulesets/main.json") else {
        push(
            report,
            "github.ruleset.expected",
            false,
            "valid bundled ruleset data",
            "invalid bundled ruleset data",
            "Review the ruleset module asset.",
        );
        return;
    };
    compare_ruleset(&expected, actual, report);
}

fn compare_ruleset(expected: &Value, actual: &Value, report: &mut DoctorReport) {
    for field in ["name", "target", "enforcement"] {
        compare_field(
            report,
            &format!("github.ruleset.{field}"),
            &expected[field],
            &actual[field],
            "Restore the managed main ruleset field.",
        );
    }
    compare_field(
        report,
        "github.ruleset.no-bypass",
        &json!([]),
        actual.get("bypass_actors").unwrap_or(&Value::Null),
        "Remove bypass actors from the managed main ruleset.",
    );
    compare_field(
        report,
        "github.ruleset.branch-conditions",
        &normalized_array(&expected["conditions"]["ref_name"]),
        &normalized_array(&actual["conditions"]["ref_name"]),
        "Restore the managed main branch conditions.",
    );

    let expected_rules = rule_map(expected);
    let actual_rules = rule_map(actual);
    let expected_types: Vec<_> = expected_rules.keys().copied().collect();
    let actual_types: Vec<_> = actual_rules.keys().copied().collect();
    compare_field(
        report,
        "github.ruleset.rule-types",
        &json!(expected_types),
        &json!(actual_types),
        "Restore the managed rules in the main ruleset.",
    );

    if let (Some(expected_pr), Some(actual_pr)) = (
        expected_rules.get("pull_request"),
        actual_rules.get("pull_request"),
    ) {
        let expected_parameters = &expected_pr["parameters"];
        let actual_parameters = &actual_pr["parameters"];
        for field in [
            "required_approving_review_count",
            "dismiss_stale_reviews_on_push",
            "require_code_owner_review",
            "require_last_push_approval",
            "require_extra_approval_for_unattributed_changes",
            "required_review_thread_resolution",
        ] {
            compare_field(
                report,
                &format!("github.ruleset.pull-request.{field}"),
                &expected_parameters[field],
                &actual_parameters[field],
                "Restore the managed pull-request review setting.",
            );
        }
        compare_field(
            report,
            "github.ruleset.pull-request.merge-methods",
            &normalized_array(&expected_parameters["allowed_merge_methods"]),
            &normalized_array(&actual_parameters["allowed_merge_methods"]),
            "Restore the managed allowed merge methods.",
        );
    }
    if let (Some(expected_checks), Some(actual_checks)) = (
        expected_rules.get("required_status_checks"),
        actual_rules.get("required_status_checks"),
    ) {
        let expected_parameters = &expected_checks["parameters"];
        let actual_parameters = &actual_checks["parameters"];
        for field in [
            "strict_required_status_checks_policy",
            "do_not_enforce_on_create",
        ] {
            compare_field(
                report,
                &format!("github.ruleset.status-checks.{field}"),
                &expected_parameters[field],
                &actual_parameters[field],
                "Restore the managed required-status-check setting.",
            );
        }
        let expected_contexts = status_contexts(&expected_parameters["required_status_checks"]);
        let actual_contexts = status_contexts(&actual_parameters["required_status_checks"]);
        compare_field(
            report,
            "github.ruleset.required-ci",
            &json!(expected_contexts),
            &json!(actual_contexts),
            "Require CI in the main ruleset.",
        );
    }
}

fn check_repository_settings(
    github: &impl GitHubTransport,
    prefix: &str,
    report: &mut DoctorReport,
) {
    let pages = match github.get(prefix) {
        Ok(pages) => pages,
        Err(error) => {
            api_failure(report, "github.repository-settings.api", error);
            return;
        }
    };
    let Some(actual) = single_object(&pages) else {
        push(
            report,
            "github.repository-settings.api",
            false,
            "one repository object",
            "invalid or ambiguous repository response",
            "Retry the read and review repository permissions if the failure continues.",
        );
        return;
    };
    for (field, expected) in [
        ("allow_auto_merge", false),
        ("allow_merge_commit", false),
        ("allow_rebase_merge", false),
        ("allow_squash_merge", true),
        ("delete_branch_on_merge", true),
        ("has_wiki", false),
    ] {
        compare_field(
            report,
            &format!("github.repository.{field}"),
            &json!(expected),
            &actual[field],
            "Restore the documented repository merge settings.",
        );
    }
}

fn check_actions_settings(github: &impl GitHubTransport, prefix: &str, report: &mut DoctorReport) {
    let pages = match github.get(&format!("{prefix}/actions/permissions/workflow")) {
        Ok(pages) => pages,
        Err(error) => {
            api_failure(report, "github.actions-defaults.api", error);
            return;
        }
    };
    let Some(actual) = single_object(&pages) else {
        push(
            report,
            "github.actions-defaults.api",
            false,
            "one Actions permissions object",
            "invalid or ambiguous Actions permissions response",
            "Retry the read and review repository permissions if the failure continues.",
        );
        return;
    };
    for (field, expected) in [
        ("default_workflow_permissions", json!("read")),
        ("can_approve_pull_request_reviews", json!(false)),
    ] {
        compare_field(
            report,
            &format!("github.actions-defaults.{field}"),
            &expected,
            &actual[field],
            "Restore the documented read-only Actions defaults.",
        );
    }
}

fn split_repository(repository: &str) -> Option<(&str, &str)> {
    let (owner, name) = repository.split_once('/')?;
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    };
    (valid_segment(owner) && valid_segment(name) && !name.contains('/')).then_some((owner, name))
}

fn asset_json(path: &str) -> Result<Value, ()> {
    let bytes = builtin_modules()
        .iter()
        .flat_map(|module| module.assets)
        .find(|asset| asset.output_path == path)
        .ok_or(())?
        .contents;
    serde_json::from_slice(bytes).map_err(|_| ())
}

const fn single_object(pages: &[Value]) -> Option<&Value> {
    match pages {
        [Value::Object(_)] => pages.first(),
        _ => None,
    }
}

fn rule_map(value: &Value) -> BTreeMap<&str, &Value> {
    value["rules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|rule| rule["type"].as_str().map(|kind| (kind, rule)))
        .collect()
}

fn status_contexts(statuses: &Value) -> Vec<String> {
    let mut contexts: Vec<_> = statuses
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|status| status["context"].as_str().map(str::to_owned))
        .collect();
    contexts.sort_unstable();
    contexts
}

fn normalized_array(value: &Value) -> Value {
    if let Some(values) = value.as_array() {
        let mut values = values.clone();
        values.sort_by_key(Value::to_string);
        return Value::Array(values);
    }
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    let mut normalized = object.clone();
    for key in ["include", "exclude", "allowed_merge_methods"] {
        if let Some(values) = normalized.get_mut(key).and_then(Value::as_array_mut) {
            values.sort_by_key(Value::to_string);
        }
    }
    Value::Object(normalized)
}

fn semantic_equal(expected: &Value, actual: &Value) -> bool {
    if let (Some(expected), Some(actual)) = (expected.as_str(), actual.as_str()) {
        return expected.eq_ignore_ascii_case(actual);
    }
    expected == actual
}

fn compare_field(
    report: &mut DoctorReport,
    id: &str,
    expected: &Value,
    observed: &Value,
    repair: &str,
) {
    let passed = semantic_equal(expected, observed);
    push(
        report,
        id,
        passed,
        json_description(expected),
        json_description(observed),
        if passed { "No repair needed." } else { repair },
    );
}

fn json_description(value: &Value) -> String {
    value.to_string()
}

fn push(
    report: &mut DoctorReport,
    id: &str,
    passed: bool,
    expected: impl Into<String>,
    observed: impl Into<String>,
    repair: &str,
) {
    report.checks.push(Check {
        id: id.to_owned(),
        passed,
        expected: expected.into(),
        observed: observed.into(),
        repair: repair.to_owned(),
    });
}

fn api_failure(report: &mut DoctorReport, id: &str, error: GitHubError) {
    push(
        report,
        id,
        false,
        "successful read-only GitHub API response",
        error.description(),
        match error {
            GitHubError::AuthenticationRequired => {
                "Authenticate gh with access to the configured repository."
            }
            GitHubError::PermissionDenied => {
                "Ask a repository administrator to grant read access to this setting."
            }
            GitHubError::ApiFailure => "Retry the read and review GitHub API availability.",
        },
    );
}

fn stable_path(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}
