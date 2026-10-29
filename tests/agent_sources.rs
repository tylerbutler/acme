use acme::FactoryConfig;
use acme::agent_catalog::{AgentModule, AgentOwnedPath, AgentPathKind, agent_modules};
use acme::config::ModelId;
use acme::render::{AgentSourcePlan, build_agent_source_plan, build_render_plan};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const FACTORY: &str = include_str!("../factory/factory.toml");
const INTAKE: &str = ".github/workflows/intake.md";
const BUILD: &str = ".github/workflows/build-cloud.md";
const HELPER: &str = "scripts/check-build-approval.py";

fn config() -> FactoryConfig {
    let mut config = FactoryConfig::from_toml(FACTORY).unwrap();
    env!("CARGO_PKG_VERSION").clone_into(&mut config.toolchain.acme.target_version);
    config
}

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "acme-agent-sources-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join("AGENTS.md"),
            "Human-owned project instructions.\n",
        )
        .unwrap();
        fs::write(
            root.join("commit-types.json"),
            include_bytes!("../commit-types.json"),
        )
        .unwrap();
        Self(root)
    }

    fn plan(
        &self,
        config: &FactoryConfig,
    ) -> Result<AgentSourcePlan, acme::render::RenderPlanError> {
        build_agent_source_plan(config, &self.0)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn text<'a>(plan: &'a AgentSourcePlan, path: &str) -> &'a str {
    std::str::from_utf8(
        &plan
            .outputs
            .iter()
            .find(|output| output.path == path)
            .unwrap()
            .contents,
    )
    .unwrap()
}

#[test]
fn omitted_models_reproduce_reviewed_sources_and_only_declare_compiler_ownership() {
    let workspace = Workspace::new();
    let config = config();
    let plan = workspace.plan(&config).unwrap();
    assert_eq!(plan, workspace.plan(&config).unwrap());
    assert_eq!(plan.outputs.len(), 3);
    for (path, expected, owner) in [
        (
            INTAKE,
            include_bytes!("../.github/workflows/intake.md").as_slice(),
            AgentModule::Intake,
        ),
        (
            BUILD,
            include_bytes!("../.github/workflows/build-cloud.md").as_slice(),
            AgentModule::BuildCloud,
        ),
        (
            HELPER,
            include_bytes!("../scripts/check-build-approval.py").as_slice(),
            AgentModule::BuildCloud,
        ),
    ] {
        let output = plan
            .outputs
            .iter()
            .find(|output| output.path == path)
            .unwrap();
        assert_eq!(output.module, owner);
        assert_eq!(output.contents, expected, "{path}");
    }
    assert_eq!(plan.ownership.len(), 5);
    for output in &plan.outputs {
        assert!(plan.ownership.contains(&AgentOwnedPath {
            module: output.module,
            path: output.path,
            kind: if output.path == HELPER {
                AgentPathKind::RuntimeHelper
            } else {
                AgentPathKind::WorkflowSource
            },
        }));
    }
    for (path, module) in [
        (".github/workflows/intake.lock.yml", AgentModule::Intake),
        (
            ".github/workflows/build-cloud.lock.yml",
            AgentModule::BuildCloud,
        ),
    ] {
        assert!(plan.ownership.iter().any(|entry| {
            entry.path == path
                && entry.module == module
                && entry.kind == AgentPathKind::CompilerOutput
        }));
        assert!(!plan.outputs.iter().any(|output| output.path == path));
    }
    assert_eq!(plan.runtime_tools, ["gh", "python3"]);
    assert_eq!(fs::read_dir(&workspace.0).unwrap().count(), 2);
    assert_eq!(acme::builtin_modules().len(), 4);
    assert_eq!(build_render_plan(&config).unwrap().outputs.len(), 8);
    assert_eq!(agent_modules().len(), 2);
}

