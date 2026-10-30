use acme::FactoryConfig;
use acme::render::{compile_agent_workflows, validate_compiled_inputs};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "support/compiled_fixture.rs"]
pub mod compiled_fixture;

const RECORD: &str = "compiled-workflows.json";
const OWNERSHIP: &str = "factory/managed-files.json";

#[test]
fn full_public_apis_require_compiled_inputs_for_enabled_agents() {
    let workspace = Workspace::new();
    let before = snapshot(&workspace.0);
    let destination = workspace.0.join("rendered");
    let error = acme::render::render_to_directory(&workspace.config(), &destination).unwrap_err();
    assert!(error.to_string().contains("--compiled-inputs"));
    for check in [false, true] {
        let error =
            acme::render::sync_repository(&workspace.config(), &workspace.0, check).unwrap_err();
        assert!(error.to_string().contains("--compiled-inputs"));
    }
    let check = handoff_cli(&workspace, &["sync", "--config", "factory.toml", "--check"]);
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stderr).contains("--compiled-inputs"));
    assert!(!destination.exists());
    assert!(!workspace.0.join(OWNERSHIP).exists());
    assert_eq!(snapshot(&workspace.0), before);
}

#[cfg(unix)]
fn reviewed_inputs(workspace: &Workspace) -> PathBuf {
    assert!(compile_fixture(workspace, "review", "").status.success());
    workspace.0.join("review")
}

fn handoff_cli(workspace: &Workspace, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_acme"))
        .current_dir(&workspace.0)
        .env("PATH", workspace.0.join("no-programs"))
        .args(arguments)
        .output()
        .unwrap()
}

