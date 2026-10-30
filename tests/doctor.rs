use acme::{
    FactoryConfig, GitHubError, GitHubMethod, GitHubTransport, builtin_modules,
    checkout_repository, run_doctor, run_doctor_fix, run_doctor_fix_with_compiled_inputs,
    run_doctor_with_compiled_inputs,
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const REPOSITORY: &str = "repos/tylerbutler/acme";
const LABELS: &str = "repos/tylerbutler/acme/labels?per_page=100";
const RULESETS: &str = "repos/tylerbutler/acme/rulesets?per_page=100";
const RULESET: &str = "repos/tylerbutler/acme/rulesets/7";
const ACTIONS: &str = "repos/tylerbutler/acme/actions/permissions/workflow";
const VARIABLES: &str = "repos/tylerbutler/acme/actions/variables?per_page=100";
const INTAKE_STATE: &str = "repos/tylerbutler/acme/actions/workflows/intake.lock.yml";
const BUILD_STATE: &str = "repos/tylerbutler/acme/actions/workflows/build-cloud.lock.yml";
const WORKFLOW_GATE: &str = "ACME_AGENT_WORKFLOWS_ENABLED";
const ASSIGNMENT_GATE: &str = "ACME_CLOUD_ASSIGNMENT_ENABLED";
const SECRETS: &str = "repos/tylerbutler/acme/actions/secrets?per_page=100";
const CONTROLLER_SELECTION: &str =
    "${{ vars.GH_AW_MODEL_AGENT_COPILOT || vars.GH_AW_DEFAULT_MODEL_COPILOT || 'auto' }}";
const DETECTION_SELECTION: &str =
    "${{ vars.GH_AW_MODEL_DETECTION_COPILOT || vars.GH_AW_DEFAULT_MODEL_COPILOT || 'detection' }}";
const DETECTION_FALLBACK: &str =
    "${{ vars.GH_AW_MODEL_DETECTION_COPILOT || vars.GH_AW_DEFAULT_MODEL_COPILOT || 'auto' }}";
type ConfigMutation = fn(&mut FactoryConfig);

#[path = "support/compiled_fixture.rs"]
mod compiled_fixture;

fn agent_config() -> FactoryConfig {
    let mut config = FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml")).unwrap();
    env!("CARGO_PKG_VERSION").clone_into(&mut config.toolchain.acme.target_version);
    config
}

struct AgentFixture {
    root: PathBuf,
    review: compiled_fixture::ReviewFixture,
}

impl AgentFixture {
    fn new(config: &FactoryConfig) -> Self {
        let root = temporary_root();
        copy_assets(&root);
        fs::copy(Path::new(ROOT).join("AGENTS.md"), root.join("AGENTS.md")).unwrap();
        fs::copy(
            Path::new(ROOT).join("commit-types.json"),
            root.join("commit-types.json"),
        )
        .unwrap();
        fs::write(root.join("factory.toml"), toml::to_string(config).unwrap()).unwrap();
        Self {
            root,
            review: compiled_fixture::ReviewFixture::new(),
        }
    }

    fn compiled(&self) -> PathBuf {
        self.review
            .refresh(&self.root, Path::new("factory.toml"))
            .unwrap()
    }

    fn adopt(&self, config: &FactoryConfig, compiled: &Path) {
        let inputs = acme::render::validate_compiled_inputs(config, &self.root, compiled).unwrap();
        for output in inputs.outputs {
            let path = self.root.join(output.ownership.path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, output.contents).unwrap();
        }
    }
}

impl Drop for AgentFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn enabled_agents_require_reviewed_compiled_inputs_without_implicit_compilation() {
    let config = agent_config();
    let report = run_doctor(&config, Path::new(ROOT), &FakeGitHub::matching());
    assert!(!report.is_success(), "{report}");
    let check = report
        .checks
        .iter()
        .find(|check| check.id == "local.agent.compiled-inputs")
        .expect("explicit compiled-input check");
    assert!(!check.passed);
    assert!(check.repair.contains("--compiled-inputs DIR"));
}

#[test]
fn configured_activation_flags_are_reported_separately_from_remote_state() {
    let mut config = agent_config();
    config.modules.build_cloud.workflow_enabled = true;
    config.modules.build_cloud.assignment_enabled = false;
    config.modules.build_cloud.staged = true;
    let report = run_doctor(&config, Path::new(ROOT), &FakeGitHub::matching());
    let configured = check(&report, "local.agent.build-cloud.activation-config");
    assert!(configured.passed);
    assert_eq!(
        configured.observed,
        "workflow_enabled=true; assignment_enabled=false; effective source gate=false; staged=true"
    );
    assert!(check(&report, "github.agent.build-cloud.workflow-state").passed);
    assert!(
        !check(
            &report,
            "github.agent.variable.ACME_CLOUD_ASSIGNMENT_ENABLED"
        )
        .passed
    );
}

#[test]
fn cleanly_absent_activation_variables_are_off_but_cannot_enable_active_workflows() {
    for active in [false, true] {
        let mut config = agent_config();
        config.modules.intake.workflow_enabled = active;
        config.modules.intake.staged = !active;
        config.modules.build_cloud.workflow_enabled = active;
        config.modules.build_cloud.assignment_enabled = active;
        config.modules.build_cloud.staged = !active;
        let fixture = AgentFixture::new(&config);
        let compiled = fixture.compiled();
        fixture.adopt(&config, &compiled);
        let mut github = FakeGitHub::matching();
        github.responses.insert(
            VARIABLES.to_owned(),
            vec![json!({"total_count": 0, "variables": []})],
        );
        for endpoint in [INTAKE_STATE, BUILD_STATE] {
            github.responses.get_mut(endpoint).unwrap()[0]["state"] = json!(if active {
                "active"
            } else {
                "disabled_manually"
            });
        }
        let report =
            run_doctor_with_compiled_inputs(&config, &fixture.root, &github, Some(&compiled));
        for name in [
            "ACME_AGENT_WORKFLOWS_ENABLED",
            "ACME_CLOUD_ASSIGNMENT_ENABLED",
        ] {
            let gate = check(&report, &format!("github.agent.variable.{name}"));
            assert_eq!(gate.passed, !active, "{report}");
            assert_eq!(gate.observed, "absent; workflow gate is off");
        }
        assert_eq!(report.is_success(), !active, "{report}");
    }
}

#[test]
fn fork_disabled_workflows_are_inactive_but_deleted_workflows_are_not() {
    let mut config = agent_config();
    config.modules.intake.workflow_enabled = false;
    config.modules.intake.staged = true;
    config.modules.build_cloud.workflow_enabled = false;
    config.modules.build_cloud.assignment_enabled = false;
    config.modules.build_cloud.staged = true;
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
    fixture.adopt(&config, &compiled);
    let mut github = FakeGitHub::matching();
    github.responses.insert(
        VARIABLES.to_owned(),
        vec![json!({"total_count": 0, "variables": []})],
    );
    for (state, expected_pass) in [("disabled_fork", true), ("deleted", false)] {
        for endpoint in [INTAKE_STATE, BUILD_STATE] {
            github.responses.get_mut(endpoint).unwrap()[0]["state"] = json!(state);
        }
        let report =
            run_doctor_with_compiled_inputs(&config, &fixture.root, &github, Some(&compiled));
        assert_eq!(report.is_success(), expected_pass, "{report}");
        for id in [
            "github.agent.intake.workflow-state",
            "github.agent.build-cloud.workflow-state",
        ] {
            let workflow = check(&report, id);
            assert_eq!(workflow.passed, expected_pass);
            assert_eq!(workflow.observed, state);
        }
    }
}

#[test]
fn variable_listing_handles_pagination_without_reporting_unrelated_values() {
    let config = agent_config();
    let mut github = FakeGitHub::matching();
    github.responses.insert(
        VARIABLES.to_owned(),
        vec![
            json!({"total_count": 3, "variables": [
                {"name": "UNRELATED", "value": "do-not-print-this-value"},
                {"name": "ACME_AGENT_WORKFLOWS_ENABLED", "value": "true"}
            ]}),
            json!({"total_count": 3, "variables": [
                {"name": "ACME_CLOUD_ASSIGNMENT_ENABLED", "value": "true"}
            ]}),
        ],
    );
    let report = run_doctor(&config, Path::new(ROOT), &github);
    for id in [
        "github.agent.variables",
        "github.agent.variable.ACME_AGENT_WORKFLOWS_ENABLED",
        "github.agent.variable.ACME_CLOUD_ASSIGNMENT_ENABLED",
    ] {
        assert!(check(&report, id).passed, "{report}");
    }
    assert!(!report.to_string().contains("do-not-print-this-value"));
    assert!(!report.to_string().contains("UNRELATED"));
    assert_eq!(
        github
            .calls
            .borrow()
            .iter()
            .filter(|endpoint| *endpoint == VARIABLES)
            .count(),
        1
    );
}

#[test]
fn inactive_variable_listing_errors_never_become_clean_absence() {
    let mut config = agent_config();
    config.modules.intake.workflow_enabled = false;
    config.modules.build_cloud.workflow_enabled = false;
    config.modules.build_cloud.assignment_enabled = false;
    for pages in [
        vec![],
        vec![json!([])],
        vec![json!({"total_count": "0", "variables": []})],
        vec![json!({"total_count": 1, "variables": []})],
        vec![json!({"total_count": 1, "variables": [{"name": "ANY_NAME"}]})],
        vec![
            json!({"total_count": 2, "variables": [{"name": "SAME", "value": "false"}, {"name": "same", "value": "false"}]}),
        ],
        vec![
            json!({"total_count": 0, "variables": []}),
            json!({"total_count": 1, "variables": []}),
        ],
    ] {
        let mut github = FakeGitHub::matching();
        github.responses.insert(VARIABLES.to_owned(), pages);
        let report = run_doctor(&config, Path::new(ROOT), &github);
        assert!(!check(&report, "github.agent.variables").passed);
        assert!(
            !report
                .checks
                .iter()
                .any(|check| check.id.starts_with("github.agent.variable."))
        );
    }
    for error in [
        GitHubError::AuthenticationRequired,
        GitHubError::PermissionDenied,
        GitHubError::ApiFailure,
    ] {
        let mut github = FakeGitHub::matching();
        github.error = Some(error);
        let report = run_doctor(&config, Path::new(ROOT), &github);
        assert!(!check(&report, "github.agent.variables").passed);
        assert!(!report.to_string().contains("absent; workflow gate is off"));
    }
    for value in ["off", "FALSE", "1"] {
        let mut github = FakeGitHub::matching();
        set_gate(&mut github, WORKFLOW_GATE, value);
        let report = run_doctor(&config, Path::new(ROOT), &github);
        assert!(
            !check(
                &report,
                "github.agent.variable.ACME_AGENT_WORKFLOWS_ENABLED"
            )
            .passed
        );
    }
}

#[test]
fn agent_sources_are_checked_even_without_compiled_inputs() {
    let config = agent_config();
    let root = temporary_root();
    copy_assets(&root);
    fs::copy(Path::new(ROOT).join("AGENTS.md"), root.join("AGENTS.md")).unwrap();
    fs::copy(
        Path::new(ROOT).join("commit-types.json"),
        root.join("commit-types.json"),
    )
    .unwrap();
    let plan = acme::render::build_agent_source_plan(&config, &root).unwrap();
    for output in plan.outputs {
        let path = root.join(output.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, output.contents).unwrap();
    }
    let path = root.join(".github/workflows/intake.md");
    fs::write(&path, "permissions: write-all\n").unwrap();
    let report = run_doctor(&config, &root, &FakeGitHub::matching());
    assert!(
        report.checks.iter().any(|check| {
            check.id == "local.agent.asset._github_workflows_intake_md" && !check.passed
        }),
        "{report}"
    );
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "permissions: write-all\n"
    );
    fs::remove_dir_all(root).unwrap();
}

