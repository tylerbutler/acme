use acme::FactoryConfig;
use acme::config::ModelId;
use acme::render::{compile_agent_workflows, validate_compiled_inputs};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const CONTROLLER_DEFAULT: &str =
    "${{ vars.GH_AW_MODEL_AGENT_COPILOT || vars.GH_AW_DEFAULT_MODEL_COPILOT || 'auto' }}";
const DETECTION_DEFAULT: &str =
    "${{ vars.GH_AW_MODEL_DETECTION_COPILOT || vars.GH_AW_DEFAULT_MODEL_COPILOT || 'detection' }}";
const DETECTION_FALLBACK: &str =
    "${{ vars.GH_AW_MODEL_DETECTION_COPILOT || vars.GH_AW_DEFAULT_MODEL_COPILOT || 'auto' }}";

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "acme-compiler-proof-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("AGENTS.md"), "Human-owned instructions.\n").unwrap();
        fs::write(
            path.join("commit-types.json"),
            include_bytes!("../commit-types.json"),
        )
        .unwrap();
        Self(path)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, found: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, found);
            } else {
                assert!(entry.file_type().unwrap().is_file());
                found.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    visit(root, root, &mut found);
    found
}

fn environment_values(lock: &str, job: &str, variable: &str) -> Vec<String> {
    let marker = format!("\n  {job}:\n");
    let (_, body) = lock.split_once(&marker).expect("compiled job");
    body.lines()
        .take_while(|line| {
            !line.starts_with("  ") || line.as_bytes().get(2).is_none_or(u8::is_ascii_whitespace)
        })
        .filter_map(|line| line.trim().strip_prefix(&format!("{variable}: ")))
        .map(|value| serde_json::from_str::<String>(value).unwrap_or_else(|_| value.to_owned()))
        .collect()
}

fn assert_compiled_models(
    outputs: &BTreeMap<String, Vec<u8>>,
    controller: &str,
    detection: &str,
    fallback: Option<&str>,
    reviewed: bool,
) {
    for (workflow, expected) in [
        (
            "intake",
            include_str!("../.github/workflows/intake.lock.yml"),
        ),
        (
            "build-cloud",
            include_str!("../.github/workflows/build-cloud.lock.yml"),
        ),
    ] {
        let path = format!(".github/workflows/{workflow}.lock.yml");
        let lock = std::str::from_utf8(&outputs[&path]).unwrap();
        if reviewed {
            assert_eq!(lock, expected, "{workflow} reviewed compiler output");
        }
        assert_eq!(
            environment_values(lock, "agent", "COPILOT_MODEL"),
            [controller]
        );
        assert_eq!(
            environment_values(lock, "detection", "COPILOT_MODEL"),
            [detection]
        );
        assert_eq!(
            environment_values(lock, "detection", "GH_AW_MODEL_FALLBACK"),
            fallback.into_iter().collect::<Vec<_>>()
        );
        if workflow == "build-cloud" {
            let configs = lock
                .lines()
                .filter_map(|line| {
                    line.trim()
                        .strip_prefix("GH_AW_SAFE_OUTPUTS_HANDLER_CONFIG: ")
                })
                .map(|value| {
                    let json: String = serde_json::from_str(value).unwrap();
                    serde_json::from_str::<serde_json::Value>(&json).unwrap()
                })
                .collect::<Vec<_>>();
            assert_eq!(configs.len(), 1);
            assert_eq!(configs[0]["assign_to_agent"]["model"], "gpt-6.1-sol");
        }
    }
}

