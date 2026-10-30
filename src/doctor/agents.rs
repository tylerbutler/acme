use super::{DoctorReport, GitHubTransport, api_failure, push, single_object, stable_path};
use crate::FactoryConfig;
use crate::agent_catalog::AgentPathKind;
use crate::render::{
    ValidatedCompiledInputs, build_agent_source_plan, read_regular, validate_compiled_inputs,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const REVIEW: &str =
    "Review approved config and local files; doctor does not compile or rewrite agent assets.";
const CONSENT: &str = "Ask the owner to review drift. Activation, assignment, secrets and budgets need separate approval; --fix does not repair them.";

pub(super) fn check_local(
    config: &FactoryConfig,
    root: &Path,
    compiled_inputs: Option<&Path>,
    report: &mut DoctorReport,
) {
    if !config.modules.intake.enabled && !config.modules.build_cloud.enabled {
        return;
    }
    // Validate the root and authored files before the source planner reads them.
    let prerequisites = if config.modules.build_cloud.enabled {
        &["AGENTS.md", "commit-types.json"][..]
    } else {
        &["AGENTS.md"][..]
    };
    let safe = prerequisites
        .iter()
        .all(|path| matches!(read_regular(&root.join(path)), Ok(Some(_))));
    let plan = if safe {
        build_agent_source_plan(config, root).map_err(|error| error.to_string())
    } else {
        Err("authored prerequisites are missing, unreadable or unsafe; require regular AGENTS.md and, for Build-cloud, commit-types.json".to_owned())
    };
    push(
        report,
        "local.agent.source-plan",
        plan.is_ok(),
        "valid approved agent source plan and authored prerequisites",
        plan.as_ref()
            .map_or_else(Clone::clone, |_| "validated source plan".to_owned()),
        REVIEW,
    );
    if let Ok(plan) = &plan {
        check_activation_config(config, report);
        for output in &plan.outputs {
            compare_asset(root, output.path, &output.contents, report);
        }
    }
    let validated =
        compiled_inputs.map(|directory| validate_compiled_inputs(config, root, directory));
    let observed = match &validated {
        None => "compiled inputs were not supplied".to_owned(),
        Some(Ok(_)) => "validated reviewed input inventory and captured bytes".to_owned(),
        Some(Err(error)) => error.to_string(),
    };
    push(
        report,
        "local.agent.compiled-inputs",
        matches!(&validated, Some(Ok(_))),
        "reviewed compiled inputs matching approved config, versions, inventory and hashes",
        observed,
        "Supply --compiled-inputs DIR after explicit compilation and human review. Doctor never invokes the compiler. Hashes do not prove compiler trust.",
    );
    if let Some(Ok(validated)) = &validated {
        for output in &validated.outputs {
            if output.ownership.kind == AgentPathKind::CompilerOutput {
                compare_asset(root, output.ownership.path, &output.contents, report);
            }
        }
    }
    check_models(
        config,
        validated.as_ref().and_then(|result| result.as_ref().ok()),
        report,
    );
}

fn check_activation_config(config: &FactoryConfig, report: &mut DoctorReport) {
    if config.modules.intake.enabled {
        push(
            report,
            "local.agent.intake.activation-config",
            true,
            "report approved configuration; exact source-byte checks are separate",
            format!(
                "workflow_enabled={}; effective source gate={}; staged={}",
                config.modules.intake.workflow_enabled,
                config.modules.intake.workflow_enabled,
                config.modules.intake.staged
            ),
            CONSENT,
        );
    }
    if config.modules.build_cloud.enabled {
        push(
            report,
            "local.agent.build-cloud.activation-config",
            true,
            "report approved configuration; exact source-byte checks are separate",
            format!(
                "workflow_enabled={}; assignment_enabled={}; effective source gate={}; staged={}",
                config.modules.build_cloud.workflow_enabled,
                config.modules.build_cloud.assignment_enabled,
                config.modules.build_cloud.workflow_enabled
                    && config.modules.build_cloud.assignment_enabled,
                config.modules.build_cloud.staged
            ),
            CONSENT,
        );
    }
}

fn check_models(
    config: &FactoryConfig,
    validated: Option<&ValidatedCompiledInputs>,
    report: &mut DoctorReport,
) {
    for (name, enabled, controller, detection) in [
        (
            "intake",
            config.modules.intake.enabled,
            &config.engines.intake.controller_model,
            &config.engines.intake.detection_model,
        ),
        (
            "build-cloud",
            config.modules.build_cloud.enabled,
            &config.engines.build_cloud.controller_model,
            &config.engines.build_cloud.detection_model,
        ),
    ] {
        if !enabled {
            continue;
        }
        for (role, model) in [("controller", controller), ("detection", detection)] {
            information(
                report,
                &format!("agent.models.{name}.{role}.configured"),
                model
                    .as_ref()
                    .map_or("omitted; gh-aw runtime defaults", |model| model.as_str()),
            );
        }
        let lock_path = format!(".github/workflows/{name}.lock.yml");
        let lock = validated
            .and_then(|inputs| {
                inputs
                    .outputs
                    .iter()
                    .find(|output| output.ownership.path == lock_path)
            })
            .and_then(|output| std::str::from_utf8(&output.contents).ok());
        for (role, job) in [("controller", "agent"), ("detection", "detection")] {
            let selection = lock.and_then(|lock| environment_selection(lock, job, "COPILOT_MODEL"));
            information(
                report,
                &format!("agent.models.{name}.{role}.runtime-selection"),
                selection.unwrap_or("unavailable; no unambiguous validated runtime selection"),
            );
        }
        let fallback =
            lock.and_then(|lock| environment_selection(lock, "detection", "GH_AW_MODEL_FALLBACK"));
        information(
            report,
            &format!("agent.models.{name}.detection.runtime-fallback"),
            fallback.unwrap_or("unavailable; no validated runtime fallback"),
        );
        if name == "build-cloud" {
            information(
                report,
                "agent.models.build-cloud.builder.configured",
                &config.engines.build_cloud.model,
            );
            let builder = lock.and_then(builder_selection);
            information(
                report,
                "agent.models.build-cloud.builder.runtime-selection",
                builder
                    .as_deref()
                    .unwrap_or("unavailable; no validated assignment model selection"),
            );
        }
        information(
            report,
            &format!("agent.models.{name}.usage-evidence"),
            "observed usage evidence unavailable; serving model unknown for every role; model separation is not verified",
        );
    }
}

fn builder_selection(lock: &str) -> Option<String> {
    let encoded = environment_selection(lock, "safe_outputs", "GH_AW_SAFE_OUTPUTS_HANDLER_CONFIG")?;
    let decoded: String = serde_json::from_str(encoded).ok()?;
    let handler: Value = serde_json::from_str(&decoded).ok()?;
    handler["assign_to_agent"]["model"]
        .as_str()
        .map(str::to_owned)
}

fn environment_selection<'a>(lock: &'a str, job: &str, variable: &str) -> Option<&'a str> {
    let (_, body) = lock.split_once(&format!("\n  {job}:\n"))?;
    let prefix = format!("{variable}: ");
    let mut values = body
        .lines()
        .take_while(|line| {
            !line.starts_with("  ") || line.as_bytes().get(2).is_none_or(u8::is_ascii_whitespace)
        })
        .filter_map(|line| line.trim().strip_prefix(&prefix));
    let value = values.next()?;
    values.next().is_none().then_some(value)
}