fn check<'a>(report: &'a acme::DoctorReport, id: &str) -> &'a acme::Check {
    report
        .checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("missing {id}\n{report}"))
}

fn set_gate(github: &mut FakeGitHub, name: &str, value: &str) {
    let variables = github.responses.get_mut(VARIABLES).unwrap()[0]["variables"]
        .as_array_mut()
        .unwrap();
    variables
        .iter_mut()
        .find(|variable| variable["name"] == name)
        .unwrap()["value"] = json!(value);
}

fn agent_asset_id(path: &str) -> String {
    format!(
        "local.agent.asset.{}",
        path.chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
    )
}

#[test]
fn live_and_staged_inactive_agent_fixtures_pass_with_reviewed_inputs() {
    for (intake_active, build_active, assignment) in [
        (true, true, true),
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let mut config = agent_config();
        config.modules.intake.workflow_enabled = intake_active;
        config.modules.build_cloud.workflow_enabled = build_active;
        config.modules.build_cloud.assignment_enabled = assignment;
        config.modules.intake.staged = !intake_active;
        config.modules.build_cloud.staged = !assignment;
        let fixture = AgentFixture::new(&config);
        let compiled = fixture.compiled();
        fixture.adopt(&config, &compiled);
        let mut github = FakeGitHub::matching();
        github.responses.insert(
            INTAKE_STATE.to_owned(),
            vec![json!({"state": if intake_active { "active" } else { "disabled_manually" }})],
        );
        github.responses.insert(
            BUILD_STATE.to_owned(),
            vec![json!({"state": if build_active { "active" } else { "disabled_manually" }})],
        );
        set_gate(
            &mut github,
            WORKFLOW_GATE,
            if intake_active || build_active {
                "true"
            } else {
                "false"
            },
        );
        set_gate(
            &mut github,
            ASSIGNMENT_GATE,
            if assignment { "true" } else { "false" },
        );
        let before = fs::read(fixture.root.join(".github/workflows/build-cloud.md")).unwrap();
        let report =
            run_doctor_with_compiled_inputs(&config, &fixture.root, &github, Some(&compiled));
        assert!(report.is_success(), "{report}");
        assert_eq!(
            fs::read(fixture.root.join(".github/workflows/build-cloud.md")).unwrap(),
            before
        );
        let mut calls = github.calls.borrow().clone();
        calls.sort();
        let mut allowed: Vec<_> = [
            LABELS,
            RULESETS,
            RULESET,
            REPOSITORY,
            ACTIONS,
            INTAKE_STATE,
            BUILD_STATE,
            VARIABLES,
            SECRETS,
        ]
        .map(str::to_owned)
        .into();
        allowed.sort();
        assert_eq!(
            calls, allowed,
            "no logs, tools, compiler, dispatch or inference endpoints"
        );
    }
}

#[test]
fn exact_source_helper_and_lock_drift_fails_without_rewrites() {
    let config = agent_config();
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
    fixture.adopt(&config, &compiled);
    for (path, from, to) in [
        (
            ".github/workflows/intake.md",
            "issues: read",
            "issues: write",
        ),
        (
            ".github/workflows/intake.md",
            "roles: [admin]",
            "roles: [read]",
        ),
        (
            ".github/workflows/intake.md",
            "max-ai-credits: 5000",
            "max-ai-credits: 9999",
        ),
        (
            ".github/workflows/intake.md",
            "staged: false",
            "staged: true",
        ),
        (".github/workflows/intake.md", "== 'true'", "!= 'true'"),
        (
            ".github/workflows/build-cloud.md",
            "github.actor == github.repository_owner",
            "true",
        ),
        (
            ".github/workflows/build-cloud.md",
            "target: triggering",
            "target: all",
        ),
        (".github/workflows/build-cloud.md", "max: 1", "max: 2"),
        (
            ".github/workflows/build-cloud.md",
            "model: gpt-6.1-sol",
            "model: auto",
        ),
        (
            ".github/workflows/build-cloud.md",
            "needs: [approval]",
            "needs: [activation]",
        ),
        (
            "scripts/check-build-approval.py",
            "import",
            "# drift\nimport",
        ),
        (
            ".github/workflows/intake.lock.yml",
            "contents: read",
            "contents: write",
        ),
        (
            ".github/workflows/build-cloud.lock.yml",
            "contents: read",
            "contents: write",
        ),
    ] {
        let target = fixture.root.join(path);
        let original = fs::read_to_string(&target).unwrap();
        assert!(original.contains(from), "{path}: {from}");
        let changed = original.replacen(from, to, 1);
        fs::write(&target, &changed).unwrap();
        let report = run_doctor_with_compiled_inputs(
            &config,
            &fixture.root,
            &FakeGitHub::matching(),
            Some(&compiled),
        );
        assert!(
            !check(&report, &agent_asset_id(path)).passed,
            "{path}\n{report}"
        );
        assert!(check(&report, "local.agent.compiled-inputs").passed);
        assert_eq!(fs::read_to_string(&target).unwrap(), changed);
        fs::write(target, original).unwrap();
    }
}

