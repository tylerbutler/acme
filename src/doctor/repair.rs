use super::{
    DoctorReport, GitHubError, GitHubMethod, GitHubTransport, asset_json, push,
    run_doctor_with_compiled_inputs, semantic_equal, single_object, split_repository,
};
use crate::FactoryConfig;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

struct Snapshot<'a, T> {
    github: &'a T,
    responses: RefCell<BTreeMap<String, Result<Vec<Value>, GitHubError>>>,
}

impl<T: GitHubTransport> GitHubTransport for Snapshot<'_, T> {
    fn get(&self, endpoint: &str) -> Result<Vec<Value>, GitHubError> {
        if let Some(response) = self.responses.borrow().get(endpoint) {
            return response.clone();
        }
        let response = self.github.get(endpoint);
        self.responses
            .borrow_mut()
            .insert(endpoint.to_owned(), response.clone());
        response
    }
}

struct Repair {
    method: GitHubMethod,
    endpoint: String,
    body: Value,
}

#[must_use]
pub fn run_doctor_fix(
    config: &FactoryConfig,
    root: &Path,
    github: &impl GitHubTransport,
    checkout: Option<&str>,
) -> DoctorReport {
    run_doctor_fix_with_compiled_inputs(config, root, github, checkout, None)
}

/// Apply only existing settings repairs, then check fresh state with the same review inputs.
#[must_use]
pub fn run_doctor_fix_with_compiled_inputs(
    config: &FactoryConfig,
    root: &Path,
    github: &impl GitHubTransport,
    checkout: Option<&str>,
    compiled_inputs: Option<&Path>,
) -> DoctorReport {
    let mut report = DoctorReport { checks: Vec::new() };
    if split_repository(&config.repository).is_none()
        || checkout.is_some_and(|repository| !repository.eq_ignore_ascii_case(&config.repository))
    {
        plan_failure(
            &mut report,
            "The configured repository is invalid or conflicts with the checkout identity.",
        );
        return report;
    }
    let snapshot = Snapshot {
        github,
        responses: RefCell::new(BTreeMap::new()),
    };
    report = run_doctor_with_compiled_inputs(config, root, &snapshot, compiled_inputs);
    let repairs = match build_plan(config, &snapshot, &report) {
        Ok(repairs) => repairs,
        Err(message) => {
            plan_failure(&mut report, &message);
            return report;
        }
    };

    let mut applied = 0;
    let mut failure = None;
    for repair in &repairs {
        if let Err(error) = github.write(repair.method, &repair.endpoint, &repair.body) {
            failure = Some(error);
            break;
        }
        applied += 1;
    }
    // Read fresh state after every attempt. A failed request can still change remote state.
    report = run_doctor_with_compiled_inputs(config, root, github, compiled_inputs);
    push(
        &mut report,
        "repair.plan",
        true,
        "a bounded plan with matching repository identity and administration access",
        format!(
            "validated {} repairs for {}",
            repairs.len(),
            config.repository
        ),
        "Local source changes still need review; doctor does not rewrite files.",
    );
    push(
        &mut report,
        "repair.apply",
        failure.is_none(),
        format!("{} repairs acknowledged", repairs.len()),
        format!(
            "{applied} of {} repairs acknowledged; {}; no rollback was attempted",
            repairs.len(),
            failure.map_or("fresh verification follows".to_owned(), |error| {
                format!(
                    "{}; the failed request may have changed state",
                    error.description()
                )
            })
        ),
        "Review the fresh checks and any partial changes before explicitly retrying --fix.",
    );
    report
}

fn plan_failure(report: &mut DoctorReport, message: &str) {
    push(
        report,
        "repair.plan",
        false,
        "a fully validated repair plan before any writes",
        message,
        "No writes were attempted. Resolve identity, API, administration or ambiguous-state failures, then retry explicitly.",
    );
}

