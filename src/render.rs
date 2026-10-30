use crate::{BuiltinModule, ConfigBinding, FactoryConfig, ModuleManifest, builtin_modules};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

mod agent_sources;
mod compiled;
mod init;
mod output;
mod sync;
pub use agent_sources::{AgentSourceOutput, AgentSourcePlan, build_agent_source_plan};
pub use compiled::{
    CompileError, CompiledAgentOutput, ValidatedCompiledInputs, compile_agent_workflows,
    validate_compiled_inputs,
};
pub use output::{RenderError, render_to_directory, render_to_directory_with_compiled_inputs};
pub(crate) use sync::read_regular;
pub use sync::{
    SyncError, SyncReport, init_repository, sync_repository, sync_repository_with_compiled_inputs,
};

struct BytePlan {
    acme_version: &'static str,
    outputs: Vec<ByteOutput>,
}

struct ByteOutput {
    module: String,
    path: &'static str,
    contents: Vec<u8>,
}

fn m2_byte_plan(config: &FactoryConfig) -> Result<BytePlan, RenderPlanError> {
    let plan = build_render_plan(config)?;
    let outputs = plan
        .outputs
        .into_iter()
        .map(|output| {
            let contents = output::render_output(&output)
                .map_err(|error| RenderPlanError(error.to_string()))?;
            Ok(ByteOutput {
                module: output.module,
                path: output.path,
                contents,
            })
        })
        .collect::<Result<_, RenderPlanError>>()?;
    Ok(BytePlan {
        acme_version: plan.acme_version,
        outputs,
    })
}

fn final_byte_plan(
    config: &FactoryConfig,
    repository: &Path,
    compiled_inputs: Option<&Path>,
) -> Result<BytePlan, RenderPlanError> {
    let mut plan = m2_byte_plan(config)?;
    if (config.modules.intake.enabled || config.modules.build_cloud.enabled)
        && compiled_inputs.is_none()
    {
        return Err(RenderPlanError(
            "enabled agent modules require --compiled-inputs DIR; explicitly compile and review approved inputs first"
                .to_owned(),
        ));
    }
    if let Some(directory) = compiled_inputs {
        let validated = validate_compiled_inputs(config, repository, directory)
            .map_err(|error| RenderPlanError(error.to_string()))?;
        plan.outputs
            .extend(validated.outputs.into_iter().map(|output| ByteOutput {
                module: output.ownership.module.name().to_owned(),
                path: output.ownership.path,
                contents: output.contents,
            }));
    }
    plan.outputs.sort_by_key(|output| output.path);
    validate_byte_outputs(&plan.outputs)?;
    Ok(plan)
}

fn validate_byte_outputs(outputs: &[ByteOutput]) -> Result<(), RenderPlanError> {
    validate_output_paths(outputs.iter().map(|output| output.path))
}