#[test]
fn compiled_inputs_missing_edited_extra_and_stale_fail_explicitly() {
    let config = agent_config();
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
    fixture.adopt(&config, &compiled);
    let run = |config: &FactoryConfig, path: &Path| {
        let report = run_doctor_with_compiled_inputs(
            config,
            &fixture.root,
            &FakeGitHub::matching(),
            Some(path),
        );
        assert!(
            !check(&report, "local.agent.compiled-inputs").passed,
            "{report}"
        );
    };
    run(&config, &fixture.root.join("missing-review"));
    let record_path = compiled.join("compiled-workflows.json");
    let record = fs::read(&record_path).unwrap();
    fs::remove_file(&record_path).unwrap();
    run(&config, &compiled);
    fs::write(&record_path, b"{}").unwrap();
    run(&config, &compiled);
    fs::write(&record_path, record).unwrap();
    let lock = compiled.join(".github/workflows/intake.lock.yml");
    let bytes = fs::read(&lock).unwrap();
    fs::write(&lock, "edited lock").unwrap();
    run(&config, &compiled);
    fs::write(lock, bytes).unwrap();
    fs::write(compiled.join("extra"), "unexpected").unwrap();
    run(&config, &compiled);
    fs::remove_file(compiled.join("extra")).unwrap();
    let mut stale = agent_config();
    stale.modules.intake.staged = !stale.modules.intake.staged;
    run(&stale, &compiled);
    let mut wrong_pin = agent_config();
    wrong_pin.toolchain.acme.target_version = "99.0.0".to_owned();
    run(&wrong_pin, &compiled);
}

#[cfg(unix)]
#[test]
fn unsafe_checkout_and_compiled_paths_are_not_followed() {
    use std::os::unix::fs::symlink;
    let config = agent_config();
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
    fixture.adopt(&config, &compiled);
    for path in [
        ".github/workflows/intake.md",
        ".github/workflows/intake.lock.yml",
        "scripts/check-build-approval.py",
        "AGENTS.md",
        "commit-types.json",
    ] {
        let target = fixture.root.join(path);
        let bytes = fs::read(&target).unwrap();
        let outside = fixture.root.join("symlink-target");
        fs::rename(&target, &outside).unwrap();
        symlink(&outside, &target).unwrap();
        let report = run_doctor_with_compiled_inputs(
            &config,
            &fixture.root,
            &FakeGitHub::matching(),
            Some(&compiled),
        );
        let id = if ["AGENTS.md", "commit-types.json"].contains(&path) {
            "local.agent.source-plan".to_owned()
        } else {
            agent_asset_id(path)
        };
        assert!(!check(&report, &id).passed, "{report}");
        assert_eq!(fs::read(&outside).unwrap(), bytes);
        fs::remove_file(&target).unwrap();
        fs::rename(outside, target).unwrap();
    }
    let directory = fixture.root.join(".github/workflows");
    let outside = fixture.root.join("outside-workflows");
    fs::rename(&directory, &outside).unwrap();
    symlink(&outside, &directory).unwrap();
    let report = run_doctor_with_compiled_inputs(
        &config,
        &fixture.root,
        &FakeGitHub::matching(),
        Some(&compiled),
    );
    assert!(!check(&report, &agent_asset_id(".github/workflows/intake.md")).passed);
    assert!(
        !check(
            &report,
            &agent_asset_id(".github/workflows/intake.lock.yml")
        )
        .passed
    );
    fs::remove_file(&directory).unwrap();
    fs::rename(outside, directory).unwrap();

    let linked = fixture.root.join("linked-review");
    symlink(&compiled, &linked).unwrap();
    let report = run_doctor_with_compiled_inputs(
        &config,
        &fixture.root,
        &FakeGitHub::matching(),
        Some(&linked),
    );
    assert!(!check(&report, "local.agent.compiled-inputs").passed);
    let lock = compiled.join(".github/workflows/intake.lock.yml");
    fs::remove_file(&lock).unwrap();
    symlink(
        fixture.root.join(".github/workflows/intake.lock.yml"),
        &lock,
    )
    .unwrap();
    let report = run_doctor_with_compiled_inputs(
        &config,
        &fixture.root,
        &FakeGitHub::matching(),
        Some(&compiled),
    );
    assert!(!check(&report, "local.agent.compiled-inputs").passed);
}

#[test]
fn missing_and_nonregular_agent_files_and_authored_prerequisites_fail() {
    let config = agent_config();
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
    fixture.adopt(&config, &compiled);
    for path in [
        ".github/workflows/intake.md",
        ".github/workflows/build-cloud.md",
        ".github/workflows/intake.lock.yml",
        "scripts/check-build-approval.py",
        "AGENTS.md",
        "commit-types.json",
    ] {
        let target = fixture.root.join(path);
        let bytes = fs::read(&target).unwrap();
        fs::remove_file(&target).unwrap();
        for directory in [false, true] {
            if directory {
                fs::create_dir(&target).unwrap();
            }
            let report = run_doctor_with_compiled_inputs(
                &config,
                &fixture.root,
                &FakeGitHub::matching(),
                Some(&compiled),
            );
            let id = if ["AGENTS.md", "commit-types.json"].contains(&path) {
                "local.agent.source-plan".to_owned()
            } else {
                agent_asset_id(path)
            };
            assert!(!check(&report, &id).passed, "{path}\n{report}");
            if directory {
                fs::remove_dir(&target).unwrap();
            }
        }
        fs::write(target, bytes).unwrap();
    }
    fs::write(fixture.root.join("commit-types.json"), "{}").unwrap();
    let report = run_doctor(&config, &fixture.root, &FakeGitHub::matching());
    assert!(!check(&report, "local.agent.source-plan").passed);
}