fn build_plan(
    config: &FactoryConfig,
    snapshot: &Snapshot<'_, impl GitHubTransport>,
    report: &DoctorReport,
) -> Result<Vec<Repair>, String> {
    if let Some(error) = snapshot
        .responses
        .borrow()
        .values()
        .find_map(|response| response.as_ref().err().copied())
    {
        return Err(error.description().to_owned());
    }
    let prefix = format!("repos/{}", config.repository);
    let repository = object(snapshot, &prefix)?;
    if !repository["full_name"]
        .as_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(&config.repository))
    {
        return Err("The API repository identity does not match the approved config.".to_owned());
    }
    if repository["permissions"]["admin"] != true {
        return Err("Authenticated repository-administration access is required.".to_owned());
    }
    let mut repairs = Vec::new();
    plan_labels(snapshot, &prefix, &mut repairs)?;
    plan_ruleset(snapshot, &prefix, report, &mut repairs)?;
    plan_settings(
        &prefix,
        GitHubMethod::Patch,
        &repository,
        &json!({
            "allow_auto_merge": false,
            "allow_merge_commit": false,
            "allow_rebase_merge": false,
            "allow_squash_merge": true,
            "delete_branch_on_merge": true,
            "has_wiki": false
        }),
        &mut repairs,
    )?;
    let endpoint = format!("{prefix}/actions/permissions/workflow");
    plan_settings(
        &endpoint,
        GitHubMethod::Put,
        &object(snapshot, &endpoint)?,
        &json!({
            "default_workflow_permissions": "read",
            "can_approve_pull_request_reviews": false
        }),
        &mut repairs,
    )?;
    Ok(repairs)
}

fn plan_labels(
    github: &impl GitHubTransport,
    prefix: &str,
    repairs: &mut Vec<Repair>,
) -> Result<(), String> {
    let expected =
        asset_json(".github/labels.yml").map_err(|()| "Invalid bundled label data.".to_owned())?;
    let expected = expected.as_array().ok_or("Invalid bundled label list.")?;
    let observed = list(github, &format!("{prefix}/labels?per_page=100"))?;
    for label in expected {
        let name = label["name"]
            .as_str()
            .ok_or("Invalid bundled label name.")?;
        let matches: Vec<_> = observed
            .iter()
            .filter(|candidate| {
                candidate["name"]
                    .as_str()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
            })
            .collect();
        let (method, endpoint, body) = match matches.as_slice() {
            [] => (
                GitHubMethod::Post,
                format!("{prefix}/labels"),
                label.clone(),
            ),
            [actual] => {
                let mut body = changed_fields(label, actual, &["color"]);
                if label["description"] != actual["description"] {
                    body["description"] = label["description"].clone();
                }
                if label["name"] != actual["name"] {
                    body["new_name"] = label["name"].clone();
                }
                if body.as_object().is_some_and(serde_json::Map::is_empty) {
                    continue;
                }
                (
                    GitHubMethod::Patch,
                    format!(
                        "{prefix}/labels/{}",
                        encode_segment(actual["name"].as_str().ok_or("Invalid label name.")?)
                    ),
                    body,
                )
            }
            _ => {
                return Err(format!(
                    "Duplicate managed label {name:?}; resolve it manually."
                ));
            }
        };
        repairs.push(Repair {
            method,
            endpoint,
            body,
        });
    }
    Ok(())
}

fn plan_ruleset(
    github: &impl GitHubTransport,
    prefix: &str,
    report: &DoctorReport,
    repairs: &mut Vec<Repair>,
) -> Result<(), String> {
    let rulesets = list(github, &format!("{prefix}/rulesets?per_page=100"))?;
    let matches: Vec<_> = rulesets
        .iter()
        .filter(|ruleset| ruleset["name"] == "main")
        .collect();
    let (method, endpoint) = match matches.as_slice() {
        [] => (GitHubMethod::Post, format!("{prefix}/rulesets")),
        [ruleset] => {
            if ruleset
                .get("source_type")
                .is_some_and(|kind| kind != "Repository")
            {
                return Err("The main ruleset is inherited; ask its owner to repair it.".to_owned());
            }
            let id = ruleset["id"]
                .as_u64()
                .ok_or("The main ruleset has no usable ID.")?;
            if !report
                .checks
                .iter()
                .any(|check| check.id.starts_with("github.ruleset.") && !check.passed)
            {
                return Ok(());
            }
            let endpoint = format!("{prefix}/rulesets/{id}");
            object(github, &endpoint)?;
            (GitHubMethod::Put, endpoint)
        }
        _ => return Err("Duplicate managed main rulesets; resolve them manually.".to_owned()),
    };
    let body = asset_json(".github/rulesets/main.json")
        .map_err(|()| "Invalid bundled main ruleset data.".to_owned())?;
    repairs.push(Repair {
        method,
        endpoint,
        body,
    });
    Ok(())
}