#[test]
fn disabling_build_cloud_removes_only_its_sources_and_requirements() {
    let workspace = Workspace::new();
    let mut config = config();
    config.modules.build_cloud.enabled = false;
    config.modules.build_cloud.workflow_enabled = false;
    config.modules.build_cloud.assignment_enabled = false;
    fs::remove_file(workspace.0.join("commit-types.json")).unwrap();
    let plan = workspace.plan(&config).unwrap();
    assert_eq!(plan.outputs.len(), 1);
    assert_eq!(plan.outputs[0].path, INTAKE);
    assert_eq!(plan.ownership.len(), 2);
    assert!(
        plan.ownership
            .iter()
            .all(|owned| owned.module == AgentModule::Intake)
    );
    assert!(plan.runtime_tools.is_empty());
    assert_eq!(
        text(&plan, INTAKE),
        include_str!("../.github/workflows/intake.md")
    );
}

#[test]
fn builder_model_only_changes_assignment_not_controller_or_detection() {
    let workspace = Workspace::new();
    let mut config = config();
    let before = workspace.plan(&config).unwrap();
    "builder-2".clone_into(&mut config.engines.build_cloud.model);
    let after = workspace.plan(&config).unwrap();
    assert_eq!(text(&before, INTAKE), text(&after, INTAKE));
    assert_eq!(text(&before, HELPER), text(&after, HELPER));
    assert_eq!(
        text(&after, BUILD),
        text(&before, BUILD).replace("    model: gpt-6.1-sol", "    model: \"builder-2\"")
    );
}

#[test]
fn agent_catalog_preserves_static_dependencies_and_human_spec() {
    let workspace = Workspace::new();
    let mut config = config();
    let plan = workspace.plan(&config).unwrap();
    assert!(plan.modules.iter().any(|module| {
        module.name == "spec" && module.provides.contains(&"approved-spec".to_owned())
    }));
    for change in [
        "missing",
        "ambiguous",
        "cycle",
        "removed-static",
        "nonhuman-spec-provider",
    ] {
        config = self::config();
        match change {
            "missing" => config.modules.spec.enabled = false,
            "ambiguous" => config.modules.spec.provides.push("ci".to_owned()),
            "cycle" => config.modules.intake.needs.push("draft-pr".to_owned()),
            "removed-static" => {
                config.modules.spec.enabled = false;
                config.modules.build_cloud.needs.clear();
            }
            "nonhuman-spec-provider" => {
                config.modules.spec.enabled = false;
                config.modules.ci.provides.push("approved-spec".to_owned());
            }
            _ => unreachable!(),
        }
        assert!(workspace.plan(&config).is_err(), "{change}");
    }
}

#[test]
fn model_overrides_only_change_the_requested_roles() {
    let workspace = Workspace::new();
    for intake in [false, true] {
        for controller in [false, true] {
            for detection in [false, true] {
                let mut config = config();
                if intake {
                    config.engines.intake.controller_model =
                        controller.then(|| ModelId::try_from("controller-1".to_owned()).unwrap());
                    config.engines.intake.detection_model =
                        detection.then(|| ModelId::try_from("detector-1".to_owned()).unwrap());
                } else {
                    config.engines.build_cloud.controller_model =
                        controller.then(|| ModelId::try_from("controller-1".to_owned()).unwrap());
                    config.engines.build_cloud.detection_model =
                        detection.then(|| ModelId::try_from("detector-1".to_owned()).unwrap());
                }
                let plan = workspace.plan(&config).unwrap();
                let source = text(&plan, if intake { INTAKE } else { BUILD });
                assert_eq!(source.contains("  model: \"controller-1\"\n"), controller);
                assert_eq!(source.contains("    model: \"detector-1\"\n"), detection);
                if controller && !detection {
                    assert!(source.contains(
                        "    model: ${{ vars.GH_AW_MODEL_DETECTION_COPILOT || vars.GH_AW_DEFAULT_MODEL_COPILOT || 'detection' }}\n"
                    ));
                }
                assert!(text(&plan, BUILD).contains("    model: gpt-6.1-sol\n"));
                let other = if intake { BUILD } else { INTAKE };
                assert_eq!(
                    text(&plan, other),
                    text(&workspace.plan(&self::config()).unwrap(), other)
                );
            }
        }
    }
}