#[test]
fn model_roles_and_runtime_expressions_are_separate_from_unavailable_usage_evidence() {
    use acme::config::ModelId;
    for (controller, detection) in [
        (None, None),
        (Some("gpt-4.1"), None),
        (None, Some("gpt-4.1")),
        (Some("gpt-4.1"), Some("gpt-5.1")),
    ] {
        let mut config = agent_config();
        config.engines.intake.controller_model =
            controller.map(|model| ModelId::try_from(model.to_owned()).unwrap());
        config.engines.intake.detection_model =
            detection.map(|model| ModelId::try_from(model.to_owned()).unwrap());
        config.engines.build_cloud.controller_model =
            controller.map(|model| ModelId::try_from(model.to_owned()).unwrap());
        config.engines.build_cloud.detection_model =
            detection.map(|model| ModelId::try_from(model.to_owned()).unwrap());
        config.engines.build_cloud.model = "gpt-4.1".to_owned();
        let fixture = AgentFixture::new(&config);
        let compiled = fixture.compiled();
        fixture.adopt(&config, &compiled);
        let report = run_doctor_with_compiled_inputs(
            &config,
            &fixture.root,
            &FakeGitHub::matching(),
            Some(&compiled),
        );
        assert!(report.is_success(), "{report}");
        for module in ["intake", "build-cloud"] {
            for (role, model) in [("controller", controller), ("detection", detection)] {
                assert!(
                    check(&report, &format!("agent.models.{module}.{role}.configured"))
                        .observed
                        .contains(model.unwrap_or("omitted")),
                    "configured model role is missing"
                );
                assert!(
                    check(
                        &report,
                        &format!("agent.models.{module}.{role}.runtime-selection")
                    )
                    .observed
                    .contains("vars.GH_AW_"),
                    "runtime selection is missing"
                );
            }
            let evidence =
                &check(&report, &format!("agent.models.{module}.usage-evidence")).observed;
            for (selection, expected) in [
                ("controller.runtime-selection", CONTROLLER_SELECTION),
                ("detection.runtime-selection", DETECTION_SELECTION),
                ("detection.runtime-fallback", DETECTION_FALLBACK),
            ] {
                assert_eq!(
                    check(&report, &format!("agent.models.{module}.{selection}")).observed,
                    expected
                );
            }
            assert!(
                evidence.contains("unavailable")
                    && evidence.contains("serving model unknown")
                    && evidence.contains("separation is not verified")
            );
        }
        assert_eq!(
            check(&report, "agent.models.build-cloud.builder.configured").observed,
            "gpt-4.1"
        );
        assert_eq!(
            check(
                &report,
                "agent.models.build-cloud.builder.runtime-selection"
            )
            .observed,
            "gpt-6.1-sol"
        );
    }
}

#[test]
fn malformed_and_drifted_agent_remote_metadata_fails_without_exposing_response_fields() {
    let config = agent_config();
    for (endpoint, id, responses) in [
        (INTAKE_STATE, "github.agent.intake.workflow-state", vec![]),
        (
            INTAKE_STATE,
            "github.agent.intake.workflow-state",
            vec![json!({"state": "disabled_manually"})],
        ),
        (
            BUILD_STATE,
            "github.agent.build-cloud.workflow-state",
            vec![json!({"state": null})],
        ),
        (
            BUILD_STATE,
            "github.agent.build-cloud.workflow-state",
            vec![json!({"state": "unknown-secret-value"})],
        ),
        (
            BUILD_STATE,
            "github.agent.build-cloud.workflow-state",
            vec![json!({"state": "active"}), json!({"state": "active"})],
        ),
        (
            VARIABLES,
            "github.agent.variable.ACME_AGENT_WORKFLOWS_ENABLED",
            vec![
                json!({"total_count": 1, "variables": [{"name": "ACME_AGENT_WORKFLOWS_ENABLED", "value": "false"}]}),
            ],
        ),
        (
            VARIABLES,
            "github.agent.variables",
            vec![
                json!({"total_count": 1, "variables": [{"name": "ACME_AGENT_WORKFLOWS_ENABLED", "value": true}]}),
            ],
        ),
        (
            VARIABLES,
            "github.agent.variable.ACME_CLOUD_ASSIGNMENT_ENABLED",
            vec![json!({"total_count": 1, "variables": [{"name": "wrong", "value": "true"}]})],
        ),
        (
            VARIABLES,
            "github.agent.variable.ACME_CLOUD_ASSIGNMENT_ENABLED",
            vec![
                json!({"total_count": 1, "variables": [{"name": "ACME_CLOUD_ASSIGNMENT_ENABLED", "value": "unknown-secret-value"}]}),
            ],
        ),
        (
            VARIABLES,
            "github.agent.variables",
            vec![json!({"variables": [{"name": "ACME_AGENT_WORKFLOWS_ENABLED", "value": "true"}]})],
        ),
        (
            VARIABLES,
            "github.agent.variables",
            vec![
                json!({"total_count": 2, "variables": [{"name": "ACME_CLOUD_ASSIGNMENT_ENABLED", "value": "true"}]}),
                json!({"total_count": 2, "variables": [{"name": "ACME_CLOUD_ASSIGNMENT_ENABLED", "value": "true"}]}),
            ],
        ),
    ] {
        let mut github = FakeGitHub::matching();
        github.responses.insert(endpoint.to_owned(), responses);
        let report = run_doctor(&config, Path::new(ROOT), &github);
        assert!(!check(&report, id).passed, "{endpoint}\n{report}");
        assert!(!report.to_string().contains("unknown-secret-value"));
    }
}

#[test]
fn malformed_secret_metadata_fails_without_exposing_response_fields() {
    let config = agent_config();
    for (endpoint, id, responses) in [
        (
            SECRETS,
            "github.agent.secret-names",
            vec![json!([{"name": "GH_PAT"}])],
        ),
        (
            SECRETS,
            "github.agent.secret-names",
            vec![json!({"total_count": "1", "secrets": [{"name": "GH_PAT"}]})],
        ),
        (
            SECRETS,
            "github.agent.secret-names",
            vec![json!({"total_count": 1, "secrets": null})],
        ),
        (
            SECRETS,
            "github.agent.secret-names",
            vec![json!({"secrets": [{"name": "GH_PAT"}]})],
        ),
        (
            SECRETS,
            "github.agent.secret-names",
            vec![json!({"total_count": 2, "secrets": [{"name": "GH_PAT"}]})],
        ),
        (
            SECRETS,
            "github.agent.secret-names",
            vec![json!({"total_count": 1, "secrets": [{"name": "invalid-name"}]})],
        ),
        (
            SECRETS,
            "github.agent.secret-names",
            vec![json!({"total_count": 1, "secrets": [{"value": "unknown-secret-value"}]})],
        ),
        (
            SECRETS,
            "github.agent.secret-names",
            vec![json!({"total_count": 2, "secrets": [{"name": "GH_PAT"}, {"name": "gh_pat"}]})],
        ),
        (
            SECRETS,
            "github.agent.secret-names",
            vec![
                json!({"total_count": 1, "secrets": [{"name": "GH_PAT"}]}),
                json!({"total_count": 2, "secrets": []}),
            ],
        ),
    ] {
        let mut github = FakeGitHub::matching();
        github.responses.insert(endpoint.to_owned(), responses);
        let report = run_doctor(&config, Path::new(ROOT), &github);
        assert!(!check(&report, id).passed, "{endpoint}\n{report}");
        assert!(!report.to_string().contains("unknown-secret-value"));
    }
}

#[test]
fn remote_secret_metadata_is_paginated_and_checks_distinct_configured_names_only() {
    let mut config = agent_config();
    config.engines.intake.token_secret = "intake_inference".to_owned();
    config.engines.build_cloud.token_secret = "BUILD_INFERENCE".to_owned();
    config.modules.build_cloud.token_secret = "BUILD_ASSIGNMENT".to_owned();
    let mut github = FakeGitHub::matching();
    github.responses.insert(SECRETS.to_owned(), vec![
        json!({"total_count": 4, "secrets": [{"name": "INTAKE_INFERENCE", "value": "unknown-secret-value"}, {"name": "BUILD_INFERENCE"}]}),
        json!({"total_count": 4, "secrets": [{"name": "BUILD_ASSIGNMENT"}, {"name": "OTHER_NAME"}]}),
    ]);
    let report = run_doctor(&config, Path::new(ROOT), &github);
    for id in [
        "github.agent.secret-names",
        "github.agent.secret.intake.inference",
        "github.agent.secret.build-cloud.inference",
        "github.agent.secret.build-cloud.assignment",
    ] {
        assert!(check(&report, id).passed, "{report}");
    }
    let text = report.to_string();
    assert!(text.contains("OTHER_NAME"));
    assert!(!text.contains("unknown-secret-value"));
    assert!(text.contains("presence does not verify scopes or assignment capability"));

    github.responses.insert(
        SECRETS.to_owned(),
        vec![json!({"total_count": 0, "secrets": []})],
    );
    let report = run_doctor(&config, Path::new(ROOT), &github);
    assert!(check(&report, "github.agent.secret-names").passed);
    for role in [
        "intake.inference",
        "build-cloud.inference",
        "build-cloud.assignment",
    ] {
        assert!(
            !check(&report, &format!("github.agent.secret.{role}")).passed,
            "missing configured secret must fail"
        );
    }
}