fn plan_settings(
    endpoint: &str,
    method: GitHubMethod,
    actual: &Value,
    expected: &Value,
    repairs: &mut Vec<Repair>,
) -> Result<(), String> {
    let fields = expected.as_object().ok_or("Invalid managed settings.")?;
    for (field, value) in fields {
        if (value.is_boolean() && !actual[field].is_boolean())
            || (value.is_string() && !actual[field].is_string())
        {
            return Err(format!(
                "Missing or invalid setting {field:?} at {endpoint}."
            ));
        }
    }
    let names: Vec<_> = fields.keys().map(String::as_str).collect();
    let body = changed_fields(expected, actual, &names);
    if body.as_object().is_some_and(|fields| !fields.is_empty()) {
        repairs.push(Repair {
            method,
            endpoint: endpoint.to_owned(),
            body,
        });
    }
    Ok(())
}

fn changed_fields(expected: &Value, actual: &Value, fields: &[&str]) -> Value {
    let mut changes = serde_json::Map::new();
    for field in fields {
        if !semantic_equal(&expected[field], &actual[field]) {
            changes.insert((*field).to_owned(), expected[field].clone());
        }
    }
    Value::Object(changes)
}

fn object(github: &impl GitHubTransport, endpoint: &str) -> Result<Value, String> {
    let pages = github
        .get(endpoint)
        .map_err(|error| error.description().to_owned())?;
    single_object(&pages)
        .cloned()
        .ok_or_else(|| format!("Invalid or ambiguous object response at {endpoint}."))
}

fn list(github: &impl GitHubTransport, endpoint: &str) -> Result<Vec<Value>, String> {
    let pages = github
        .get(endpoint)
        .map_err(|error| error.description().to_owned())?;
    if pages.is_empty() || pages.iter().any(|page| !page.is_array()) {
        return Err(format!("Invalid paginated list at {endpoint}."));
    }
    let items: Vec<_> = pages
        .into_iter()
        .flat_map(|page| page.as_array().cloned().unwrap_or_default())
        .collect();
    if items.iter().any(|item| item["name"].as_str().is_none()) {
        return Err(format!("Invalid named resource at {endpoint}."));
    }
    Ok(items)
}

fn encode_segment(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

pub fn checkout_repository(root: &Path) -> Result<Option<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|_| "Cannot inspect checkout identity; no repairs were attempted.")?;
    if !output.status.success() {
        if String::from_utf8_lossy(&output.stderr).contains("not a git repository") {
            return Ok(None);
        }
        return Err("Cannot inspect checkout identity; no repairs were attempted.".to_owned());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "get-url", "--all", "origin"])
        .output()
        .map_err(|_| "Cannot read origin; no repairs were attempted.")?;
    if !output.status.success() {
        return Err("The checkout needs an identifiable GitHub origin before --fix.".to_owned());
    }
    let urls = String::from_utf8(output.stdout)
        .map_err(|_| "Invalid origin encoding; no repairs were attempted.")?;
    let mut identities = urls.lines().map(repository_from_url);
    let identity = identities
        .next()
        .flatten()
        .ok_or("The checkout origin is not an identifiable github.com repository.")?;
    if identities.any(|candidate| {
        !candidate.is_some_and(|candidate| candidate.eq_ignore_ascii_case(identity))
    }) {
        return Err("Conflicting checkout origins; no repairs were attempted.".to_owned());
    }
    Ok(Some(identity.to_owned()))
}

fn repository_from_url(url: &str) -> Option<&str> {
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let repository = path
        .trim_end_matches('/')
        .strip_suffix(".git")
        .unwrap_or_else(|| path.trim_end_matches('/'));
    split_repository(repository)?;
    Some(repository)
}