#[test]
fn staged_and_inactive_settings_keep_live_safety_guards() {
    let workspace = Workspace::new();
    for workflow_enabled in [false, true] {
        for assignment_enabled in [false, true] {
            let mut config = config();
            config.modules.intake.staged = true;
            config.modules.build_cloud.staged = true;
            config.modules.intake.workflow_enabled = workflow_enabled;
            config.modules.build_cloud.workflow_enabled = workflow_enabled;
            config.modules.build_cloud.assignment_enabled = assignment_enabled;
            let plan = workspace.plan(&config).unwrap();
            let intake = text(&plan, INTAKE);
            let build = text(&plan, BUILD);
            for source in [intake, build] {
                assert!(source.contains("  staged: true\n"));
                assert!(source.contains("vars.ACME_AGENT_WORKFLOWS_ENABLED == 'true'"));
                assert!(source.contains("permissions:\n  contents: read\n  issues: read\n"));
                assert!(source.contains("    max: 1\n"));
                assert!(!source.contains("write-all"));
            }
            assert_eq!(intake.contains("false &&"), !workflow_enabled);
            assert_eq!(
                build.contains("false &&"),
                !workflow_enabled || !assignment_enabled
            );
            for guard in [
                "vars.ACME_CLOUD_ASSIGNMENT_ENABLED == 'true'",
                "github.event.label.name == 'ready-for-agent'",
                "github.actor == github.repository_owner",
                "github.triggering_actor == github.repository_owner",
                "if: needs.approval.outputs.approved == 'true'",
                "    required-labels: [ready-for-agent]\n",
                "    target: triggering\n",
                "persist-credentials: false",
            ] {
                assert!(build.contains(guard), "{guard}");
            }
        }
    }
}

#[test]
fn supported_config_values_are_rendered_not_silently_ignored() {
    let workspace = Workspace::new();
    let mut config = config();
    config.modules.intake.roles = vec!["maintain".to_owned(), "write".to_owned()];
    config.engines.intake.token_secret = "INTAKE_TOKEN".to_owned();
    config.engines.build_cloud.token_secret = "CONTROLLER_TOKEN".to_owned();
    config.modules.build_cloud.token_secret = "ASSIGNMENT_TOKEN".to_owned();
    config.engines.build_cloud.model = "builder-2".to_owned();
    config.modules.build_cloud.target_minutes = 45;
    config.budget.max_ai_credits_per_run = 1000;
    let plan = workspace.plan(&config).unwrap();
    let intake = text(&plan, INTAKE);
    let build = text(&plan, BUILD);
    assert!(intake.contains("  roles: [\"maintain\",\"write\"]"));
    assert!(intake.contains("${{ secrets.INTAKE_TOKEN }}"));
    assert!(build.contains("COPILOT_GITHUB_TOKEN: ${{ secrets.CONTROLLER_TOKEN }}"));
    assert!(build.contains("github-token: ${{ secrets.ASSIGNMENT_TOKEN }}"));
    assert!(build.contains("    model: \"builder-2\""));
    assert!(build.contains("under 45 minutes"));
    assert!(build.contains("max-ai-credits: 1000"));
}

#[test]
fn unsupported_inputs_and_injection_fail_explicitly() {
    let workspace = Workspace::new();
    let changes: &[fn(&mut FactoryConfig)] = &[
        |config| config.toolchain.acme.target_version = "HEAD".to_owned(),
        |config| config.toolchain.gh_aw.version = "latest".to_owned(),
        |config| config.engines.intake.engine = "claude".to_owned(),
        |config| config.engines.build_cloud.engine = "codex".to_owned(),
        |config| config.engines.review.enabled = true,
        |config| config.modules.spec.implementation = "agent".to_owned(),
        |config| config.modules.spec.gate = "none".to_owned(),
        |config| config.modules.intake.roles.clear(),
        |config| config.modules.intake.roles = vec!["all".to_owned()],
        |config| config.modules.intake.roles = vec!["admin]\npermissions: write-all".to_owned()],
        |config| config.engines.intake.token_secret = "TOKEN }}".to_owned(),
        |config| config.modules.build_cloud.token_secret = "${{ secrets.OTHER }}".to_owned(),
        |config| config.engines.build_cloud.model = "builder\nmodel: malicious".to_owned(),
        |config| config.modules.intake.gate = "none".to_owned(),
        |config| config.modules.build_cloud.gate = "none".to_owned(),
        |config| config.modules.build_cloud.max_repositories = 2,
        |config| config.modules.build_cloud.max_pull_requests = 2,
        |config| config.modules.build_cloud.target_minutes = 0,
        |config| config.budget.max_ai_credits_per_run = 0,
        |config| config.budget.enforce_monthly = true,
        |config| config.budget.enforce_ai_credits = false,
        |config| config.autonomy = "L5".to_owned(),
    ];
    for change in changes {
        let mut config = config();
        change(&mut config);
        assert!(workspace.plan(&config).is_err());
    }
}