#[test]
fn lowercase_secret_references_match_uppercase_names_and_reject_case_duplicates() {
    let mut config = agent_config();
    config.engines.intake.token_secret = "gh_pat".to_owned();
    config.engines.build_cloud.token_secret = "gh_pat".to_owned();
    config.modules.build_cloud.token_secret = "gh_pat".to_owned();
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
    fixture.adopt(&config, &compiled);
    let mut github = FakeGitHub::matching();
    let report = run_doctor_with_compiled_inputs(&config, &fixture.root, &github, Some(&compiled));
    assert!(report.is_success(), "{report}");
    for role in [
        "intake.inference",
        "build-cloud.inference",
        "build-cloud.assignment",
    ] {
        let credential = check(&report, &format!("github.agent.secret.{role}"));
        assert!(credential.passed);
        assert_eq!(
            credential.expected,
            "configured secret name gh_pat is present"
        );
    }

    github.responses.insert(
        SECRETS.to_owned(),
        vec![
            json!({"total_count": 2, "secrets": [{"name": "GH_PAT"}]}),
            json!({"total_count": 2, "secrets": [{"name": "gh_pat"}]}),
        ],
    );
    let report = run_doctor_with_compiled_inputs(&config, &fixture.root, &github, Some(&compiled));
    assert!(!check(&report, "github.agent.secret-names").passed);
}

#[test]
fn inactive_unprovisioned_credentials_fail_despite_stored_token_metadata() {
    for stored in [false, true] {
        let mut config = agent_config();
        config.modules.intake.workflow_enabled = false;
        config.modules.intake.staged = true;
        config.modules.build_cloud.workflow_enabled = false;
        config.modules.build_cloud.assignment_enabled = false;
        config.modules.build_cloud.staged = true;
        config.modules.build_cloud.token_configured = stored;
        config.modules.build_cloud.token_verified = stored;
        let fixture = AgentFixture::new(&config);
        let compiled = fixture.compiled();
        fixture.adopt(&config, &compiled);
        let mut github = FakeGitHub::matching();
        for endpoint in [INTAKE_STATE, BUILD_STATE] {
            github.responses.get_mut(endpoint).unwrap()[0]["state"] = json!("disabled_manually");
        }
        for name in [WORKFLOW_GATE, ASSIGNMENT_GATE] {
            set_gate(&mut github, name, "false");
        }
        github.responses.insert(
            SECRETS.to_owned(),
            vec![json!({"total_count": 0, "secrets": []})],
        );
        let report =
            run_doctor_with_compiled_inputs(&config, &fixture.root, &github, Some(&compiled));
        assert!(!report.is_success());
        let failures: Vec<_> = report
            .checks
            .iter()
            .filter(|check| !check.passed)
            .map(|check| {
                assert_eq!(check.observed, "configured secret name is absent");
                check.id.as_str()
            })
            .collect();
        assert_eq!(
            failures,
            [
                "github.agent.secret.intake.inference",
                "github.agent.secret.build-cloud.inference",
                "github.agent.secret.build-cloud.assignment",
            ],
            "inactive gates pass, but stored metadata cannot prove credential presence"
        );
        assert!(check(&report, "github.agent.secret-names").passed);
        assert!(
            github
                .calls
                .borrow()
                .iter()
                .all(|endpoint| { !endpoint.contains("dispatch") && !endpoint.contains("enable") })
        );
    }
}

#[test]
fn remote_agent_api_failures_and_missing_endpoints_are_explicit_failed_checks() {
    let config = agent_config();
    for (endpoint, id) in [
        (INTAKE_STATE, "github.agent.intake.workflow-state"),
        (BUILD_STATE, "github.agent.build-cloud.workflow-state"),
        (VARIABLES, "github.agent.variables"),
        (SECRETS, "github.agent.secret-names"),
    ] {
        let mut github = FakeGitHub::matching();
        github.responses.remove(endpoint);
        let report = run_doctor(&config, Path::new(ROOT), &github);
        assert!(!check(&report, id).passed);
        for error in [
            GitHubError::AuthenticationRequired,
            GitHubError::PermissionDenied,
            GitHubError::ApiFailure,
        ] {
            github.error = Some(error);
            let report = run_doctor(&config, Path::new(ROOT), &github);
            assert!(!check(&report, id).passed);
        }
    }
}

#[test]
fn unreadable_secret_metadata_is_not_reported_as_credential_absence() {
    struct SecretError {
        github: FakeGitHub,
        error: GitHubError,
    }

    impl GitHubTransport for SecretError {
        fn get(&self, endpoint: &str) -> Result<Vec<Value>, GitHubError> {
            if endpoint == SECRETS {
                Err(self.error)
            } else {
                self.github.get(endpoint)
            }
        }
    }

    let config = agent_config();
    for (error, observed) in [
        (
            GitHubError::AuthenticationRequired,
            "authentication required",
        ),
        (GitHubError::PermissionDenied, "insufficient permission"),
        (GitHubError::ApiFailure, "GitHub API request failed"),
    ] {
        let github = SecretError {
            github: FakeGitHub::matching(),
            error,
        };
        let report = run_doctor(&config, Path::new(ROOT), &github);
        let names = check(&report, "github.agent.secret-names");
        assert!(!names.passed);
        assert_eq!(names.observed, observed);
        assert!(
            !report
                .checks
                .iter()
                .any(|check| check.id.starts_with("github.agent.secret."))
        );
        assert!(
            !report
                .to_string()
                .contains("configured secret name is absent")
        );
    }
}

#[test]
fn disabled_agent_modules_do_not_require_review_inputs_or_agent_metadata_requests() {
    let mut config = agent_config();
    config.modules.intake.enabled = false;
    config.modules.build_cloud.enabled = false;
    let github = FakeGitHub::matching();
    let report = run_doctor(&config, Path::new(ROOT), &github);
    assert!(
        !report
            .checks
            .iter()
            .any(|check| check.id.starts_with("local.agent.")
                || check.id.starts_with("github.agent.")
                || check.id.starts_with("agent.models."))
    );
    assert!(github.calls.borrow().iter().all(|endpoint| {
        [LABELS, RULESETS, RULESET, REPOSITORY, ACTIONS].contains(&endpoint.as_str())
    }));
}

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
                (INTAKE_STATE.to_owned(), vec![json!({"state": "active"})]),
                (BUILD_STATE.to_owned(), vec![json!({"state": "active"})]),
                (
                    VARIABLES.to_owned(),
                    vec![json!({"total_count": 2, "variables": [
                        {"name": "ACME_AGENT_WORKFLOWS_ENABLED", "value": "true"},
                        {"name": "ACME_CLOUD_ASSIGNMENT_ENABLED", "value": "true"}
                    ]})],
                ),
                (
                    SECRETS.to_owned(),
                    vec![json!({"total_count": 1, "secrets": [{"name": "GH_PAT"}]})],
                ),
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
    let config = agent_config();
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
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

    let report =
        run_doctor_with_compiled_inputs(&config, Path::new(ROOT), &github, Some(&compiled));

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