fn information(report: &mut DoctorReport, id: &str, observed: &str) {
    push(
        report,
        id,
        true,
        "report selection metadata only, not serving-model or usage verification",
        observed,
        "No inference or workflow-log requests are made. Configured IDs and runtime expressions do not prove serving models or separation.",
    );
}

fn compare_asset(root: &Path, path: &str, expected: &[u8], report: &mut DoctorReport) {
    let actual = read_regular(&root.join(path));
    let passed = actual
        .as_ref()
        .is_ok_and(|actual| actual.as_deref() == Some(expected));
    push(
        report,
        &format!("local.agent.asset.{}", stable_path(path)),
        passed,
        "exact approved source/helper bytes or validated compiled lock bytes",
        if passed {
            "exact bytes match"
        } else if matches!(actual, Ok(Some(_))) {
            "file bytes differ"
        } else {
            "file is missing, unreadable or unsafe; require regular files and directory components"
        },
        REVIEW,
    );
}

pub(super) fn check_remote(
    config: &FactoryConfig,
    github: &impl GitHubTransport,
    prefix: &str,
    report: &mut DoctorReport,
) {
    let intake = &config.modules.intake;
    let build = &config.modules.build_cloud;
    if !intake.enabled && !build.enabled {
        return;
    }
    for (name, enabled, active) in [
        ("intake", intake.enabled, intake.workflow_enabled),
        ("build-cloud", build.enabled, build.workflow_enabled),
    ] {
        if enabled {
            check_workflow(github, prefix, name, active, report);
        }
    }
    check_variables(config, github, prefix, report);
    check_secrets(config, github, prefix, report);
}