#[derive(Debug, Eq, PartialEq)]
pub struct RenderPlan {
    pub acme_version: &'static str,
    pub modules: Vec<ModuleManifest>,
    pub external_stages: Vec<ModuleManifest>,
    pub outputs: Vec<PlannedOutput>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct PlannedOutput {
    pub module: String,
    pub path: &'static str,
    pub template: &'static [u8],
    pub inputs: TemplateInputs,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TemplateInputs {
    Static,
    Ci {
        required_check: String,
        steps: Vec<CiStep>,
    },
    Ruleset {
        required_checks: Vec<String>,
        required_approvals: u32,
        require_extra_approval_for_unattributed_changes: bool,
    },
}

#[derive(Debug, Eq, PartialEq)]
pub struct CiStep {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct RenderPlanError(String);

impl fmt::Display for RenderPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RenderPlanError {}

/// Plan M2 templates and stage dependencies, not full compiled workflow output.
pub fn build_render_plan(config: &FactoryConfig) -> Result<RenderPlan, RenderPlanError> {
    build_render_plan_with_catalog(config, builtin_modules())
}

pub fn build_render_plan_with_catalog(
    config: &FactoryConfig,
    catalog: &[BuiltinModule],
) -> Result<RenderPlan, RenderPlanError> {
    validate_config(config)?;
    let mut modules = BTreeMap::new();
    for module in catalog {
        let manifest = module
            .manifest()
            .map_err(|error| RenderPlanError(error.to_string()))?;
        let enabled = module_enabled(config, module, &manifest)?;
        let name = manifest.name.clone();
        if modules
            .insert(name.clone(), (module, manifest, enabled))
            .is_some()
        {
            return Err(RenderPlanError(format!("duplicate catalog module {name}")));
        }
    }
    for name in ["ci", "issue-forms", "labels", "ruleset"] {
        if !modules.contains_key(name) {
            return Err(RenderPlanError(format!("missing catalog module {name}")));
        }
    }

    let mut external_stages = external_stages(config);
    let mut nodes = BTreeMap::new();
    for (_, manifest, enabled) in modules.values_mut() {
        if *enabled {
            if manifest.name == "ci" {
                manifest.provides.extend(config.modules.ci.provides.clone());
            }
            normalize(manifest)?;
            nodes.insert(manifest.name.clone(), &*manifest);
        }
    }
    for stage in &mut external_stages {
        normalize(stage)?;
        nodes.insert(stage.name.clone(), &*stage);
    }
    let order = dependency_order(&nodes)?;
    let mut plan = RenderPlan {
        acme_version: env!("CARGO_PKG_VERSION"),
        modules: Vec::new(),
        external_stages: Vec::new(),
        outputs: Vec::new(),
    };
    for name in order {
        if let Some((module, manifest, true)) = modules.remove(&name) {
            for asset in module.assets {
                plan.outputs.push(PlannedOutput {
                    module: name.clone(),
                    path: asset.output_path,
                    template: asset.template(),
                    inputs: TemplateInputs::Static,
                });
            }
            plan.modules.push(manifest);
        }
    }
    plan.external_stages = external_stages;
    plan.outputs.sort_by_key(|output| output.path);
    validate_paths(&plan.outputs, &plan.modules)?;
    for output in &mut plan.outputs {
        output.inputs = template_inputs(config, output.path);
    }
    Ok(plan)
}

fn module_enabled(
    config: &FactoryConfig,
    module: &BuiltinModule,
    manifest: &ModuleManifest,
) -> Result<bool, RenderPlanError> {
    let (binding, gate, enabled) = match manifest.name.as_str() {
        "ci" => (
            ConfigBinding::ModuleSection("modules.ci"),
            "required-status-check",
            config.modules.ci.enabled,
        ),
        "labels" | "issue-forms" => (
            ConfigBinding::EnabledDependency("modules.intake"),
            "enabled-intake",
            config.modules.intake.enabled,
        ),
        "ruleset" => (
            ConfigBinding::HumanMerge,
            "human-merge",
            config.modules.merge.enabled,
        ),
        name => return Err(RenderPlanError(format!("unsupported render module {name}"))),
    };
    if module.binding != binding || manifest.gate != gate {
        return Err(RenderPlanError(format!(
            "unsupported binding or gate for render module {}",
            manifest.name
        )));
    }
    Ok(enabled)
}

fn validate_config(config: &FactoryConfig) -> Result<(), RenderPlanError> {
    let acme = &config.toolchain.acme;
    if !acme.bootstrap && acme.target_version != env!("CARGO_PKG_VERSION") {
        return Err(RenderPlanError(format!(
            "toolchain.acme.target_version {:?} must be a concrete version matching running acme {}; use {:?} (tag v{}) or install the binary matching the approved pin",
            acme.target_version,
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_VERSION"),
        )));
    }
    if config.profile != "cli" || config.languages != ["rust"] {
        return Err(RenderPlanError(
            "unsupported render preset: require profile cli and languages [rust]".to_owned(),
        ));
    }
    for (name, enabled) in [
        ("review", config.modules.review.enabled),
        ("holdout", config.modules.holdout.enabled),
        ("build_local", config.modules.build_local.enabled),
        ("release", config.modules.release.enabled),
        ("ops", config.modules.ops.enabled),
    ] {
        if enabled {
            return Err(RenderPlanError(format!(
                "unsupported enabled module {name}"
            )));
        }
    }
    let ci = &config.modules.ci;
    if ci.enabled
        && (ci.gate != "required-status-check"
            || ci.required_checks.len() != 1
            || ci.required_checks[0].trim().is_empty()
            || ci.commands.is_empty()
            || ci.commands.len() != ci.step_names.len()
            || ci
                .commands
                .iter()
                .chain(&ci.step_names)
                .any(|value| value.trim().is_empty()))
    {
        return Err(RenderPlanError(
            "unsupported CI gate or inputs: use required-status-check, one required check, and one nonempty step name per command".to_owned(),
        ));
    }
    let merge = &config.modules.merge;
    if merge.enabled
        && (ci.required_checks.is_empty()
            || ci
                .required_checks
                .iter()
                .any(|check| check.trim().is_empty()))
    {
        return Err(RenderPlanError(
            "required-CI merge gate needs nonempty required check names".to_owned(),
        ));
    }
    if merge.enabled
        && (merge.implementation != "human"
            || merge.gate != "required-ci-and-resolved-threads"
            || merge.auto_merge)
    {
        return Err(RenderPlanError(
            "unsupported merge gate: require human merge, required CI and resolved threads; auto_merge must be false".to_owned(),
        ));
    }
    Ok(())
}

fn external_stages(config: &FactoryConfig) -> Vec<ModuleManifest> {
    let modules = &config.modules;
    [
        (
            "build_cloud",
            modules.build_cloud.enabled,
            &modules.build_cloud.gate,
            &modules.build_cloud.needs,
            &modules.build_cloud.provides,
        ),
        (
            "intake",
            modules.intake.enabled,
            &modules.intake.gate,
            &modules.intake.needs,
            &modules.intake.provides,
        ),
        (
            "merge",
            modules.merge.enabled,
            &modules.merge.gate,
            &modules.merge.needs,
            &modules.merge.provides,
        ),
        (
            "spec",
            modules.spec.enabled,
            &modules.spec.gate,
            &modules.spec.needs,
            &modules.spec.provides,
        ),
    ]
    .into_iter()
    .filter(|(_, enabled, _, _, _)| *enabled)
    .map(|(name, _, gate, needs, provides)| {
        let mut needs = needs.clone();
        if name == "merge" {
            needs.push("ruleset".to_owned());
            needs.push("ci".to_owned());
        }
        ModuleManifest {
            name: name.to_owned(),
            gate: gate.clone(),
            needs,
            provides: provides.clone(),
        }
    })
    .collect()
}

fn normalize(manifest: &mut ModuleManifest) -> Result<(), RenderPlanError> {
    for capabilities in [&mut manifest.needs, &mut manifest.provides] {
        if capabilities.iter().any(|value| value.trim().is_empty()) {
            return Err(RenderPlanError(format!(
                "empty capability in {}",
                manifest.name
            )));
        }
        capabilities.sort();
        capabilities.dedup();
    }
    Ok(())
}

fn dependency_order(
    nodes: &BTreeMap<String, &ModuleManifest>,
) -> Result<Vec<String>, RenderPlanError> {
    let mut providers = BTreeMap::new();
    for (name, node) in nodes {
        for capability in &node.provides {
            if let Some(previous) = providers.insert(capability, name) {
                return Err(RenderPlanError(format!(
                    "ambiguous capability {capability}: {previous} and {name}"
                )));
            }
        }
    }
    let mut dependencies = BTreeMap::new();
    for (name, node) in nodes {
        let mut needs = BTreeSet::new();
        for capability in &node.needs {
            let provider = providers.get(capability).ok_or_else(|| {
                RenderPlanError(format!(
                    "module {name} needs missing capability {capability}"
                ))
            })?;
            needs.insert((*provider).clone());
        }
        dependencies.insert(name.clone(), needs);
    }
    let mut order = Vec::new();
    while !dependencies.is_empty() {
        let ready = dependencies
            .iter()
            .find(|(_, needs)| needs.is_empty())
            .map(|(name, _)| name.clone())
            .ok_or_else(|| {
                RenderPlanError(format!(
                    "capability cycle among {}",
                    dependencies.keys().cloned().collect::<Vec<_>>().join(", ")
                ))
            })?;
        dependencies.remove(&ready);
        for needs in dependencies.values_mut() {
            needs.remove(&ready);
        }
        order.push(ready);
    }
    Ok(order)
}

fn template_inputs(config: &FactoryConfig, path: &str) -> TemplateInputs {
    match path {
        ".github/workflows/ci.yml" => TemplateInputs::Ci {
            required_check: config.modules.ci.required_checks[0].clone(),
            steps: config
                .modules
                .ci
                .step_names
                .iter()
                .zip(&config.modules.ci.commands)
                .map(|(name, command)| CiStep {
                    name: name.clone(),
                    command: command.clone(),
                })
                .collect(),
        },
        ".github/rulesets/main.json" => TemplateInputs::Ruleset {
            required_checks: config.modules.ci.required_checks.clone(),
            required_approvals: config.modules.merge.required_approvals,
            require_extra_approval_for_unattributed_changes: config
                .modules
                .merge
                .require_extra_approval_for_unattributed_changes,
        },
        _ => TemplateInputs::Static,
    }
}

fn validate_output_paths<'a>(
    outputs: impl IntoIterator<Item = &'a str>,
) -> Result<(), RenderPlanError> {
    let mut paths = BTreeSet::new();
    for path in outputs {
        if path.is_empty()
            || path
                .bytes()
                .any(|byte| !byte.is_ascii_alphanumeric() && !b"-_./".contains(&byte))
            || path.split('/').any(|part| {
                part.is_empty() || matches!(part, "." | "..") || part.eq_ignore_ascii_case(".git")
            })
        {
            return Err(RenderPlanError(format!("unsafe output path {path:?}")));
        }
        let folded = path.to_ascii_lowercase();
        if !paths.insert(folded.clone()) {
            return Err(RenderPlanError(format!("output path collision at {path}")));
        }
    }
    for path in &paths {
        for (index, _) in path.match_indices('/') {
            if paths.contains(&path[..index]) {
                return Err(RenderPlanError(format!("overlapping output path {path}")));
            }
        }
    }
    Ok(())
}

fn validate_paths(
    outputs: &[PlannedOutput],
    selected_modules: &[ModuleManifest],
) -> Result<(), RenderPlanError> {
    validate_output_paths(outputs.iter().map(|output| output.path))?;
    let mut inventory = BTreeMap::new();
    for module in builtin_modules() {
        let manifest = module
            .manifest()
            .map_err(|error| RenderPlanError(error.to_string()))?;
        for asset in module.assets {
            inventory.insert(asset.output_path, manifest.name.clone());
        }
    }
    for output in outputs {
        if inventory.get(output.path) != Some(&output.module) {
            return Err(RenderPlanError(format!(
                "output {} is outside module {}'s M2 inventory",
                output.path, output.module
            )));
        }
    }
    for (path, owner) in inventory {
        if selected_modules.iter().any(|module| module.name == owner)
            && !outputs.iter().any(|output| output.path == path)
        {
            return Err(RenderPlanError(format!(
                "missing output {path} for module {owner}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ByteOutput, validate_byte_outputs};

    fn union_outputs(paths: &[&'static str]) -> Vec<ByteOutput> {
        paths
            .iter()
            .enumerate()
            .map(|(index, path)| ByteOutput {
                module: if index == 0 { "ci" } else { "intake" }.to_owned(),
                path,
                contents: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn final_union_rejects_collisions_and_file_directory_overlaps() {
        for paths in [
            [".github/workflows/ci.yml", ".github/workflows/ci.yml"],
            [".GITHUB/workflows/CI.yml", ".github/workflows/ci.yml"],
            [".github/workflows", ".github/workflows/intake.md"],
            ["scripts", "scripts/check-build-approval.py"],
        ] {
            assert!(
                validate_byte_outputs(&union_outputs(&paths)).is_err(),
                "accepted conflicting union paths: {paths:?}"
            );
        }
    }

    #[test]
    fn final_union_rejects_unsafe_paths_but_accepts_distinct_file_prefixes() {
        for path in [
            "../escape",
            "/absolute",
            ".git/config",
            "scripts\\helper",
            "",
        ] {
            assert!(
                validate_byte_outputs(&union_outputs(&["safe", path])).is_err(),
                "accepted unsafe union path: {path:?}"
            );
        }
        assert!(
            validate_byte_outputs(&union_outputs(&[
                ".github/workflows/intake.md",
                ".github/workflows/intake.md-extra",
                "scripts/check-build-approval.py",
            ]))
            .is_ok()
        );
    }
}