fn label_mutations() -> [(&'static str, Mutation); 4] {
    [
        (
            "github.label.needs_spec",
            Mutation::Label("color", json!("000000")),
        ),
        (
            "github.label.needs_spec",
            Mutation::Label("description", json!("changed")),
        ),
        (
            "github.label.needs_spec",
            Mutation::Label("name", json!("Needs-Spec")),
        ),
        (
            "github.label.needs_spec",
            Mutation::Label(
                "description",
                json!("needs an explicit spec and acceptance scenarios"),
            ),
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
            Mutation::StatusCheck("strict_required_status_checks_policy", json!(true)),
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
        assert!(!report.to_string().contains("secret-value"));
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
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "acme-doctor-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

struct RepairGitHub {
    state: RefCell<FakeGitHub>,
    writes: RefCell<Vec<(GitHubMethod, String, Value)>>,
    fail_write: Option<usize>,
    ignore_writes: bool,
    fail_verification: bool,
}

impl RepairGitHub {
    fn matching() -> Self {
        let mut state = FakeGitHub::matching();
        let repository = &mut state.responses.get_mut(REPOSITORY).expect("repository")[0];
        repository["full_name"] = json!("tylerbutler/acme");
        repository["permissions"] = json!({"admin": true});
        repository["has_discussions"] = json!(true);
        state.responses.insert(
            RULESETS.to_owned(),
            vec![json!([{"id": 6, "name": "unmanaged"}, {"id": 7, "name": "main"}])],
        );
        Self {
            state: RefCell::new(state),
            writes: RefCell::new(Vec::new()),
            fail_write: None,
            ignore_writes: false,
            fail_verification: false,
        }
    }

    fn fix(&self, root: &Path) -> acme::DoctorReport {
        let config = agent_config();
        let fixture = AgentFixture::new(&config);
        let compiled = fixture.compiled();
        run_doctor_fix_with_compiled_inputs(
            &config,
            root,
            self,
            Some("tylerbutler/acme"),
            Some(&compiled),
        )
    }
}

impl GitHubTransport for RepairGitHub {
    fn get(&self, endpoint: &str) -> Result<Vec<Value>, GitHubError> {
        if self.fail_verification && !self.writes.borrow().is_empty() {
            return Err(GitHubError::ApiFailure);
        }
        self.state.borrow().get(endpoint)
    }

    fn write(&self, method: GitHubMethod, endpoint: &str, body: &Value) -> Result<(), GitHubError> {
        self.writes
            .borrow_mut()
            .push((method, endpoint.to_owned(), body.clone()));
        if self.fail_write == Some(self.writes.borrow().len()) {
            return Err(GitHubError::PermissionDenied);
        }
        if self.ignore_writes {
            return Ok(());
        }
        let mut state = self.state.borrow_mut();
        match (method, endpoint) {
            (GitHubMethod::Patch, REPOSITORY) | (GitHubMethod::Put, ACTIONS) => {
                let object = &mut state.responses.get_mut(endpoint).expect("settings")[0];
                for (field, value) in body.as_object().expect("settings patch") {
                    object[field] = value.clone();
                }
            }
            (GitHubMethod::Put, RULESET) => {
                state
                    .responses
                    .insert(RULESET.to_owned(), vec![body.clone()]);
            }
            (GitHubMethod::Post, "repos/tylerbutler/acme/rulesets") => {
                state.responses.insert(
                    RULESETS.to_owned(),
                    vec![json!([{"id": 6, "name": "unmanaged"}, {"id": 7, "name": "main"}])],
                );
                state
                    .responses
                    .insert(RULESET.to_owned(), vec![body.clone()]);
            }
            (GitHubMethod::Post, "repos/tylerbutler/acme/labels") => {
                let labels = state.responses.get_mut(LABELS).expect("labels")[0]
                    .as_array_mut()
                    .expect("labels array");
                if labels.iter().any(|label| {
                    label["name"]
                        .as_str()
                        .expect("label name")
                        .eq_ignore_ascii_case(body["name"].as_str().expect("new label name"))
                }) {
                    return Err(GitHubError::ApiFailure);
                }
                labels.push(body.clone());
            }
            (GitHubMethod::Patch, endpoint)
                if endpoint.eq_ignore_ascii_case("repos/tylerbutler/acme/labels/needs-spec") =>
            {
                let label = state.responses.get_mut(LABELS).expect("labels")[0]
                    .as_array_mut()
                    .expect("labels array")
                    .iter_mut()
                    .find(|label| {
                        label["name"]
                            .as_str()
                            .expect("label name")
                            .eq_ignore_ascii_case("needs-spec")
                    })
                    .expect("managed label");
                for (field, value) in body.as_object().expect("label patch") {
                    label[if field == "new_name" { "name" } else { field }] = value.clone();
                }
            }
            _ => panic!("unbounded write: {method:?} {endpoint} {body}"),
        }
        Ok(())
    }
}

#[test]
fn agent_drift_never_adds_repairs_to_the_existing_bounded_plan() {
    let config = agent_config();
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
    fixture.adopt(&config, &compiled);
    fs::write(
        fixture.root.join(".github/workflows/intake.md"),
        "unsafe change",
    )
    .unwrap();
    fs::write(
        fixture.root.join(".github/workflows/build-cloud.lock.yml"),
        "unsafe lock",
    )
    .unwrap();
    fs::write(
        fixture.root.join("scripts/check-build-approval.py"),
        "unsafe helper",
    )
    .unwrap();
    let github = RepairGitHub::matching();
    {
        let mut state = github.state.borrow_mut();
        Mutation::Label("color", json!("000000")).apply(&mut state);
        state.responses.get_mut(INTAKE_STATE).unwrap()[0]["state"] = json!("disabled_manually");
        state.responses.get_mut(BUILD_STATE).unwrap()[0]["state"] = json!("disabled_manually");
        set_gate(&mut state, WORKFLOW_GATE, "false");
        set_gate(&mut state, ASSIGNMENT_GATE, "false");
        state.responses.insert(
            SECRETS.to_owned(),
            vec![json!({"total_count": 0, "secrets": []})],
        );
    }
    let report = run_doctor_fix_with_compiled_inputs(
        &config,
        &fixture.root,
        &github,
        Some("tylerbutler/acme"),
        Some(&compiled),
    );
    assert!(!report.is_success());
    assert!(check(&report, "github.label.needs_spec").passed);
    assert!(
        check(&report, "local.agent.compiled-inputs").passed,
        "post-fix checks retain reviewed inputs"
    );
    assert_eq!(
        *github.writes.borrow(),
        [(
            GitHubMethod::Patch,
            format!("{REPOSITORY}/labels/needs-spec"),
            json!({"color": "D4C5F9"})
        )]
    );
    for endpoint in [INTAKE_STATE, BUILD_STATE, VARIABLES, SECRETS] {
        assert_eq!(
            github
                .state
                .borrow()
                .calls
                .borrow()
                .iter()
                .filter(|call| *call == endpoint)
                .count(),
            2,
            "fresh metadata checks: {endpoint}"
        );
    }
    assert_eq!(
        fs::read_to_string(fixture.root.join("scripts/check-build-approval.py")).unwrap(),
        "unsafe helper"
    );
}

#[test]
fn fix_revalidates_supplied_compiled_inputs_after_a_write() {
    struct ChangingReview {
        github: RepairGitHub,
        record: PathBuf,
    }
    impl GitHubTransport for ChangingReview {
        fn get(&self, endpoint: &str) -> Result<Vec<Value>, GitHubError> {
            self.github.get(endpoint)
        }

        fn write(
            &self,
            method: GitHubMethod,
            endpoint: &str,
            body: &Value,
        ) -> Result<(), GitHubError> {
            self.github.write(method, endpoint, body)?;
            fs::write(&self.record, "{}").unwrap();
            Ok(())
        }
    }
    let config = agent_config();
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
    fixture.adopt(&config, &compiled);
    let github = ChangingReview {
        github: RepairGitHub::matching(),
        record: compiled.join("compiled-workflows.json"),
    };
    Mutation::Label("color", json!("000000")).apply(&mut github.github.state.borrow_mut());
    let report = run_doctor_fix_with_compiled_inputs(
        &config,
        &fixture.root,
        &github,
        Some("tylerbutler/acme"),
        Some(&compiled),
    );
    assert!(
        !check(&report, "local.agent.compiled-inputs").passed,
        "{report}"
    );
    assert!(check(&report, "github.label.needs_spec").passed);
    assert_eq!(github.github.writes.borrow().len(), 1);
}

#[test]
fn old_fix_wrapper_keeps_the_no_input_failure_after_bounded_repairs() {
    let config = agent_config();
    let github = RepairGitHub::matching();
    Mutation::Label("color", json!("000000")).apply(&mut github.state.borrow_mut());
    let report = run_doctor_fix(&config, Path::new(ROOT), &github, Some("tylerbutler/acme"));
    assert!(!check(&report, "local.agent.compiled-inputs").passed);
    assert!(check(&report, "github.label.needs_spec").passed);
    assert_eq!(github.writes.borrow().len(), 1);
}

#[test]
fn fix_repairs_each_supported_setting_and_second_fix_is_a_no_op() {
    for (_, mutation) in label_mutations()
        .into_iter()
        .chain(ruleset_mutations())
        .chain(pull_request_mutations())
        .chain(status_check_mutations())
        .chain(repository_mutations())
    {
        let github = RepairGitHub::matching();
        mutation.apply(&mut github.state.borrow_mut());

        let report = github.fix(Path::new(ROOT));
        assert!(report.is_success(), "{mutation:?}\n{report}");
        assert_eq!(github.writes.borrow().len(), 1, "{mutation:?}");
        let report = github.fix(Path::new(ROOT));
        assert!(report.is_success(), "{report}");
        assert_eq!(github.writes.borrow().len(), 1, "second fix wrote state");
    }
}

#[test]
fn label_hex_color_case_is_not_drift_and_needs_no_write() {
    let github = RepairGitHub::matching();
    Mutation::Label("color", json!("d4c5f9")).apply(&mut github.state.borrow_mut());
    let config = agent_config();
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
    let report =
        run_doctor_with_compiled_inputs(&config, Path::new(ROOT), &github, Some(&compiled));
    assert!(report.is_success(), "{report}");
    let report = github.fix(Path::new(ROOT));
    assert!(report.is_success(), "{report}");
    assert!(github.writes.borrow().is_empty());
}

#[test]
fn fix_preserves_unrelated_resources_and_only_patches_drifted_settings() {
    let github = RepairGitHub::matching();
    {
        let mut state = github.state.borrow_mut();
        Mutation::Label("color", json!("000000")).apply(&mut state);
        Mutation::Repository("has_wiki", json!(true)).apply(&mut state);
        Mutation::Actions("default_workflow_permissions", json!("write")).apply(&mut state);
    }
    let before = github.state.borrow().responses.clone();
    let config = FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml")).expect("config");
    assert!(!run_doctor(&config, Path::new(ROOT), &github).is_success());
    assert!(
        github.writes.borrow().is_empty(),
        "read-only doctor wrote state"
    );

    let report = github.fix(Path::new(ROOT));
    assert!(report.is_success(), "{report}");
    let writes = github.writes.borrow();
    assert_eq!(writes.len(), 3);
    assert!(writes.contains(&(
        GitHubMethod::Patch,
        REPOSITORY.to_owned(),
        json!({"has_wiki": false})
    )));
    assert!(writes.contains(&(
        GitHubMethod::Put,
        ACTIONS.to_owned(),
        json!({"default_workflow_permissions": "read"})
    )));
    let after = &github.state.borrow().responses;
    assert_eq!(after[RULESETS], before[RULESETS]);
    assert_eq!(after[RULESET], before[RULESET]);
    assert_eq!(after[REPOSITORY][0]["has_discussions"], json!(true));
    assert_eq!(
        after[LABELS][0].as_array().expect("labels").last(),
        before[LABELS][0].as_array().expect("labels").last()
    );
}

#[test]
fn fix_creates_missing_managed_resources_without_deleting_unrelated_resources() {
    let github = RepairGitHub::matching();
    {
        let mut state = github.state.borrow_mut();
        state.responses.get_mut(LABELS).expect("labels")[0]
            .as_array_mut()
            .expect("labels")
            .retain(|label| label["name"] != "needs-spec");
        state.responses.insert(
            RULESETS.to_owned(),
            vec![json!([{"id": 6, "name": "unmanaged"}])],
        );
    }
    let report = github.fix(Path::new(ROOT));
    assert!(report.is_success(), "{report}");
    let writes = github.writes.borrow();
    assert_eq!(writes.len(), 2);
    assert!(
        writes
            .iter()
            .all(|(method, _, _)| *method == GitHubMethod::Post)
    );
    assert_eq!(
        github.state.borrow().responses[RULESETS][0][0],
        json!({"id": 6, "name": "unmanaged"})
    );
}

#[test]
fn fix_validates_the_whole_plan_before_any_write() {
    for (endpoint, response) in [
        (
            REPOSITORY,
            json!({"full_name": "other/repo", "permissions": {"admin": true}}),
        ),
        (
            REPOSITORY,
            json!({"full_name": "tylerbutler/acme", "permissions": {"admin": false}}),
        ),
        (REPOSITORY, json!({"full_name": "tylerbutler/acme"})),
        (
            RULESETS,
            json!([{"id": 7, "name": "main"}, {"id": 8, "name": "main"}]),
        ),
        (RULESETS, json!([{"name": "main"}])),
        (RULESETS, json!({"unexpected": "not an array"})),
        (ACTIONS, json!({"unexpected": "not settings"})),
    ] {
        let github = RepairGitHub::matching();
        {
            let mut state = github.state.borrow_mut();
            Mutation::Label("color", json!("000000")).apply(&mut state);
            state.responses.insert(endpoint.to_owned(), vec![response]);
        }
        let report = github.fix(Path::new(ROOT));
        assert!(!report.is_success(), "{endpoint}\n{report}");
        assert!(github.writes.borrow().is_empty(), "{endpoint}\n{report}");
    }
}

#[test]
fn fix_blocks_conflicting_checkout_identity_without_contacting_github() {
    let github = RepairGitHub::matching();
    let config = FactoryConfig::load(Path::new(ROOT).join("factory/factory.toml")).expect("config");
    let report = run_doctor_fix(&config, Path::new(ROOT), &github, Some("other/repo"));
    assert!(!report.is_success());
    assert!(report.to_string().contains("repair.plan"));
    assert!(github.writes.borrow().is_empty());
    assert!(github.state.borrow().calls.borrow().is_empty());
}

#[test]
fn fix_authentication_and_api_failures_never_write() {
    for error in [
        GitHubError::AuthenticationRequired,
        GitHubError::PermissionDenied,
        GitHubError::ApiFailure,
    ] {
        let github = RepairGitHub::matching();
        github.state.borrow_mut().error = Some(error);
        let report = github.fix(Path::new(ROOT));
        assert!(!report.is_success(), "{report}");
        assert!(github.writes.borrow().is_empty());
    }
}

#[test]
fn fix_reports_partial_application_and_rechecks_after_write_failure() {
    let mut github = RepairGitHub::matching();
    github.fail_write = Some(2);
    {
        let mut state = github.state.borrow_mut();
        Mutation::Label("color", json!("000000")).apply(&mut state);
        Mutation::Repository("has_wiki", json!(true)).apply(&mut state);
        Mutation::Actions("default_workflow_permissions", json!("write")).apply(&mut state);
    }
    let report = github.fix(Path::new(ROOT));
    assert!(!report.is_success(), "{report}");
    assert_eq!(
        github.writes.borrow().len(),
        2,
        "stop after the first failed write"
    );
    let text = report.to_string();
    assert!(text.contains("repair.apply"));
    assert!(text.contains("1 of 3"));
    assert!(text.contains("rollback"));
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.id == "github.label.needs_spec" && check.passed })
    );
    assert!(report.checks.iter().any(|check| {
        check.id == "github.actions-defaults.default_workflow_permissions" && !check.passed
    }));
}

#[test]
fn fix_does_not_trust_successful_writes_without_verification() {
    let mut github = RepairGitHub::matching();
    github.ignore_writes = true;
    Mutation::Label("color", json!("000000")).apply(&mut github.state.borrow_mut());
    let report = github.fix(Path::new(ROOT));
    assert!(!report.is_success(), "{report}");
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.id == "github.label.needs_spec" && !check.passed })
    );
}