fn check_workflow(
    github: &impl GitHubTransport,
    prefix: &str,
    name: &str,
    active: bool,
    report: &mut DoctorReport,
) {
    let id = format!("github.agent.{name}.workflow-state");
    let pages = match github.get(&format!("{prefix}/actions/workflows/{name}.lock.yml")) {
        Ok(pages) => pages,
        Err(error) => {
            api_failure(report, &id, error);
            return;
        }
    };
    let state = single_object(&pages).and_then(|object| object["state"].as_str());
    let passed = if active {
        state == Some("active")
    } else {
        matches!(
            state,
            Some("disabled_manually" | "disabled_inactivity" | "disabled_fork")
        )
    };
    push(
        report,
        &id,
        passed,
        if active {
            "active"
        } else {
            "disabled_manually, disabled_inactivity or disabled_fork"
        },
        state
            .filter(|state| {
                matches!(
                    *state,
                    "active"
                        | "disabled_manually"
                        | "disabled_inactivity"
                        | "disabled_fork"
                        | "deleted"
                )
            })
            .unwrap_or("invalid or ambiguous workflow state response"),
        CONSENT,
    );
}

fn check_variables(
    config: &FactoryConfig,
    github: &impl GitHubTransport,
    prefix: &str,
    report: &mut DoctorReport,
) {
    let id = "github.agent.variables";
    let pages = match github.get(&format!("{prefix}/actions/variables?per_page=100")) {
        Ok(pages) => pages,
        Err(error) => {
            api_failure(report, id, error);
            return;
        }
    };
    let Some(variables) = variable_values(&pages) else {
        push(
            report,
            id,
            false,
            "complete paginated variable metadata",
            "invalid, incomplete or ambiguous variable response",
            CONSENT,
        );
        return;
    };
    push(
        report,
        id,
        true,
        "complete paginated variable metadata",
        "validated complete paginated variable metadata",
        CONSENT,
    );
    check_variable(
        &variables,
        "ACME_AGENT_WORKFLOWS_ENABLED",
        config.modules.intake.enabled && config.modules.intake.workflow_enabled
            || config.modules.build_cloud.enabled && config.modules.build_cloud.workflow_enabled,
        report,
    );
    if config.modules.build_cloud.enabled {
        check_variable(
            &variables,
            "ACME_CLOUD_ASSIGNMENT_ENABLED",
            config.modules.build_cloud.assignment_enabled,
            report,
        );
    }
}

fn check_variable(
    variables: &BTreeMap<String, &str>,
    name: &str,
    enabled: bool,
    report: &mut DoctorReport,
) {
    let value = variables.get(name).copied();
    let passed = if enabled {
        value == Some("true")
    } else {
        matches!(value, None | Some("false"))
    };
    push(
        report,
        &format!("github.agent.variable.{name}"),
        passed,
        if enabled { "true" } else { "false or absent" },
        match value {
            Some("true") => "true",
            Some("false") => "false",
            Some(_) => "unexpected variable value",
            None => "absent; workflow gate is off",
        },
        CONSENT,
    );
}