#[test]
#[ignore = "requires installed gh-aw v0.93.5 and actionlint; run just prove-compilation"]
fn pinned_compiler_preserves_model_roles_and_repeatable_review_bytes() {
    for (controller, detection, expected_controller, expected_detection, expected_fallback) in [
        (
            None,
            None,
            CONTROLLER_DEFAULT,
            DETECTION_DEFAULT,
            Some(DETECTION_FALLBACK),
        ),
        (
            Some("gpt-4.1"),
            None,
            "gpt-4.1",
            DETECTION_DEFAULT,
            Some(DETECTION_FALLBACK),
        ),
        (
            None,
            Some("gpt-5-mini"),
            CONTROLLER_DEFAULT,
            "gpt-5-mini",
            None,
        ),
        (
            Some("gpt-4.1"),
            Some("gpt-5-mini"),
            "gpt-4.1",
            "gpt-5-mini",
            None,
        ),
        (Some("gpt-4.1"), Some("auto"), "gpt-4.1", "auto", None),
    ] {
        let repository = Workspace::new();
        let mut config = FactoryConfig::from_toml(include_str!("../factory/factory.toml")).unwrap();
        env!("CARGO_PKG_VERSION").clone_into(&mut config.toolchain.acme.target_version);
        config.engines.intake.controller_model =
            controller.map(|id| ModelId::try_from(id.to_owned()).unwrap());
        config.engines.build_cloud.controller_model =
            controller.map(|id| ModelId::try_from(id.to_owned()).unwrap());
        config.engines.intake.detection_model =
            detection.map(|id| ModelId::try_from(id.to_owned()).unwrap());
        config.engines.build_cloud.detection_model =
            detection.map(|id| ModelId::try_from(id.to_owned()).unwrap());
        fs::write(
            repository.0.join("factory.toml"),
            toml::to_string(&config).unwrap(),
        )
        .unwrap();
        let before = files(&repository.0);
        let first = Workspace::new();
        let second = Workspace::new();
        compile_agent_workflows(&config, &repository.0, &first.0.join("review")).unwrap();
        let repeated = Command::new(env!("CARGO_BIN_EXE_acme"))
            .current_dir(&repository.0)
            .env("GH_AW_DEFAULT_DETECTION_MODEL", "ambient-detector")
            .env("GHAW_POLICY_MODELS_ALLOWED", "ambient-only")
            .env("GHAW_POLICY_MODELS_BLOCKED", "*")
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .env_remove("GH_PAT")
            .args(["compile", "--config", "factory.toml", "--output"])
            .arg(second.0.join("review"))
            .output()
            .unwrap();
        assert!(
            repeated.status.success(),
            "{}",
            String::from_utf8_lossy(&repeated.stderr)
        );
        validate_compiled_inputs(&config, &repository.0, &first.0.join("review")).unwrap();
        validate_compiled_inputs(&config, &repository.0, &second.0.join("review")).unwrap();
        let outputs = files(&first.0.join("review"));
        assert_eq!(outputs, files(&second.0.join("review")));
        assert_eq!(files(&repository.0), before);
        assert_eq!(outputs.len(), 6);
        assert_eq!(
            outputs["scripts/check-build-approval.py"],
            include_bytes!("../scripts/check-build-approval.py")
        );
        assert_compiled_models(
            &outputs,
            expected_controller,
            expected_detection,
            expected_fallback,
            controller.is_none() && detection.is_none(),
        );
    }
}

#[test]
#[ignore = "requires installed gh-aw v0.93.5 and actionlint; run just prove-compilation"]
fn pinned_validator_compatibility_preserves_unrelated_errors() {
    let repository = Workspace::new();
    let mut config = FactoryConfig::from_toml(include_str!("../factory/factory.toml")).unwrap();
    env!("CARGO_PKG_VERSION").clone_into(&mut config.toolchain.acme.target_version);
    let review = repository.0.join("review");
    compile_agent_workflows(&config, &repository.0, &review).unwrap();
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(review.join("compiled-workflows.json")).unwrap()).unwrap();
    let options: Vec<_> = record["validator_options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|option| option.as_str().unwrap())
        .collect();
    let workflow = repository.0.join("validation.yml");
    for (queue_key, expression, expected_error) in [
        ("queue", "job.workflow_ref", None),
        (
            "queuee",
            "job.workflow_ref",
            Some("unexpected key \"queuee\""),
        ),
        (
            "queue",
            "job.workflow_reff",
            Some("property \"workflow_reff\" is not defined"),
        ),
        (
            "queue",
            "runner.workflow_ref",
            Some("property \"workflow_ref\" is not defined"),
        ),
    ] {
        fs::write(
            &workflow,
            format!(
                "name: Validation
on: push
concurrency:
  group: validation
  {queue_key}: max
jobs:
  example:
    runs-on: ubuntu-latest
    steps:
      - run: echo ok
        env:
          WORKFLOW: ${{{{ {expression} }}}}
",
            ),
        )
        .unwrap();
        let output = Command::new("actionlint")
            .args(&options)
            .arg(&workflow)
            .output()
            .unwrap();
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            output.status.success(),
            expected_error.is_none(),
            "{diagnostic}"
        );
        if let Some(expected) = expected_error {
            assert!(diagnostic.contains(expected), "{diagnostic}");
        }
    }
}