#[test]
fn fix_leaves_local_drift_manual() {
    let root = temporary_root();
    copy_assets(&root);
    let form = root.join(".github/ISSUE_TEMPLATE/idea.yml");
    fs::write(&form, "name: incomplete\n").expect("change form");
    let github = RepairGitHub::matching();
    Mutation::Label("color", json!("000000")).apply(&mut github.state.borrow_mut());
    let report = github.fix(&root);
    assert!(!report.is_success(), "{report}");
    assert!(
        report
            .checks
            .iter()
            .any(|check| { check.id == "github.label.needs_spec" && check.passed })
    );
    assert_eq!(
        fs::read_to_string(form).expect("read form"),
        "name: incomplete\n"
    );
    assert!(report.to_string().contains("doctor does not rewrite files"));
    fs::remove_dir_all(root).expect("remove temporary tree");
}

#[test]
fn fix_handles_pagination_and_blocks_inherited_or_ambiguous_resources() {
    let github = RepairGitHub::matching();
    {
        let mut state = github.state.borrow_mut();
        let labels = state.responses.get_mut(LABELS).expect("labels");
        let second_page = labels[0].as_array_mut().expect("labels").split_off(3);
        labels.push(json!(second_page));
        state.responses.insert(
            RULESETS.to_owned(),
            vec![
                json!([{"id": 6, "name": "unmanaged"}]),
                json!([{"id": 7, "name": "main"}]),
            ],
        );
        Mutation::Label("color", json!("000000")).apply(&mut state);
    }
    let report = github.fix(Path::new(ROOT));
    assert!(report.is_success(), "{report}");
    for (endpoint, response) in [
        (
            RULESETS,
            json!([{"id": 7, "name": "main", "source_type": "Organization"}]),
        ),
        (
            LABELS,
            json!([
                {"name": "needs-spec", "color": "000000", "description": "one"},
                {"name": "needs-spec", "color": "ffffff", "description": "two"}
            ]),
        ),
        (LABELS, json!([{"color": "000000"}])),
        (RULESET, json!(null)),
    ] {
        let github = RepairGitHub::matching();
        {
            let mut state = github.state.borrow_mut();
            Mutation::Repository("has_wiki", json!(true)).apply(&mut state);
            state.responses.insert(endpoint.to_owned(), vec![response]);
        }
        let report = github.fix(Path::new(ROOT));
        assert!(!report.is_success(), "{report}");
        assert!(github.writes.borrow().is_empty(), "{report}");
    }
}

