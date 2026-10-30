use super::{RenderPlanError, build_render_plan, dependency_order, normalize};
use crate::agent_catalog::{AgentModule, AgentOwnedPath, AgentPathKind, agent_modules};
use crate::config::ModelId;
use crate::{FactoryConfig, ModuleManifest};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const COMPILER_VERSION: &str = "v0.93.5";
// gh-aw v0.93.5 threat_detection_helpers.go and copilot_engine_execution.go.
// An explicit expression avoids controller inheritance and retains the auto fallback.
const DETECTION_SELECTION: &str =
    "${{ vars.GH_AW_MODEL_DETECTION_COPILOT || vars.GH_AW_DEFAULT_MODEL_COPILOT || 'detection' }}";

/// Source bytes and future ownership only, not a compiled-input or checkout plan.
#[derive(Debug, Eq, PartialEq)]
pub struct AgentSourcePlan {
    pub acme_version: &'static str,
    pub gh_aw_version: String,
    /// Includes M2 capability providers and human stages in dependency order.
    pub modules: Vec<ModuleManifest>,
    pub outputs: Vec<AgentSourceOutput>,
    /// Compiler outputs have ownership metadata, but never fabricated bytes.
    pub ownership: Vec<AgentOwnedPath>,
    /// Required on the workflow runner, not probed or executed during planning.
    pub runtime_tools: Vec<&'static str>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct AgentSourceOutput {
    pub module: AgentModule,
    pub path: &'static str,
    pub contents: Vec<u8>,
}

/// Plans bundled sources after read-only validation of authored prerequisites.
///
/// This API does not compile, publish, activate or synchronize agent workflows.
pub fn build_agent_source_plan(
    config: &FactoryConfig,
    repository: impl AsRef<Path>,
) -> Result<AgentSourcePlan, RenderPlanError> {
    let m2 = build_render_plan(config)?;
    validate_agent_config(config)?;
    let mut manifests: BTreeMap<_, _> = m2
        .modules
        .into_iter()
        .chain(m2.external_stages)
        .map(|manifest| (manifest.name.clone(), manifest))
        .collect();
    let mut plan = AgentSourcePlan {
        acme_version: m2.acme_version,
        gh_aw_version: config.toolchain.gh_aw.version.clone(),
        modules: Vec::new(),
        outputs: Vec::new(),
        ownership: Vec::new(),
        runtime_tools: Vec::new(),
    };
    let mut prerequisites = BTreeSet::new();
    let mut runtime_tools = BTreeSet::new();
    for module in agent_modules() {
        let Some(manifest) = manifests.get_mut(module.module.name()) else {
            continue;
        };
        let declared = module
            .manifest()
            .map_err(|error| RenderPlanError(error.to_string()))?;
        if manifest.gate != declared.gate {
            return Err(error(format!(
                "unsupported {} gate: require {}",
                manifest.name, declared.gate
            )));
        }
        manifest.needs.extend(declared.needs);
        manifest.provides.extend(declared.provides);
        normalize(manifest)?;
        prerequisites.extend(module.authored_prerequisites.iter().copied());
        runtime_tools.extend(module.runtime_tools.iter().copied());
        plan.ownership.extend_from_slice(module.ownership);
    }
    let nodes = manifests
        .iter()
        .map(|(name, manifest)| (name.clone(), manifest))
        .collect();
    let order = dependency_order(&nodes)?;
    let mut example_type = "feat".to_owned();
    for prerequisite in prerequisites {
        let authored = read_authored(repository.as_ref(), prerequisite)?;
        if prerequisite == "commit-types.json" {
            example_type = validate_commit_types(&authored)?;
        }
    }
    for name in order {
        let manifest = manifests
            .remove(&name)
            .ok_or_else(|| error(format!("missing planned module {name}")))?;
        if let Some(module) = agent_modules()
            .iter()
            .find(|module| module.module.name() == name)
        {
            for asset in module.assets {
                let workflow = module.ownership.iter().any(|entry| {
                    entry.path == asset.output_path && entry.kind == AgentPathKind::WorkflowSource
                });
                let contents = if workflow {
                    render_workflow(config, module.module, asset.contents, &example_type)?
                } else {
                    asset.contents.to_vec()
                };
                plan.outputs.push(AgentSourceOutput {
                    module: module.module,
                    path: asset.output_path,
                    contents,
                });
            }
        }
        plan.modules.push(manifest);
    }
    plan.outputs.sort_by_key(|output| output.path);
    plan.ownership.sort_by_key(|entry| entry.path);
    plan.runtime_tools = runtime_tools.into_iter().collect();
    Ok(plan)
}