#[test]
fn missing_invalid_and_nonregular_authored_prerequisites_fail_without_writes() {
    for path in ["AGENTS.md", "commit-types.json"] {
        for invalid in [
            None,
            Some(b"".as_slice()),
            Some(b" \n\t".as_slice()),
            Some(b"\xff".as_slice()),
        ] {
            let workspace = Workspace::new();
            fs::remove_file(workspace.0.join(path)).unwrap();
            if let Some(bytes) = invalid {
                fs::write(workspace.0.join(path), bytes).unwrap();
            }
            let error = workspace.plan(&config()).unwrap_err().to_string();
            assert!(error.contains(path), "{error}");
            assert!(error.contains("author"), "{error}");
            if let Some(bytes) = invalid {
                assert_eq!(fs::read(workspace.0.join(path)).unwrap(), bytes);
            } else {
                assert!(!workspace.0.join(path).exists());
            }
        }
        let workspace = Workspace::new();
        fs::remove_file(workspace.0.join(path)).unwrap();
        fs::create_dir(workspace.0.join(path)).unwrap();
        assert!(workspace.plan(&config()).is_err());
    }
}

#[cfg(unix)]
#[test]
fn symlinked_authored_prerequisites_are_rejected() {
    use std::os::unix::fs::symlink;
    for path in ["AGENTS.md", "commit-types.json"] {
        let workspace = Workspace::new();
        fs::rename(workspace.0.join(path), workspace.0.join("authored")).unwrap();
        symlink("authored", workspace.0.join(path)).unwrap();
        assert!(workspace.plan(&config()).is_err());
    }
}

#[test]
fn commit_types_validate_actual_definitions_and_select_a_supported_example() {
    let workspace = Workspace::new();
    for input in [
        "{}",
        "[]",
        r#"{"types":[]}"#,
        r#"{"types":{}}"#,
        r#"{"types":{"feat":null}}"#,
        r#"{"types":{"feat":{"description":1,"changelog_group":null}}}"#,
        r#"{"types":{"feat":{"description":"","changelog_group":null}}}"#,
        r#"{"types":{"bad type":{"description":"Bad","changelog_group":null}}}"#,
        r#"{"types":{"feat":{"description":"Feature","changelog_group":[]}}}"#,
    ] {
        fs::write(workspace.0.join("commit-types.json"), input).unwrap();
        assert!(workspace.plan(&config()).is_err(), "{input}");
    }
    fs::write(
        workspace.0.join("commit-types.json"),
        r#"{"types":{"improvement":{"description":"Improve","changelog_group":"Improvements"}}}"#,
    )
    .unwrap();
    assert!(
        text(&workspace.plan(&config()).unwrap(), BUILD)
            .contains("\"improvement: add module manifests\"")
    );
}

#[test]
fn disabled_modules_have_no_outputs_ownership_or_authored_requirements() {
    let workspace = Workspace::new();
    let mut config = config();
    config.modules.intake.enabled = false;
    config.modules.spec.enabled = false;
    config.modules.build_cloud.enabled = false;
    config.modules.intake.workflow_enabled = false;
    config.modules.build_cloud.workflow_enabled = false;
    config.modules.build_cloud.assignment_enabled = false;
    fs::remove_file(workspace.0.join("AGENTS.md")).unwrap();
    fs::remove_file(workspace.0.join("commit-types.json")).unwrap();
    let plan = workspace.plan(&config).unwrap();
    assert!(plan.outputs.is_empty());
    assert!(plan.ownership.is_empty());
    assert!(plan.runtime_tools.is_empty());
    config.modules.intake.enabled = true;
    assert!(workspace.plan(&config).is_err());
}