fn variable_values(pages: &[Value]) -> Option<BTreeMap<String, &str>> {
    let total = pages.first()?.get("total_count")?.as_u64()?;
    let mut variables = BTreeMap::new();
    for page in pages {
        if page.get("total_count")?.as_u64()? != total {
            return None;
        }
        for variable in page.get("variables")?.as_array()? {
            let name = variable.get("name")?.as_str()?;
            let value = variable.get("value")?.as_str()?;
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || variables.insert(name.to_ascii_uppercase(), value).is_some()
            {
                return None;
            }
        }
    }
    (u64::try_from(variables.len()).ok()? == total).then_some(variables)
}

fn check_secrets(
    config: &FactoryConfig,
    github: &impl GitHubTransport,
    prefix: &str,
    report: &mut DoctorReport,
) {
    let id = "github.agent.secret-names";
    let pages = match github.get(&format!("{prefix}/actions/secrets?per_page=100")) {
        Ok(pages) => pages,
        Err(error) => {
            api_failure(report, id, error);
            return;
        }
    };
    let Some(names) = secret_names(&pages) else {
        push(
            report,
            id,
            false,
            "complete paginated secret-name metadata",
            "invalid, incomplete or ambiguous secret-name response",
            CONSENT,
        );
        return;
    };
    push(
        report,
        id,
        true,
        "repository secret names only",
        format!("{names:?}; presence does not verify scopes or assignment capability"),
        CONSENT,
    );
    let mut required = Vec::new();
    if config.modules.intake.enabled {
        required.push(("intake.inference", &config.engines.intake.token_secret));
    }
    if config.modules.build_cloud.enabled {
        required.push((
            "build-cloud.inference",
            &config.engines.build_cloud.token_secret,
        ));
        required.push((
            "build-cloud.assignment",
            &config.modules.build_cloud.token_secret,
        ));
    }
    for (role, name) in required {
        let present = names.contains(&name.to_ascii_uppercase());
        push(
            report,
            &format!("github.agent.secret.{role}"),
            present,
            format!("configured secret name {name} is present"),
            if present {
                "name is present; scopes and assignment capability are unverified"
            } else {
                "configured secret name is absent"
            },
            CONSENT,
        );
    }
}

fn secret_names(pages: &[Value]) -> Option<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    let total = pages.first()?.get("total_count")?.as_u64()?;
    for page in pages {
        if page.get("total_count")?.as_u64()? != total {
            return None;
        }
        for secret in page.get("secrets")?.as_array()? {
            let name = secret.get("name")?.as_str()?;
            if name.is_empty()
                || name.len() > 255
                || name.starts_with(|character: char| character.is_ascii_digit())
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || !names.insert(name.to_ascii_uppercase())
            {
                return None;
            }
        }
    }
    (u64::try_from(names.len()).ok()? == total).then_some(names)
}

#[cfg(test)]
mod tests {
    use super::environment_selection;

    #[test]
    fn model_selection_uses_job_names_when_detection_appears_first() {
        let lock = "jobs:\n  detection:\n    env:\n      COPILOT_MODEL: detector-choice\n  agent:\n    env:\n      COPILOT_MODEL: controller-choice\n";
        assert_eq!(
            environment_selection(lock, "agent", "COPILOT_MODEL"),
            Some("controller-choice")
        );
        assert_eq!(
            environment_selection(lock, "detection", "COPILOT_MODEL"),
            Some("detector-choice")
        );
    }

    #[test]
    fn missing_model_selection_does_not_use_another_jobs_value() {
        for (lock, controller, detection) in [
            (
                "jobs:\n  agent:\n    env:\n      OTHER: none\n  detection:\n    env:\n      COPILOT_MODEL: detector-choice\n",
                None,
                Some("detector-choice"),
            ),
            (
                "jobs:\n  agent:\n    env:\n      COPILOT_MODEL: controller-choice\n  detection:\n    env:\n      OTHER: none\n",
                Some("controller-choice"),
                None,
            ),
            (
                "jobs:\n  detection:\n    env:\n      COPILOT_MODEL: detector-choice\n",
                None,
                Some("detector-choice"),
            ),
        ] {
            assert_eq!(
                environment_selection(lock, "agent", "COPILOT_MODEL"),
                controller
            );
            assert_eq!(
                environment_selection(lock, "detection", "COPILOT_MODEL"),
                detection
            );
        }
    }
}