#[test]
fn fix_fails_if_fresh_verification_cannot_read_the_remote_state() {
    let mut github = RepairGitHub::matching();
    github.fail_verification = true;
    Mutation::Label("color", json!("000000")).apply(&mut github.state.borrow_mut());
    let report = github.fix(Path::new(ROOT));
    assert!(!report.is_success(), "{report}");
    assert!(report.to_string().contains("GitHub API request failed"));
    assert_eq!(github.writes.borrow().len(), 1);
}

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git should start");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn checkout_identity_accepts_github_origins_and_rejects_unknown_or_conflicting_origins() {
    let root = temporary_root();
    fs::create_dir_all(&root).expect("temporary root");
    assert_eq!(checkout_repository(&root).expect("no checkout"), None);
    git(&root, &["init", "--quiet"]);
    assert!(
        checkout_repository(&root).is_err(),
        "missing origin must fail closed"
    );
    git(
        &root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tylerbutler/acme.git",
        ],
    );
    for url in [
        "https://github.com/tylerbutler/acme.git",
        "git@github.com:tylerbutler/acme.git",
        "ssh://git@github.com/tylerbutler/acme",
    ] {
        git(&root, &["remote", "set-url", "origin", url]);
        assert_eq!(
            checkout_repository(&root)
                .expect("GitHub identity")
                .as_deref(),
            Some("tylerbutler/acme")
        );
    }
    for url in [
        "https://other.example/tylerbutler/acme.git",
        "https://github.com/tylerbutler/acme/extra",
        "https://github.com/tylerbutler/acme?query=true",
        "https://github.com.evil.example/tylerbutler/acme",
    ] {
        git(&root, &["remote", "set-url", "origin", url]);
        assert!(checkout_repository(&root).is_err(), "{url}");
    }
    git(
        &root,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/tylerbutler/acme",
        ],
    );
    git(
        &root,
        &[
            "remote",
            "set-url",
            "--add",
            "origin",
            "https://github.com/other/repo",
        ],
    );
    assert!(checkout_repository(&root).is_err());
    fs::remove_dir_all(root).expect("remove temporary root");
}

#[cfg(unix)]
fn install_fake_gh(root: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let gh = root.join("gh");
    fs::write(
        &gh,
        r#"#!/usr/bin/env python3
import json
import os
import sys

args = sys.argv[1:]
assert args[0] == "api"
assert args[args.index("--hostname") + 1] == "github.com"
endpoint = args[-1]
path = os.environ["ACME_TEST_STATE"]
with open(path) as source:
    state = json.load(source)
if "--method" not in args:
    assert "--paginate" in args and "--slurp" in args
    print(json.dumps(state[endpoint]))
else:
    assert args[args.index("--method") + 1] == "PATCH"
    assert args[args.index("--input") + 1] == "-"
    assert endpoint == "repos/tylerbutler/acme/labels/needs-spec"
    body = json.load(sys.stdin)
    assert body == {"color": "D4C5F9"}
    for label in state["repos/tylerbutler/acme/labels?per_page=100"][0]:
        if label["name"] == "needs-spec":
            label.update(body)
    with open(path, "w") as target:
        json.dump(state, target)
    with open(path + ".writes", "a") as log:
        log.write(endpoint + "\n")
"#,
    )
    .expect("write fake gh");
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).expect("fake gh executable");
    for tool in ["gh-aw", "actionlint"] {
        let path = root.join(tool);
        fs::write(&path, "#!/usr/bin/env python3\nimport os\nfrom pathlib import Path\nPath(os.environ['ACME_TEST_STATE'] + '.compiler-called').touch()\nraise SystemExit(99)\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn cli_uses_explicit_bounded_writes_and_verifies_with_a_fake_gh_process() {
    use std::process::Command;

    let config = agent_config();
    let fixture = AgentFixture::new(&config);
    let compiled = fixture.compiled();
    fixture.adopt(&config, &compiled);
    let root = fixture.root.clone();
    git(&root, &["init", "--quiet"]);
    git(
        &root,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:tylerbutler/acme.git",
        ],
    );
    let github = RepairGitHub::matching();
    Mutation::Label("color", json!("000000")).apply(&mut github.state.borrow_mut());
    let state_path = root.join("state.json");
    fs::write(
        &state_path,
        serde_json::to_vec(&github.state.borrow().responses).expect("state JSON"),
    )
    .expect("write fake state");
    install_fake_gh(&root);
    let paths = std::env::join_paths(std::iter::once(root.clone()).chain(std::env::split_paths(
        &std::env::var_os("PATH").expect("PATH"),
    )))
    .expect("test PATH");
    let invoke = |fix: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_acme"));
        command
            .current_dir(&root)
            .env("PATH", &paths)
            .env("ACME_TEST_STATE", &state_path)
            .args(["doctor", "--config"])
            .arg(root.join("factory.toml"))
            .arg("--compiled-inputs")
            .arg(&compiled);
        if fix {
            command.arg("--fix");
        }
        command.output().expect("acme should start")
    };
    let output = invoke(false);
    assert!(!output.status.success());
    assert!(!root.join("state.json.writes").exists());
    for _ in 0..2 {
        let output = invoke(true);
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("PASS github.label.needs_spec"));
    }
    let output = invoke(false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!root.join("state.json.compiler-called").exists());
    assert_eq!(
        fs::read_to_string(root.join("state.json.writes"))
            .expect("write log")
            .lines()
            .count(),
        1
    );

    git(
        &root,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/other/repo",
        ],
    );
    let output = invoke(true);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("conflicts with the checkout"));
}