fn validate_agent_config(config: &FactoryConfig) -> Result<(), RenderPlanError> {
    let intake = &config.modules.intake;
    let build = &config.modules.build_cloud;
    if !intake.enabled && !build.enabled {
        return Ok(());
    }
    if config.toolchain.gh_aw.version != COMPILER_VERSION {
        return Err(error(format!(
            "agent sources require toolchain.gh_aw.version = {COMPILER_VERSION}"
        )));
    }
    if config.autonomy != "L3" || config.engines.review.enabled {
        return Err(error(
            "agent sources require L3 autonomy and disabled review",
        ));
    }
    if build.enabled
        && (!config.modules.spec.enabled
            || !config
                .modules
                .spec
                .provides
                .iter()
                .any(|capability| capability == "approved-spec"))
        || config.modules.spec.enabled
            && (config.modules.spec.implementation != "human"
                || config.modules.spec.gate != "owner-ready-for-agent")
    {
        return Err(error(
            "agent sources require the human spec implementation and owner-ready-for-agent gate; Build-cloud needs the enabled approved-spec provider",
        ));
    }
    if config.budget.enforce_monthly
        || !config.budget.enforce_ai_credits
        || config.budget.max_ai_credits_per_run == 0
    {
        return Err(error(
            "agent sources require a positive per-run AI-credit limit and no monthly enforcement",
        ));
    }
    if intake.enabled {
        validate_engine(
            "engines.intake",
            &config.engines.intake.engine,
            &config.engines.intake.token_secret,
        )?;
        if intake.roles.is_empty()
            || intake.roles.len() > 50
            || intake.roles.iter().any(|role| {
                !matches!(
                    role.as_str(),
                    "admin" | "maintainer" | "maintain" | "write" | "triage" | "read"
                )
            })
        {
            return Err(error(
                "modules.intake.roles must contain 1..50 repository roles: admin, maintainer, maintain, write, triage or read",
            ));
        }
    }
    if build.enabled {
        validate_engine(
            "engines.build_cloud",
            &config.engines.build_cloud.engine,
            &config.engines.build_cloud.token_secret,
        )?;
        validate_secret("modules.build_cloud.token_secret", &build.token_secret)?;
        ModelId::validate(&config.engines.build_cloud.model)
            .map_err(|message| error(format!("engines.build_cloud.model: {message}")))?;
        if build.max_repositories != 1
            || build.max_pull_requests != 1
            || !(1..=59).contains(&build.target_minutes)
        {
            return Err(error(
                "Build-cloud requires one repository, one draft PR and a target of 1..59 minutes; the target is instructional, not an enforced runtime limit",
            ));
        }
    }
    Ok(())
}

fn validate_engine(field: &str, engine: &str, secret: &str) -> Result<(), RenderPlanError> {
    if engine != "copilot" {
        return Err(error(format!("{field}.engine supports only copilot")));
    }
    validate_secret(&format!("{field}.token_secret"), secret)
}

fn validate_secret(field: &str, value: &str) -> Result<(), RenderPlanError> {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with(|character: char| character.is_ascii_digit())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || value.to_ascii_uppercase().starts_with("GITHUB_")
    {
        return Err(error(format!(
            "{field} must be a GitHub secret name: ASCII letters, digits or underscores, not starting with a digit or GITHUB_"
        )));
    }
    Ok(())
}

fn read_authored(root: &Path, relative: &str) -> Result<String, RenderPlanError> {
    let path = root.join(relative);
    let guidance = || {
        format!(
            "author {relative} as a nonempty UTF-8 regular file before planning agent sources; acme never generates or overwrites it"
        )
    };
    let metadata =
        fs::symlink_metadata(&path).map_err(|source| error(format!("{}: {source}", guidance())))?;
    if !metadata.file_type().is_file() {
        return Err(error(guidance()));
    }
    let contents =
        fs::read_to_string(path).map_err(|source| error(format!("{}: {source}", guidance())))?;
    if contents.trim().is_empty() {
        return Err(error(guidance()));
    }
    Ok(contents)
}

fn validate_commit_types(input: &str) -> Result<String, RenderPlanError> {
    let invalid = || {
        error(
            "author valid commit-types.json: a nonempty types object with safe lowercase type names, nonempty description strings and string-or-null changelog_group fields",
        )
    };
    let value: serde_json::Value = serde_json::from_str(input).map_err(|_| invalid())?;
    let types = value
        .get("types")
        .and_then(serde_json::Value::as_object)
        .filter(|types| !types.is_empty())
        .ok_or_else(invalid)?;
    for (name, definition) in types {
        if name.len() > 32
            || !name.starts_with(|character: char| character.is_ascii_lowercase())
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || definition
                .get("description")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|description| description.trim().is_empty())
            || !matches!(
                definition.get("changelog_group"),
                Some(serde_json::Value::Null)
            ) && definition
                .get("changelog_group")
                .and_then(serde_json::Value::as_str)
                .is_none_or(|group| group.trim().is_empty())
        {
            return Err(invalid());
        }
    }
    if types.contains_key("feat") {
        return Ok("feat".to_owned());
    }
    types.keys().next().cloned().ok_or_else(invalid)
}