fn succeeds(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[derive(Debug, Eq, PartialEq)]
enum Snapshot {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
    Other,
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Snapshot> {
    fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Snapshot>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let state = if kind.is_symlink() {
                Snapshot::Symlink(fs::read_link(&path).unwrap())
            } else if kind.is_dir() {
                visit(root, &path, files);
                Snapshot::Directory
            } else if kind.is_file() {
                Snapshot::File(fs::read(&path).unwrap())
            } else {
                Snapshot::Other
            };
            files.insert(path.strip_prefix(root).unwrap().to_owned(), state);
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[cfg(unix)]
fn copy_tree(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[cfg(unix)]
fn rejects_sync(config: &FactoryConfig, workspace: &Workspace, directory: &Path) {
    let before = snapshot(&workspace.0);
    for check in [false, true] {
        assert!(
            acme::render::sync_repository_with_compiled_inputs(
                config,
                &workspace.0,
                check,
                Some(directory),
            )
            .is_err()
        );
        assert_eq!(snapshot(&workspace.0), before);
    }
}

#[cfg(unix)]
fn rejects_handoff(config: &FactoryConfig, workspace: &Workspace, directory: &Path) {
    let before = snapshot(&workspace.0);
    assert!(
        acme::render::render_to_directory_with_compiled_inputs(
            config,
            &workspace.0,
            &workspace.0.join("rejected-render"),
            Some(directory),
        )
        .is_err()
    );
    assert_eq!(snapshot(&workspace.0), before);
    rejects_sync(config, workspace, directory);
}

#[cfg(unix)]
#[test]
fn full_reviewed_golden_render_adoption_and_check_never_invoke_compiler() {
    let workspace = Workspace::new();
    let review = reviewed_inputs(&workspace);
    let validated = validate_compiled_inputs(&workspace.config(), &workspace.0, &review).unwrap();
    for output in &validated.outputs {
        let expected =
            fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(output.ownership.path)).unwrap();
        assert_eq!(output.contents, expected, "{}", output.ownership.path);
    }
    succeeds(&handoff_cli(
        &workspace,
        &[
            "render",
            "--config",
            "factory.toml",
            "--output",
            "rendered",
            "--compiled-inputs",
            "review",
        ],
    ));
    for path in [
        ".github/ISSUE_TEMPLATE/bug.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/ISSUE_TEMPLATE/idea.yml",
        ".github/ISSUE_TEMPLATE/spec-request.yml",
        ".github/actions/setup-rust/action.yml",
        ".github/labels.yml",
        ".github/rulesets/main.json",
        ".github/workflows/ci.yml",
    ] {
        assert_eq!(
            fs::read(workspace.0.join("rendered").join(path)).unwrap(),
            fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap(),
            "{path}"
        );
    }
    for output in &validated.outputs {
        assert_eq!(
            fs::read(workspace.0.join("rendered").join(output.ownership.path)).unwrap(),
            output.contents
        );
    }
    assert_eq!(
        snapshot(&workspace.0.join("rendered"))
            .values()
            .filter(|entry| matches!(entry, Snapshot::File(_)))
            .count(),
        13
    );
    // Adopt the entire rendered tree, not just the workflow files.
    copy_tree(&workspace.0.join("rendered"), &workspace.0);
    let arguments = [
        "sync",
        "--config",
        "factory.toml",
        "--compiled-inputs",
        "review",
    ];
    let unchanged = snapshot(&workspace.0);
    let before = handoff_cli(&workspace, &[arguments.as_slice(), &["--check"]].concat());
    assert!(!before.status.success());
    assert!(!workspace.0.join(OWNERSHIP).exists());
    assert_eq!(snapshot(&workspace.0), unchanged);
    let adoption = handoff_cli(&workspace, &arguments);
    succeeds(&adoption);
    assert_eq!(
        String::from_utf8(adoption.stdout).unwrap(),
        "update factory/managed-files.json\n"
    );
    let record = fs::read(workspace.0.join(OWNERSHIP)).unwrap();
    let entries: Value = serde_json::from_slice(&record).unwrap();
    assert_eq!(entries["entries"].as_array().unwrap().len(), 13);
    for output in &validated.outputs {
        assert!(
            entries["entries"].as_array().unwrap().iter().any(|entry| {
                entry["module"] == output.ownership.module.name()
                    && entry["path"] == output.ownership.path
                    && entry["sha256"] == format!("{:x}", Sha256::digest(&output.contents))
            }),
            "missing ownership entry for {}",
            output.ownership.path
        );
    }
    let adopted = snapshot(&workspace.0);
    let second = handoff_cli(&workspace, &arguments);
    succeeds(&second);
    assert_eq!(second.stdout, b"Sync is clean.\n");
    succeeds(&handoff_cli(
        &workspace,
        &[arguments.as_slice(), &["--check"]].concat(),
    ));
    assert_eq!(fs::read(workspace.0.join(OWNERSHIP)).unwrap(), record);
    assert_eq!(snapshot(&workspace.0), adopted);
}

#[test]
fn compiled_inputs_cli_parser_rejects_missing_empty_duplicate_and_unknown_arguments() {
    let workspace = Workspace::new();
    for command in ["render", "sync"] {
        for (extra, message) in [
            (
                vec!["--compiled-inputs"],
                "--compiled-inputs requires a path",
            ),
            (
                vec!["--compiled-inputs", ""],
                "--compiled-inputs requires a path",
            ),
            (
                vec!["--compiled-inputs", "--check"],
                "--compiled-inputs requires a path",
            ),
            (
                vec!["--compiled-inputs", "review", "--compiled-inputs", "other"],
                "--compiled-inputs must be specified once",
            ),
            (vec!["--compilation", "review"], "unknown"),
            (vec!["--config", "other"], "--config must be specified once"),
        ] {
            let before = snapshot(&workspace.0);
            let mut arguments = vec![command, "--config", "factory.toml"];
            if command == "render" {
                arguments.extend(["--output", "rendered"]);
            }
            arguments.extend(extra);
            let result = handoff_cli(&workspace, &arguments);
            assert!(!result.status.success());
            assert!(
                String::from_utf8_lossy(&result.stderr).contains(message),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(snapshot(&workspace.0), before);
        }
    }
}

#[test]
fn serializable_config_schema_remains_synchronized() {
    let expected: Value =
        serde_json::from_str(include_str!("../schema/factory.schema.json")).unwrap();
    let actual = serde_json::to_value(acme::config::factory_schema()).unwrap();
    assert_eq!(actual, expected);
}

#[cfg(unix)]
#[test]
fn all_agent_targets_reject_conflicts_and_modified_ownership_before_any_writes() {
    let workspace = Workspace::new();
    let review = reviewed_inputs(&workspace);
    let config = workspace.config();
    rejects_handoff(&config, &workspace, &workspace.0.join("missing-review"));
    rejects_handoff(&config, &workspace, &workspace.0.join("factory.toml"));
    let validated = validate_compiled_inputs(&config, &workspace.0, &review).unwrap();
    for output in &validated.outputs {
        let target = workspace.0.join(output.ownership.path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "authored conflict").unwrap();
        rejects_sync(&config, &workspace, &review);
        fs::remove_file(target).unwrap();
    }
    acme::render::sync_repository_with_compiled_inputs(&config, &workspace.0, false, Some(&review))
        .unwrap();
    for output in &validated.outputs {
        let target = workspace.0.join(output.ownership.path);
        fs::write(&target, "local edit").unwrap();
        rejects_sync(&config, &workspace, &review);
        let mut disabled = workspace.config();
        for module in [
            &mut disabled.modules.intake.enabled,
            &mut disabled.modules.spec.enabled,
            &mut disabled.modules.build_cloud.enabled,
        ] {
            *module = false;
        }
        let before = snapshot(&workspace.0);
        for check in [false, true] {
            let error = acme::render::sync_repository(&disabled, &workspace.0, check).unwrap_err();
            assert!(error.to_string().contains("modified owned file"));
            assert!(error.to_string().contains(output.ownership.path));
            assert_eq!(snapshot(&workspace.0), before);
        }
        fs::write(target, &output.contents).unwrap();
    }
    // Missing owned agent files are recreated, not treated as local modifications.
    for output in &validated.outputs {
        fs::remove_file(workspace.0.join(output.ownership.path)).unwrap();
    }
    let before = snapshot(&workspace.0);
    let report = acme::render::sync_repository_with_compiled_inputs(
        &config,
        &workspace.0,
        true,
        Some(&review),
    )
    .unwrap();
    assert_eq!(report.creates.len(), 5);
    assert_eq!(snapshot(&workspace.0), before);
    acme::render::sync_repository_with_compiled_inputs(&config, &workspace.0, false, Some(&review))
        .unwrap();
    for output in validated.outputs {
        assert_eq!(
            fs::read(workspace.0.join(output.ownership.path)).unwrap(),
            output.contents
        );
    }
}

#[cfg(unix)]
#[test]
fn handoff_checks_authored_prerequisites_without_overwriting_them() {
    let workspace = Workspace::new();
    let review = reviewed_inputs(&workspace);
    for (path, invalid) in [("AGENTS.md", ""), ("commit-types.json", "not json")] {
        let target = workspace.0.join(path);
        let original = fs::read(&target).unwrap();
        fs::remove_file(&target).unwrap();
        rejects_handoff(&workspace.config(), &workspace, &review);
        fs::write(&target, invalid).unwrap();
        rejects_handoff(&workspace.config(), &workspace, &review);
        fs::write(target, original).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn handoff_rejects_symlinked_and_nonregular_inputs_and_checkout_targets() {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;
    let workspace = Workspace::new();
    let review = reviewed_inputs(&workspace);
    let config = workspace.config();
    symlink(&review, workspace.0.join("review-link")).unwrap();
    rejects_handoff(&config, &workspace, &workspace.0.join("review-link"));
    let outputs = validate_compiled_inputs(&config, &workspace.0, &review).unwrap();
    for path in outputs
        .outputs
        .iter()
        .map(|output| output.ownership.path)
        .chain([RECORD])
    {
        let input = review.join(path);
        let saved = fs::read(&input).unwrap();
        fs::remove_file(&input).unwrap();
        rejects_handoff(&config, &workspace, &review);
        for dangling in [false, true] {
            let target = if dangling { "absent" } else { "AGENTS.md" };
            symlink(workspace.0.join(target), &input).unwrap();
            rejects_handoff(&config, &workspace, &review);
            fs::remove_file(&input).unwrap();
        }
        let listener = UnixListener::bind(&input).unwrap();
        rejects_handoff(&config, &workspace, &review);
        drop(listener);
        fs::remove_file(&input).unwrap();
        fs::create_dir(&input).unwrap();
        rejects_handoff(&config, &workspace, &review);
        fs::remove_dir(&input).unwrap();
        fs::write(input, saved).unwrap();
    }
    for path in outputs
        .outputs
        .iter()
        .map(|output| output.ownership.path)
        .chain([".github/workflows", "scripts"])
    {
        let target = workspace.0.join(path);
        if matches!(path, ".github/workflows" | "scripts") {
            fs::remove_dir(&target).unwrap();
        }
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        for dangling in [false, true] {
            let outside = if dangling { "absent" } else { "AGENTS.md" };
            symlink(workspace.0.join(outside), &target).unwrap();
            rejects_sync(&config, &workspace, &review);
            fs::remove_file(&target).unwrap();
        }
        let listener = UnixListener::bind(&target).unwrap();
        rejects_sync(&config, &workspace, &review);
        drop(listener);
        fs::remove_file(&target).unwrap();
        fs::create_dir(&target).unwrap();
        // Parent directories are valid; leaf directories are not.
        if !matches!(path, ".github/workflows" | "scripts") {
            rejects_sync(&config, &workspace, &review);
        }
        fs::remove_dir(&target).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn module_removal_uses_only_remaining_compilation_and_preserves_authored_files() {
    let workspace = Workspace::new();
    let review = reviewed_inputs(&workspace);
    let arguments = [
        "sync",
        "--config",
        "factory.toml",
        "--compiled-inputs",
        "review",
    ];
    succeeds(&handoff_cli(&workspace, &arguments));
    for (path, contents) in [
        ("docs/specs/project.md", "Authored spec.\n"),
        ("factory/brief.md", "Authored brief.\n"),
        ("scripts/unrelated.py", "# Unrelated helper.\n"),
        ("commit-types.json", "Authored, now invalid JSON.\n"),
    ] {
        let target = workspace.0.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, contents).unwrap();
    }
    let authored: BTreeMap<_, _> = [
        "AGENTS.md",
        "commit-types.json",
        "docs/specs/project.md",
        "factory/brief.md",
        "scripts/unrelated.py",
    ]
    .map(|path| (path, fs::read(workspace.0.join(path)).unwrap()))
    .into();
    let mut config = workspace.config();
    config.modules.build_cloud.enabled = false;
    workspace.write_config(&config);
    rejects_handoff(&config, &workspace, &review);
    fs::remove_dir_all(&review).unwrap();
    // The fake compiler checks that only Intake is requested.
    let review = reviewed_inputs(&workspace);
    let validated = validate_compiled_inputs(&config, &workspace.0, &review).unwrap();
    assert_eq!(validated.outputs.len(), 2);
    let before = snapshot(&workspace.0);
    let report = acme::render::sync_repository_with_compiled_inputs(
        &config,
        &workspace.0,
        true,
        Some(&review),
    )
    .unwrap();
    assert_eq!(
        report.removals,
        [
            ".github/workflows/build-cloud.lock.yml",
            ".github/workflows/build-cloud.md",
            "scripts/check-build-approval.py",
        ]
    );
    assert_eq!(snapshot(&workspace.0), before);
    succeeds(&handoff_cli(&workspace, &arguments));
    for path in &report.removals {
        assert!(!workspace.0.join(path).exists());
    }
    for output in validated.outputs {
        assert_eq!(
            fs::read(workspace.0.join(output.ownership.path)).unwrap(),
            output.contents,
        );
    }
    config.modules.intake.enabled = false;
    config.modules.spec.enabled = false;
    // No disabled agent/compiler validation is needed for the M2-only path.
    "v0.0.0".clone_into(&mut config.toolchain.gh_aw.version);
    workspace.write_config(&config);
    fs::remove_dir_all(review).unwrap();
    fs::remove_dir_all(workspace.0.join("bin")).unwrap();
    let before = snapshot(&workspace.0);
    let check = handoff_cli(&workspace, &["sync", "--config", "factory.toml", "--check"]);
    assert!(!check.status.success());
    assert_eq!(snapshot(&workspace.0), before);
    succeeds(&handoff_cli(
        &workspace,
        &["sync", "--config", "factory.toml"],
    ));
    for path in [
        ".github/workflows/intake.md",
        ".github/workflows/intake.lock.yml",
        ".github/labels.yml",
        ".github/ISSUE_TEMPLATE/bug.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/ISSUE_TEMPLATE/idea.yml",
        ".github/ISSUE_TEMPLATE/spec-request.yml",
    ] {
        assert!(!workspace.0.join(path).exists());
    }
    for (path, contents) in authored {
        assert_eq!(fs::read(workspace.0.join(path)).unwrap(), contents);
    }
    succeeds(&handoff_cli(
        &workspace,
        &["sync", "--config", "factory.toml", "--check"],
    ));
}

#[cfg(unix)]
#[test]
fn no_agents_rejects_stale_explicit_inputs_and_accepts_a_valid_empty_review() {
    let workspace = Workspace::new();
    let stale = reviewed_inputs(&workspace);
    let mut config = workspace.config();
    config.modules.intake.enabled = false;
    config.modules.spec.enabled = false;
    config.modules.build_cloud.enabled = false;
    workspace.write_config(&config);
    rejects_handoff(&config, &workspace, &stale);
    fs::remove_file(workspace.0.join("AGENTS.md")).unwrap();
    fs::remove_file(workspace.0.join("commit-types.json")).unwrap();
    succeeds(&handoff_cli(
        &workspace,
        &["sync", "--config", "factory.toml"],
    ));
    // Explicit compilation of an empty plan never runs the fake compile action.
    let result = compile_fixture(&workspace, "empty-review", "exit 123");
    succeeds(&result);
    let empty = workspace.0.join("empty-review");
    assert!(
        validate_compiled_inputs(&config, &workspace.0, &empty)
            .unwrap()
            .outputs
            .is_empty()
    );
    fs::remove_dir_all(workspace.0.join("bin")).unwrap();
    succeeds(&handoff_cli(
        &workspace,
        &[
            "render",
            "--config",
            "factory.toml",
            "--output",
            "rendered",
            "--compiled-inputs",
            "empty-review",
        ],
    ));
    let before = snapshot(&workspace.0);
    succeeds(&handoff_cli(
        &workspace,
        &[
            "sync",
            "--config",
            "factory.toml",
            "--check",
            "--compiled-inputs",
            "empty-review",
        ],
    ));
    assert_eq!(snapshot(&workspace.0), before);
}

#[cfg(unix)]
#[test]
fn agent_ownership_inventory_rejects_forged_paths_even_when_modules_are_disabled() {
    let workspace = Workspace::new();
    let review = reviewed_inputs(&workspace);
    acme::render::sync_repository_with_compiled_inputs(
        &workspace.config(),
        &workspace.0,
        false,
        Some(&review),
    )
    .unwrap();
    let record_path = workspace.0.join(OWNERSHIP);
    let original = fs::read(&record_path).unwrap();
    let mut config = workspace.config();
    config.modules.intake.enabled = false;
    config.modules.spec.enabled = false;
    config.modules.build_cloud.enabled = false;
    for (module, path) in [
        ("intake", "AGENTS.md"),
        ("build_cloud", "commit-types.json"),
        ("build_cloud", "scripts/unrelated.py"),
        ("intake", "scripts/check-build-approval.py"),
        ("build_cloud", ".github/workflows/intake.lock.yml"),
        ("build-cloud", ".github/workflows/build-cloud.lock.yml"),
        ("build_cloud", "../escape"),
        ("build_cloud", "/absolute"),
        ("build_cloud", ".github\\workflows\\build-cloud.md"),
    ] {
        let mut record: Value = serde_json::from_slice(&original).unwrap();
        record["entries"][0]["module"] = module.into();
        record["entries"][0]["path"] = path.into();
        fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
        let before = snapshot(&workspace.0);
        for check in [false, true] {
            let error = acme::render::sync_repository(&config, &workspace.0, check).unwrap_err();
            assert!(error.to_string().contains("inventory"), "{error}");
            assert_eq!(snapshot(&workspace.0), before);
        }
    }
}

#[cfg(unix)]
#[test]
fn full_sync_partial_io_keeps_old_record_and_cleans_staged_files() {
    use std::os::unix::fs::PermissionsExt;
    for record_failure in [false, true] {
        let workspace = Workspace::new();
        reviewed_inputs(&workspace);
        let arguments = [
            "sync",
            "--config",
            "factory.toml",
            "--compiled-inputs",
            "review",
        ];
        succeeds(&handoff_cli(&workspace, &arguments));
        let record_path = workspace.0.join(OWNERSHIP);
        if record_failure {
            let mut record: Value =
                serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
            record["acme_version"] = "0.0.1".into();
            fs::write(&record_path, serde_json::to_vec(&record).unwrap()).unwrap();
        } else {
            fs::remove_file(workspace.0.join("scripts/check-build-approval.py")).unwrap();
        }
        let old_record = fs::read(&record_path).unwrap();
        let lock = workspace.0.join(".github/workflows/build-cloud.lock.yml");
        let lock_bytes = fs::read(&lock).unwrap();
        fs::remove_file(&lock).unwrap();
        let directory = workspace
            .0
            .join(if record_failure { "factory" } else { "scripts" });
        let permissions = fs::metadata(&directory).unwrap().permissions();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
        let result = handoff_cli(&workspace, &arguments);
        fs::set_permissions(directory, permissions).unwrap();
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains("sync I/O failure"), "{error}");
        assert!(error.contains("after 1 output changes"), "{error}");
        assert!(
            error.contains("ownership record was not advanced"),
            "{error}"
        );
        assert_eq!(fs::read(&record_path).unwrap(), old_record);
        assert_eq!(fs::read(&lock).unwrap(), lock_bytes);
        assert!(!snapshot(&workspace.0).keys().any(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".acme-sync-")
        }));
        succeeds(&handoff_cli(&workspace, &arguments));
        succeeds(&handoff_cli(
            &workspace,
            &[arguments.as_slice(), &["--check"]].concat(),
        ));
    }
}

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "acme-compiled-{}-{}",
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
        let mut config = FactoryConfig::from_toml(include_str!("../factory/factory.toml")).unwrap();
        env!("CARGO_PKG_VERSION").clone_into(&mut config.toolchain.acme.target_version);
        fs::write(path.join("factory.toml"), toml::to_string(&config).unwrap()).unwrap();
        Self(path)
    }

    fn config(&self) -> FactoryConfig {
        let mut config = FactoryConfig::load(self.0.join("factory.toml")).unwrap();
        env!("CARGO_PKG_VERSION").clone_into(&mut config.toolchain.acme.target_version);
        config
    }

    #[cfg(unix)]
    fn write_config(&self, config: &FactoryConfig) {
        fs::write(
            self.0.join("factory.toml"),
            toml::to_string(config).unwrap(),
        )
        .unwrap();
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[cfg(unix)]
fn executable(path: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[cfg(unix)]
fn compile_fixture(workspace: &Workspace, output: &str, behavior: &str) -> std::process::Output {
    compiled_fixture::compile_fixture(
        &workspace.0,
        Path::new("factory.toml"),
        &workspace.0.join(output),
        &workspace.0.join("bin"),
        behavior,
    )
}

#[cfg(unix)]
fn run_cli(workspace: &Workspace, output: &str, bin: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_acme"))
        .current_dir(&workspace.0)
        .env("PATH", bin)
        .env("HOME", &workspace.0)
        .env("GH_TOKEN", "test-token-must-not-reach-child")
        .env("GH_AW_MODEL", "ambient-override")
        .env("GH_AW_ACTION_TAG", "ambient-tag")
        .env("GH_AW_DEFAULT_DETECTION_MODEL", "ambient-detection-model")
        .env("GHAW_POLICY_MODELS_ALLOWED", "ambient-only-model")
        .env("GHAW_POLICY_MODELS_BLOCKED", "gpt-4.1")
        .args(["compile", "--config", "factory.toml", "--output", output])
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn isolated_compilation_is_repeatable_and_returns_owned_validated_bytes() {
    let workspace = Workspace::new();
    for output in ["first", "second"] {
        let result = compile_fixture(&workspace, output, "");
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let first = workspace.0.join("first");
    let validated = validate_compiled_inputs(&workspace.config(), &workspace.0, &first).unwrap();
    assert_eq!(validated.outputs.len(), 5);
    for output in validated.outputs {
        assert_eq!(
            output.contents,
            fs::read(workspace.0.join("second").join(output.ownership.path)).unwrap()
        );
    }
    assert_eq!(
        fs::read(first.join(RECORD)).unwrap(),
        fs::read(workspace.0.join("second").join(RECORD)).unwrap()
    );
    assert!(!workspace.0.join(".github").exists());
    assert!(!workspace.0.join("factory").exists());
}

#[cfg(unix)]
#[test]
fn validator_rejects_all_stale_edited_and_extra_inputs() {
    let workspace = Workspace::new();
    assert!(compile_fixture(&workspace, "output", "").status.success());
    let directory = workspace.0.join("output");
    let record_path = directory.join(RECORD);
    let original = fs::read(&record_path).unwrap();
    let config = workspace.config();
    for mutate in [
        |value: &mut Value| value["unexpected"] = true.into(),
        |value: &mut Value| value["format_version"] = 99.into(),
        |value: &mut Value| value["acme_version"] = "stale".into(),
        |value: &mut Value| value["compiler_version"] = "v0.0.0".into(),
        |value: &mut Value| value["config_sha256"] = "stale".into(),
        |value: &mut Value| value["action_cache_sha256"] = "stale".into(),
        |value: &mut Value| value["assets"]["extra"] = "stale".into(),
        |value: &mut Value| value["compiler_options"] = serde_json::json!([]),
        |value: &mut Value| value["validator_options"] = serde_json::json!([]),
        |value: &mut Value| value["outputs"][0]["unexpected"] = true.into(),
        |value: &mut Value| value["outputs"][0]["path"] = "../escape".into(),
        |value: &mut Value| value["outputs"][0]["path"] = "/absolute".into(),
        |value: &mut Value| value["outputs"][0]["module"] = "other".into(),
        |value: &mut Value| value["outputs"][0]["sha256"] = "bad".into(),
        |value: &mut Value| value["outputs"].as_array_mut().unwrap().reverse(),
        |value: &mut Value| {
            value["outputs"].as_array_mut().unwrap().pop();
        },
        |value: &mut Value| {
            let duplicate = value["outputs"][0].clone();
            value["outputs"].as_array_mut().unwrap().push(duplicate);
        },
    ] {
        let mut value: Value = serde_json::from_slice(&original).unwrap();
        mutate(&mut value);
        fs::write(&record_path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(validate_compiled_inputs(&config, &workspace.0, &directory).is_err());
        rejects_handoff(&config, &workspace, &directory);
    }
    fs::write(&record_path, &original).unwrap();
    let mut stale = workspace.config();
    stale.solo = !stale.solo;
    assert!(validate_compiled_inputs(&stale, &workspace.0, &directory).is_err());
    rejects_handoff(&stale, &workspace, &directory);
    for extra in ["extra", "empty-directory"] {
        let path = directory.join(extra);
        if extra == "extra" {
            fs::write(&path, "extra").unwrap();
        } else {
            fs::create_dir(&path).unwrap();
        }
        assert!(validate_compiled_inputs(&config, &workspace.0, &directory).is_err());
        rejects_handoff(&config, &workspace, &directory);
        if path.is_dir() {
            fs::remove_dir(path).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }
    let source = directory.join(".github/workflows/intake.md");
    let source_bytes = fs::read(&source).unwrap();
    fs::write(&source, "mutated source").unwrap();
    // Even an attacker updating the record hash cannot substitute expected source bytes.
    let mut value: Value = serde_json::from_slice(&original).unwrap();
    for entry in value["outputs"].as_array_mut().unwrap() {
        if entry["path"] == ".github/workflows/intake.md" {
            entry["sha256"] = format!("{:x}", Sha256::digest(b"mutated source")).into();
        }
    }
    fs::write(&record_path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(validate_compiled_inputs(&config, &workspace.0, &directory).is_err());
    rejects_handoff(&config, &workspace, &directory);
    fs::write(&source, source_bytes).unwrap();
    fs::write(&record_path, original).unwrap();
    fs::remove_file(directory.join(".github/workflows/intake.lock.yml")).unwrap();
    assert!(validate_compiled_inputs(&config, &workspace.0, &directory).is_err());
    rejects_handoff(&config, &workspace, &directory);
}

#[cfg(unix)]
#[test]
fn compilation_uses_an_isolated_git_root() {
    let workspace = Workspace::new();
    let output = compile_fixture(
        &workspace,
        "review",
        "test -f .git || exit 91\n\
         test \"$(git rev-parse --show-toplevel)\" = \"$PWD\" || exit 92\n\
         test \"$(git symbolic-ref --short HEAD)\" = 'acme' || exit 93\n\
         test -z \"$(git remote)\" || exit 94",
    );
    succeeds(&output);
    assert!(!workspace.0.join(".git").exists());
    let review = workspace.0.join("review");
    let validated = validate_compiled_inputs(&workspace.config(), &workspace.0, &review).unwrap();
    assert_eq!(validated.outputs.len(), 5);
    assert!(!review.join(".git").exists());
}

#[cfg(unix)]
#[test]
fn git_initialization_failures_never_publish_or_mutate_checkout() {
    let workspace = Workspace::new();
    succeeds(&compile_fixture(&workspace, "seed", ""));
    let bin = workspace.0.join("bin");
    fs::remove_file(bin.join("git")).unwrap();
    for failed in [false, true] {
        if failed {
            executable(
                &bin.join("git"),
                "#!/bin/sh\nprintf 'git initialization blocked\\n'\nexit 23\n",
            );
        }
        let before = snapshot(&workspace.0);
        let result = run_cli(&workspace, "output", &bin);
        assert!(!result.status.success());
        let diagnostic = String::from_utf8_lossy(&result.stderr);
        assert!(diagnostic.contains("git"), "{diagnostic}");
        if failed {
            assert!(
                diagnostic.contains("git initialization blocked"),
                "{diagnostic}"
            );
        }
        assert_eq!(snapshot(&workspace.0), before);
        assert!(!workspace.0.join("output").join(RECORD).exists());
    }
}

#[cfg(unix)]
#[test]
fn failures_never_publish_a_record_and_do_not_mutate_checkout() {
    for behavior in [
        "exit 12",
        "exit 0",
        "printf changed > .github/workflows/intake.md",
        "printf extra > unexpected",
        "/bin/mkdir unexpected-directory",
        "/bin/ln -s /etc/passwd unexpected-link",
    ] {
        let workspace = Workspace::new();
        let result = compile_fixture(&workspace, "output", behavior);
        assert!(!result.status.success(), "{behavior}");
        assert!(!workspace.0.join("output").join(RECORD).exists());
        assert!(!workspace.0.join(".github").exists());
    }
}

#[cfg(unix)]
#[test]
fn missing_wrong_or_failed_tools_are_explicit_and_never_publish() {
    let workspace = Workspace::new();
    let bin = workspace.0.join("bin");
    fs::create_dir(&bin).unwrap();
    let result = run_cli(&workspace, "output", &bin);
    assert!(!result.status.success());
    executable(
        &bin.join("gh-aw"),
        "#!/bin/sh\nprintf 'gh aw version v0.1.0\\n'\n",
    );
    let result = run_cli(&workspace, "output", &bin);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("actionlint"));
    executable(
        &bin.join("actionlint"),
        "#!/bin/sh\nprintf 'v1.7.12\\n'\nexit 0\n",
    );
    let result = run_cli(&workspace, "output", &bin);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("v0.93.5"));
    assert!(compile_fixture(&workspace, "good", "").status.success());
    for version in ["v1.7.11", "unknown", "v1.7.12-beta", ""] {
        executable(
            &bin.join("actionlint"),
            &format!("#!/bin/sh\nprintf '{version}\\n'\nexit 0\n"),
        );
        let result = run_cli(&workspace, "output", &bin);
        assert!(!result.status.success(), "{version:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("actionlint >=1.7.12"));
        assert!(!workspace.0.join("output").join(RECORD).exists());
    }
    executable(
        &bin.join("actionlint"),
        "#!/bin/sh\ncase \"$1\" in -version) printf 'v1.7.12\\n'; exit 0;; esac\nprintf 'invalid workflow expression\\n'\nprintf 'validator context\\n' >&2\nexit 19\n",
    );
    let result = run_cli(&workspace, "output", &bin);
    assert!(!result.status.success());
    let diagnostic = String::from_utf8_lossy(&result.stderr);
    for expected in [
        "actionlint failed",
        "invalid workflow expression",
        "validator context",
    ] {
        assert!(diagnostic.contains(expected), "{diagnostic}");
    }
    assert!(!workspace.0.join("output").join(RECORD).exists());
    // The validator needs neither compiler nor actionlint.
    fs::remove_dir_all(bin).unwrap();
    assert!(
        validate_compiled_inputs(&workspace.config(), &workspace.0, &workspace.0.join("good"))
            .is_ok()
    );
}

#[cfg(unix)]
#[test]
fn validator_rejects_symlinked_roots_parents_files_and_fifos() {
    use std::os::unix::fs::symlink;
    let workspace = Workspace::new();
    assert!(compile_fixture(&workspace, "output", "").status.success());
    let directory = workspace.0.join("output");
    symlink(&directory, workspace.0.join("linked-output")).unwrap();
    assert!(
        validate_compiled_inputs(
            &workspace.config(),
            &workspace.0,
            &workspace.0.join("linked-output")
        )
        .is_err()
    );
    let source = directory.join(".github/workflows/intake.md");
    let saved = fs::read(&source).unwrap();
    fs::remove_file(&source).unwrap();
    symlink(workspace.0.join("AGENTS.md"), &source).unwrap();
    assert!(validate_compiled_inputs(&workspace.config(), &workspace.0, &directory).is_err());
    fs::remove_file(&source).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(&source)
            .status()
            .unwrap()
            .success()
    );
    assert!(validate_compiled_inputs(&workspace.config(), &workspace.0, &directory).is_err());
    fs::remove_file(&source).unwrap();
    fs::write(&source, saved).unwrap();
    fs::rename(directory.join(".github"), directory.join("moved")).unwrap();
    symlink(directory.join("moved"), directory.join(".github")).unwrap();
    assert!(validate_compiled_inputs(&workspace.config(), &workspace.0, &directory).is_err());
}

#[cfg(unix)]
#[test]
fn temporary_parent_must_not_be_inside_any_git_checkout() {
    let workspace = Workspace::new();
    assert!(compile_fixture(&workspace, "seed", "").status.success());
    for marker in ["directory", "worktree", "symlink"] {
        let external = Workspace::new();
        match marker {
            "worktree" => {
                fs::write(external.0.join(".git"), "gitdir: /unread/external/gitdir\n").unwrap();
            }
            "symlink" => {
                std::os::unix::fs::symlink(
                    external.0.join("missing-gitdir"),
                    external.0.join(".git"),
                )
                .unwrap();
            }
            _ => fs::create_dir(external.0.join(".git")).unwrap(),
        }
        let temporary = external.0.join("temporary");
        fs::create_dir(&temporary).unwrap();
        fs::write(external.0.join(".gitattributes"), "human-owned\n").unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_acme"))
            .current_dir(&workspace.0)
            .env("PATH", workspace.0.join("bin"))
            .env("HOME", &workspace.0)
            .env("TMPDIR", &temporary)
            .args(["compile", "--config", "factory.toml", "--output", "review"])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("Git checkout"));
        assert_eq!(fs::read_dir(&temporary).unwrap().count(), 0);
        assert_eq!(
            fs::read(external.0.join(".gitattributes")).unwrap(),
            b"human-owned\n"
        );
        assert!(!workspace.0.join("review").exists());
    }
}

#[cfg(unix)]
#[test]
fn review_destination_inside_a_future_workspace_survives_cleanup() {
    let workspace = Workspace::new();
    assert!(compile_fixture(&workspace, "seed", "").status.success());
    let external = Workspace::new();
    let temporary = external.0.join("temporary");
    fs::create_dir(&temporary).unwrap();
    // exec preserves the shell PID, making this exact future candidate known
    // without changing global process environment or the workspace counter.
    let child = Command::new("/bin/sh")
        .args([
            "-c",
            "exec \"$1\" compile --config factory.toml --output \"$TMPDIR/acme-compile-private-$$-0/home/review\"",
            "acme-overlap-probe",
            env!("CARGO_BIN_EXE_acme"),
        ])
        .current_dir(&workspace.0)
        .env("PATH", workspace.0.join("bin"))
        .env("HOME", &workspace.0)
        .env("TMPDIR", &temporary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let directory = temporary
        .join(format!("acme-compile-private-{}-0", child.id()))
        .join("home/review");
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        validate_compiled_inputs(&workspace.config(), &workspace.0, &directory).is_ok(),
        "successful review output was deleted during workspace cleanup"
    );
}

#[test]
fn compile_cli_rejects_invalid_arguments_before_loading_config() {
    for arguments in [
        vec!["compile"],
        vec!["compile", "--output"],
        vec!["compile", "--config"],
        vec!["compile", "--unknown"],
        vec!["compile", "--output", "a", "--output", "b"],
        vec!["compile", "--config", "a", "--config", "b", "--output", "c"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_acme"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("failed to read"));
    }
}

#[test]
fn unsafe_or_nonempty_destinations_are_rejected_before_tools() {
    let workspace = Workspace::new();
    for path in [&workspace.0, &workspace.0.join("../escape")] {
        assert!(compile_agent_workflows(&workspace.config(), &workspace.0, path).is_err());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&workspace.0, workspace.0.join("link")).unwrap();
        assert!(
            compile_agent_workflows(
                &workspace.config(),
                &workspace.0,
                &workspace.0.join("link/output")
            )
            .is_err()
        );
    }
}
