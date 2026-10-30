use acme::FactoryConfig;
use acme::render::{compile_agent_workflows, validate_compiled_inputs};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const RECORD: &str = "compiled-workflows.json";

#[test]
fn serializable_config_schema_remains_synchronized() {
    let expected: Value =
        serde_json::from_str(include_str!("../schema/factory.schema.json")).unwrap();
    let actual = serde_json::to_value(acme::config::factory_schema()).unwrap();
    assert_eq!(actual, expected);
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
    let bin = workspace.0.join("bin");
    fs::create_dir_all(&bin).unwrap();
    executable(
        &bin.join("gh-aw"),
        &format!(
            "#!/bin/sh\n\
             case \"$1\" in\n\
             version) printf 'gh aw version v0.93.5\\n'; exit 0;;\n\
             compile)\n\
             test \"$#\" -eq 9 || exit 85\n\
             test \"$*\" = 'compile --no-check-update --strict --action-mode action --action-tag v0.93.5 .github/workflows/build-cloud.md .github/workflows/intake.md' || exit 85\n\
             test -z \"$GH_TOKEN$GITHUB_TOKEN$GH_AW_MODEL$GH_AW_ACTION_TAG\" || exit 80\n\
             test -z \"$GH_AW_DEFAULT_DETECTION_MODEL$GHAW_POLICY_MODELS_ALLOWED$GHAW_POLICY_MODELS_BLOCKED\" || exit 84\n\
             test \"$HOME\" != '{root}' || exit 81\n\
             test \"$HTTPS_PROXY\" = 'http://127.0.0.1:9' || exit 82\n\
             test -z \"$NO_PROXY\" || exit 83\n\
             {behavior}\n\
             for source in .github/workflows/*.md; do\n\
               printf 'name: deterministic\\n' > \"${{source%.md}}.lock.yml\"\n\
             done\n\
             exit 0;;\n\
             esac\n\
             exit 90\n",
            root = workspace.0.display()
        ),
    );
    executable(
        &bin.join("actionlint"),
        "#!/bin/sh\ncase \"$1\" in -version) printf 'v1.7.12\\n';; esac\nexit 0\n",
    );
    run_cli(workspace, output, &bin)
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
    }
    fs::write(&record_path, &original).unwrap();
    let mut stale = workspace.config();
    stale.solo = !stale.solo;
    assert!(validate_compiled_inputs(&stale, &workspace.0, &directory).is_err());
    for extra in ["extra", "empty-directory"] {
        let path = directory.join(extra);
        if extra == "extra" {
            fs::write(&path, "extra").unwrap();
        } else {
            fs::create_dir(&path).unwrap();
        }
        assert!(validate_compiled_inputs(&config, &workspace.0, &directory).is_err());
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
    fs::write(&source, source_bytes).unwrap();
    fs::write(&record_path, original).unwrap();
    fs::remove_file(directory.join(".github/workflows/intake.lock.yml")).unwrap();
    assert!(validate_compiled_inputs(&config, &workspace.0, &directory).is_err());
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
        "#!/bin/sh\ncase \"$1\" in -version) printf 'v1.7.12\\n'; exit 0;; esac\nexit 19\n",
    );
    let result = run_cli(&workspace, "output", &bin);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("actionlint failed"));
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