fn render_workflow(
    config: &FactoryConfig,
    module: AgentModule,
    template: &[u8],
    example_type: &str,
) -> Result<Vec<u8>, RenderPlanError> {
    let mut source = std::str::from_utf8(template)
        .map_err(|source| error(format!("bundled agent source is not UTF-8: {source}")))?
        .to_owned();
    let (controller, detection, token, staged, enabled) = match module {
        AgentModule::Intake => {
            if config.modules.intake.roles != ["admin"] {
                replace(
                    &mut source,
                    "  roles: [admin]\n",
                    &format!("  roles: {}\n", json(&config.modules.intake.roles)?),
                )?;
            }
            (
                &config.engines.intake.controller_model,
                &config.engines.intake.detection_model,
                &config.engines.intake.token_secret,
                config.modules.intake.staged,
                config.modules.intake.workflow_enabled,
            )
        }
        AgentModule::BuildCloud => {
            render_build_inputs(config, &mut source, example_type)?;
            (
                &config.engines.build_cloud.controller_model,
                &config.engines.build_cloud.detection_model,
                &config.engines.build_cloud.token_secret,
                config.modules.build_cloud.staged,
                config.modules.build_cloud.workflow_enabled
                    && config.modules.build_cloud.assignment_enabled,
            )
        }
    };
    replace(
        &mut source,
        "    COPILOT_GITHUB_TOKEN: ${{ secrets.GH_PAT }}\n",
        &format!("    COPILOT_GITHUB_TOKEN: ${{{{ secrets.{token} }}}}\n"),
    )?;
    replace(
        &mut source,
        "max-ai-credits: 5000\n",
        &format!("max-ai-credits: {}\n", config.budget.max_ai_credits_per_run),
    )?;
    replace(
        &mut source,
        "  staged: false\n",
        &format!("  staged: {staged}\n"),
    )?;
    if !enabled {
        match module {
            AgentModule::Intake => replace(
                &mut source,
                "if: vars.ACME_AGENT_WORKFLOWS_ENABLED",
                "if: false && vars.ACME_AGENT_WORKFLOWS_ENABLED",
            )?,
            AgentModule::BuildCloud => {
                replace(&mut source, "if: >-\n", "if: >-\n  false &&\n")?;
            }
        }
    }
    if let Some(model) = controller {
        replace(
            &mut source,
            "engine:\n  id: copilot\n",
            &format!(
                "engine:\n  id: copilot\n  model: {}\n",
                json(model.as_str())?
            ),
        )?;
    }
    let detection_selection = match detection {
        Some(model) => Some(json(model.as_str())?),
        None if controller.is_some() => Some(DETECTION_SELECTION.to_owned()),
        None => None,
    };
    if let Some(model) = detection_selection {
        replace(
            &mut source,
            "safe-outputs:\n",
            &format!("safe-outputs:\n  threat-detection:\n    model: {model}\n"),
        )?;
    }
    Ok(source.into_bytes())
}

fn render_build_inputs(
    config: &FactoryConfig,
    source: &mut String,
    example_type: &str,
) -> Result<(), RenderPlanError> {
    let builder = &config.engines.build_cloud.model;
    if builder != "gpt-6.1-sol" {
        replace(
            source,
            "    model: gpt-6.1-sol\n",
            &format!("    model: {}\n", json(builder)?),
        )?;
    }
    replace(
        source,
        "    github-token: ${{ secrets.GH_PAT }}\n",
        &format!(
            "    github-token: ${{{{ secrets.{} }}}}\n",
            config.modules.build_cloud.token_secret
        ),
    )?;
    replace(
        source,
        "under 59 minutes",
        &format!(
            "under {} minutes",
            config.modules.build_cloud.target_minutes
        ),
    )?;
    replace(
        source,
        "\"feat: add module manifests\"",
        &format!("\"{example_type}: add module manifests\""),
    )
}

fn json(value: &(impl serde::Serialize + ?Sized)) -> Result<String, RenderPlanError> {
    serde_json::to_string(value)
        .map_err(|source| error(format!("cannot serialize agent source value: {source}")))
}

fn replace(source: &mut String, from: &str, to: &str) -> Result<(), RenderPlanError> {
    if source.matches(from).count() != 1 {
        return Err(error(format!("bundled agent source requires one {from:?}")));
    }
    *source = source.replacen(from, to, 1);
    Ok(())
}

fn error(message: impl Into<String>) -> RenderPlanError {
    RenderPlanError(message.into())
}
