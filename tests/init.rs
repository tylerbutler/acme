use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const CONFIG: &str = "factory/factory.toml";
const RECORD: &str = "factory/managed-files.json";
const CI: &str = ".github/workflows/ci.yml";

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "acme-init-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let workspace = Self(root);
        workspace.write(
            "Cargo.toml",
            "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        );
        workspace.write(
            "Cargo.lock",
            "version = 4\n[[package]]\nname = \"sample\"\nversion = \"0.1.0\"\n",
        );
        workspace.write("src/main.rs", "fn main() {}\n");
        workspace.write("factory/brief.md", "# Human brief\n");
        workspace.write("AGENTS.md", "# Authored instructions\n");
        workspace.write("docs/specs/example.md", "# Authored spec\n");
        let mut config: toml::Value =
            toml::from_str(include_str!("../factory/factory.toml")).unwrap();
        config["toolchain"]["acme"]["target_version"] = env!("CARGO_PKG_VERSION").into();
        for module in ["intake", "spec", "build_cloud"] {
            config["modules"][module]["enabled"] = false.into();
        }
        config["modules"]["intake"]["workflow_enabled"] = false.into();
        config["modules"]["build_cloud"]["workflow_enabled"] = false.into();
        config["modules"]["build_cloud"]["assignment_enabled"] = false.into();
        config["modules"]["ci"]["commands"] = toml::Value::Array(
            [
                "cargo fmt --all -- --check",
                "cargo clippy --locked --all-targets --all-features -- -D warnings",
                "cargo test --locked --all-features",
            ]
            .map(Into::into)
            .into(),
        );
        config["modules"]["ci"]["step_names"] =
            toml::Value::Array(["Formatting", "Clippy", "Tests"].map(Into::into).into());
        workspace.write(CONFIG, &toml::to_string(&config).unwrap());
        workspace
    }

    fn write(&self, path: &str, bytes: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn change(&self, change: impl FnOnce(&mut toml::Value)) {
        let mut config: toml::Value =
            toml::from_str(&fs::read_to_string(self.0.join(CONFIG)).unwrap()).unwrap();
        change(&mut config);
        self.write(CONFIG, &toml::to_string(&config).unwrap());
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_acme"))
            .current_dir(&self.0)
            .env("PATH", "")
            .args(args)
            .output()
            .unwrap()
    }

    fn rejects(&self, args: &[&str], message: &str) {
        let before = tree(&self.0);
        let output = self.run(args);
        assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(message),
            "expected {message:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(tree(&self.0), before);
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let bytes = if metadata.is_symlink() {
                format!("symlink:{}", fs::read_link(&path).unwrap().display()).into_bytes()
            } else if metadata.is_dir() {
                visit(root, &path, result);
                b"directory".to_vec()
            } else if metadata.is_file() {
                fs::read(&path).unwrap()
            } else {
                b"special".to_vec()
            };
            result.insert(path.strip_prefix(root).unwrap().to_owned(), bytes);
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

fn succeeds(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn init_matches_render_and_sync_and_preserves_authored_inputs_without_programs() {
    let workspace = Workspace::new();
    let before = tree(&workspace.0);
    let output = workspace.run(&["init"]);
    succeeds(&output);
    for (path, bytes) in before {
        assert_eq!(tree(&workspace.0)[&path], bytes);
    }
    for step in [
        "git diff",
        "git switch -c ",
        "git add",
        "git commit",
        "gh pr create --draft",
    ] {
        assert!(String::from_utf8_lossy(&output.stdout).contains(step));
    }
    let other = Workspace::new();
    succeeds(&other.run(&["sync"]));
    assert_eq!(tree(&workspace.0), tree(&other.0));
    succeeds(&workspace.run(&["sync", "--check"]));
    succeeds(&other.run(&["render", "--output", "rendered"]));
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(workspace.0.join(RECORD)).unwrap()).unwrap();
    assert_eq!(record["entries"].as_array().unwrap().len(), 3);
    for entry in record["entries"].as_array().unwrap() {
        let path = entry["path"].as_str().unwrap();
        assert_eq!(
            fs::read(workspace.0.join(path)).unwrap(),
            fs::read(other.0.join("rendered").join(path)).unwrap()
        );
    }
    assert!(!workspace.0.join(".github/workflows/intake.md").exists());
    assert!(!workspace.0.join("scripts").exists());
}

#[test]
fn alternate_config_is_copied_exactly_and_is_deterministic() {
    let source = Workspace::new();
    let approved = format!(
        "# Reviewed config\r\n{}",
        fs::read_to_string(source.0.join(CONFIG)).unwrap()
    );
    source.write("approved.toml", &approved);
    let first = Workspace::new();
    let second = Workspace::new();
    for target in [&first, &second] {
        fs::remove_file(target.0.join(CONFIG)).unwrap();
        succeeds(&target.run(&[
            "init",
            "--config",
            source.0.join("approved.toml").to_str().unwrap(),
        ]));
        assert_eq!(
            fs::read(target.0.join(CONFIG)).unwrap(),
            approved.as_bytes()
        );
    }
    assert_eq!(tree(&first.0), tree(&second.0));
}

#[test]
fn matching_config_and_exact_outputs_can_be_adopted() {
    let workspace = Workspace::new();
    succeeds(&workspace.run(&["render", "--output", "rendered"]));
    workspace.write(
        CI,
        &fs::read_to_string(workspace.0.join("rendered").join(CI)).unwrap(),
    );
    let config = fs::read_to_string(workspace.0.join(CONFIG)).unwrap();
    workspace.write("approved.toml", &config);
    let output = workspace.run(&["init", "--config", "approved.toml"]);
    succeeds(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let staging = stdout
        .lines()
        .find(|line| line.starts_with("git add "))
        .unwrap();
    assert!(
        staging.split_whitespace().any(|path| path == CI),
        "{staging}"
    );
    succeeds(&workspace.run(&["sync", "--check"]));
}

#[test]
fn next_steps_stage_exact_paths_not_unrelated_authored_files() {
    let workspace = Workspace::new();
    workspace.write(".github/workflows/human.yml", "# Authored workflow\n");
    let output = workspace.run(&["init"]);
    succeeds(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let staging = stdout
        .lines()
        .find(|line| line.starts_with("git add "))
        .unwrap();
    assert_eq!(
        staging,
        "git add -- .github/actions/setup-rust/action.yml .github/rulesets/main.json .github/workflows/ci.yml factory/factory.toml factory/managed-files.json"
    );
    assert_eq!(
        fs::read_to_string(workspace.0.join(".github/workflows/human.yml")).unwrap(),
        "# Authored workflow\n"
    );
}

#[test]
fn library_init_resolves_relative_config_against_the_supplied_root() {
    let workspace = Workspace::new();
    acme::render::init_repository(&workspace.0, Path::new(CONFIG)).unwrap();
    succeeds(&workspace.run(&["sync", "--check"]));
}

#[test]
fn configured_engines_and_budget_are_preserved_with_a_supported_human_spec_stage() {
    let workspace = Workspace::new();
    workspace.change(|config| {
        config["modules"]["spec"]["enabled"] = true.into();
        config["modules"]["spec"]["needs"] = Vec::<String>::new().into();
    });
    let approved = fs::read(workspace.0.join(CONFIG)).unwrap();
    let config = acme::FactoryConfig::from_toml(std::str::from_utf8(&approved).unwrap()).unwrap();
    assert_eq!(config.modules.spec.implementation, "human");
    for engine in [
        &config.engines.intake.engine,
        &config.engines.build_cloud.engine,
        &config.engines.review.engine,
    ] {
        assert_eq!(engine, "copilot");
    }
    succeeds(&workspace.run(&["init"]));
    assert_eq!(fs::read(workspace.0.join(CONFIG)).unwrap(), approved);
    succeeds(&workspace.run(&["sync", "--check"]));
}

#[test]
fn enabled_spec_requires_a_human_implementation_before_writes() {
    for implementation in ["copilot", "other"] {
        let workspace = Workspace::new();
        workspace.change(|config| {
            config["modules"]["spec"]["enabled"] = true.into();
            config["modules"]["spec"]["implementation"] = implementation.into();
            config["modules"]["spec"]["needs"] = Vec::<String>::new().into();
        });
        workspace.rejects(&["init"], "human");
    }
}

#[test]
fn conflicting_canonical_config_is_never_replaced() {
    let workspace = Workspace::new();
    let config = fs::read_to_string(workspace.0.join(CONFIG)).unwrap();
    workspace.write(
        "approved.toml",
        &(config + "\n# Different approved bytes\n"),
    );
    workspace.rejects(&["init", "--config", "approved.toml"], "conflicting");
}

#[test]
fn conflicting_output_is_detected_before_copying_config() {
    let workspace = Workspace::new();
    fs::rename(workspace.0.join(CONFIG), workspace.0.join("approved.toml")).unwrap();
    workspace.write(CI, "# Human workflow\n");
    workspace.rejects(&["init", "--config", "approved.toml"], "conflicting");
}

#[test]
fn existing_records_including_malformed_records_direct_to_sync() {
    let workspace = Workspace::new();
    succeeds(&workspace.run(&["init"]));
    workspace.rejects(&["init"], "acme sync");
    workspace.write(RECORD, "malformed");
    workspace.rejects(&["init"], "acme sync");
    fs::remove_file(workspace.0.join(RECORD)).unwrap();
    fs::create_dir(workspace.0.join(RECORD)).unwrap();
    workspace.rejects(&["init"], "acme sync");
}

#[test]
fn invalid_config_contracts_fail_without_writes() {
    let cases: &[(&str, &str, toml::Value, &str)] = &[
        ("schema_version", "", 99.into(), "schema version"),
        ("autonomy", "", "L4".into(), "L3"),
        ("profile", "", "web".into(), "preset"),
        ("languages", "", vec!["python"].into(), "preset"),
        ("toolchain.acme", "bootstrap", true.into(), "bootstrap"),
        (
            "toolchain.acme",
            "target_version",
            "0.1.0".into(),
            "target_version",
        ),
        (
            "toolchain.acme",
            "target_version",
            "HEAD".into(),
            "target_version",
        ),
        ("modules.intake", "enabled", true.into(), "agent"),
        ("modules.build_cloud", "enabled", true.into(), "agent"),
        ("modules.intake", "workflow_enabled", true.into(), "agent"),
        (
            "modules.build_cloud",
            "assignment_enabled",
            true.into(),
            "agent",
        ),
        ("engines.review", "enabled", true.into(), "agent"),
        (
            "coordinator",
            "automatic_dispatch",
            true.into(),
            "automatic_dispatch",
        ),
        (
            "modules.merge",
            "needs",
            vec!["missing"].into(),
            "missing capability",
        ),
        ("modules.release", "enabled", true.into(), "release"),
    ];
    for (section, field, value, message) in cases {
        let workspace = Workspace::new();
        workspace.change(|config| {
            if field.is_empty() {
                config[*section] = value.clone();
            } else {
                let mut table = config;
                for part in section.split('.') {
                    if table.get(part).is_none() {
                        table
                            .as_table_mut()
                            .unwrap()
                            .insert(part.to_owned(), toml::Value::Table(toml::map::Map::new()));
                    }
                    table = &mut table[part];
                }
                table
                    .as_table_mut()
                    .unwrap()
                    .insert((*field).to_owned(), value.clone());
            }
        });
        workspace.rejects(&["init"], message);
    }
}

#[test]
fn missing_and_malformed_configs_fail_without_writes() {
    let workspace = Workspace::new();
    workspace.write(CONFIG, "schema_version = [");
    workspace.rejects(&["init"], "parse");
    fs::remove_file(workspace.0.join(CONFIG)).unwrap();
    workspace.rejects(&["init"], "config");
}

#[test]
fn missing_project_inputs_fail_without_writes() {
    for path in ["Cargo.toml", "Cargo.lock", "src/main.rs"] {
        let workspace = Workspace::new();
        fs::remove_file(workspace.0.join(path)).unwrap();
        workspace.rejects(&["init"], path);
    }
}

#[test]
fn malformed_project_inputs_and_unsupported_layouts_fail_without_writes() {
    for (path, bytes, message) in [
        ("Cargo.toml", "", "package"),
        ("Cargo.toml", "[package]\nname = \"sample\"\n", "version"),
        (
            "Cargo.toml",
            "[workspace]\nmembers = []\n",
            "single-package",
        ),
        ("Cargo.lock", "not toml", "Cargo.lock"),
        ("src/main.rs", "", "source"),
        ("rust-toolchain.toml", "[toolchain", "rust-toolchain.toml"),
        (".cargo/config.toml", "[alias]\ntest = \"run\"\n", ".cargo"),
    ] {
        let workspace = Workspace::new();
        workspace.write(path, bytes);
        workspace.rejects(&["init"], message);
    }
}

#[test]
fn declared_targets_and_build_scripts_are_preflighted() {
    for (extra, message) in [
        (
            "[[bin]]\nname = \"other\"\npath = \"other.rs\"\n",
            "other.rs",
        ),
        ("[lib]\npath = \"../outside.rs\"\n", "path"),
    ] {
        let workspace = Workspace::new();
        let manifest = fs::read_to_string(workspace.0.join("Cargo.toml")).unwrap();
        workspace.write("Cargo.toml", &(manifest + extra));
        workspace.rejects(&["init"], message);
    }
    let workspace = Workspace::new();
    workspace.write(
        "Cargo.toml",
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\nbuild = \"build.rs\"\n",
    );
    workspace.rejects(&["init"], "build.rs");
}

#[test]
fn custom_library_target_and_matching_toolchain_are_supported() {
    let workspace = Workspace::new();
    fs::remove_file(workspace.0.join("src/main.rs")).unwrap();
    workspace.write(
        "Cargo.toml",
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\n[lib]\npath = \"code/lib.rs\"\n",
    );
    workspace.write("code/lib.rs", "pub fn sample() {}\n");
    workspace.write(
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.85\"\ncomponents = [\"clippy\", \"rustfmt\"]\n",
    );
    succeeds(&workspace.run(&["init"]));
}

#[test]
fn arbitrary_commands_and_unsupported_cargo_flags_are_not_accepted() {
    for command in [
        "just check",
        "python3 scripts/check-configs-sync.py",
        "cargo run --bin generate-schema",
        "cargo test; touch executed",
        "cargo test\ntrue",
        "cargo test --manifest-path other/Cargo.toml",
        "cargo +nightly test",
        "cargo test --features missing",
        "cargo test --target unknown",
        "cargo fmt",
    ] {
        let workspace = Workspace::new();
        workspace.change(|config| {
            config["modules"]["ci"]["commands"] = vec![command].into();
            config["modules"]["ci"]["step_names"] = vec!["Check"].into();
        });
        workspace.rejects(&["init"], "supported CI commands");
    }
}

#[test]
fn unlocked_cargo_check_does_not_require_a_lockfile() {
    let workspace = Workspace::new();
    workspace.change(|config| {
        config["modules"]["ci"]["commands"] = vec!["cargo check --all-targets"].into();
        config["modules"]["ci"]["step_names"] = vec!["Check"].into();
    });
    fs::remove_file(workspace.0.join("Cargo.lock")).unwrap();
    succeeds(&workspace.run(&["init"]));
}

#[test]
fn declared_bin_targets_need_explicit_paths() {
    let workspace = Workspace::new();
    workspace.write(
        "Cargo.toml",
        "[package]\nname = \"sample\"\nversion = \"0.1.0\"\n[[bin]]\nname = \"missing\"\n",
    );
    workspace.rejects(&["init"], "explicit");
}

#[test]
fn empty_lockfile_fails_before_writes() {
    let workspace = Workspace::new();
    workspace.write("Cargo.lock", "");
    workspace.rejects(&["init"], "Cargo.lock");
}

#[test]
fn init_leaves_cargo_compatibility_and_dependency_resolution_to_cargo() {
    let workspace = Workspace::new();
    workspace.write(
        "Cargo.toml",
        "[package]\nname = \"sample\"\nversion = \"0.2.0-beta.1\"\nedition = \"2021\"\nrust-version = \"1.84.1\"\n\
         [dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n\
         [patch.crates-io]\nother = { path = \"../other\" }\n",
    );
    workspace.write(
        "rust-toolchain.toml",
        "[toolchain]\nchannel = \"1.85\"\ncomponents = [\"rustfmt\", \"clippy\", \"llvm-tools-preview\"]\n",
    );
    succeeds(&workspace.run(&["init"]));
}

#[test]
fn all_supported_cargo_forms_are_accepted() {
    for command in [
        "cargo fmt -- --check",
        "cargo check",
        "cargo build --locked",
        "cargo test --no-default-features",
        "cargo clippy --all-targets -- -D warnings",
    ] {
        let workspace = Workspace::new();
        workspace.change(|config| {
            config["modules"]["ci"]["commands"] = vec![command].into();
            config["modules"]["ci"]["step_names"] = vec!["Check"].into();
        });
        succeeds(&workspace.run(&["init"]));
    }
}

#[cfg(unix)]
#[test]
fn external_config_symlink_and_symlink_ancestor_are_rejected() {
    use std::os::unix::fs::symlink;
    let source = Workspace::new();
    let target = Workspace::new();
    symlink(source.0.join(CONFIG), target.0.join("approved.toml")).unwrap();
    target.rejects(&["init", "--config", "approved.toml"], "symlink");
    symlink(source.0.join("factory"), target.0.join("approval")).unwrap();
    target.rejects(&["init", "--config", "approval/factory.toml"], "symlink");
}

#[test]
fn malformed_cli_arguments_do_not_mutate() {
    let workspace = Workspace::new();
    for (args, message) in [
        (vec!["init", "--config"], "requires a path"),
        (vec!["init", "--config", "--check"], "requires a path"),
        (vec!["init", "--config", ""], "requires a path"),
        (vec!["init", "--config", CONFIG, "--config", CONFIG], "once"),
        (vec!["init", "--check"], "unknown init argument"),
        (vec!["init", "extra"], "unknown init argument"),
    ] {
        workspace.rejects(&args, message);
    }
}

#[cfg(unix)]
#[test]
fn symlink_files_and_ancestors_are_rejected_without_writes() {
    use std::os::unix::fs::symlink;
    for path in [
        CONFIG,
        "Cargo.toml",
        "Cargo.lock",
        "src/main.rs",
        RECORD,
        CI,
    ] {
        let workspace = Workspace::new();
        let target = workspace.0.join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        if target.exists() {
            fs::rename(&target, workspace.0.join("saved")).unwrap();
        }
        symlink(workspace.0.join("saved"), &target).unwrap();
        workspace.rejects(
            &["init"],
            if path == RECORD {
                "acme sync"
            } else {
                "symlink"
            },
        );
    }
    for directory in ["factory", "src", ".github", ".github/workflows"] {
        let workspace = Workspace::new();
        let target = workspace.0.join(directory);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        if target.exists() {
            fs::rename(&target, workspace.0.join("saved")).unwrap();
        } else {
            fs::create_dir(workspace.0.join("saved")).unwrap();
        }
        symlink(workspace.0.join("saved"), &target).unwrap();
        workspace.rejects(&["init"], "symlink");
    }
}

#[test]
fn directories_in_place_of_input_and_output_files_are_rejected_without_writes() {
    for path in [CONFIG, "Cargo.toml", "Cargo.lock", "src/main.rs", CI] {
        let workspace = Workspace::new();
        let target = workspace.0.join(path);
        if target.exists() {
            fs::remove_file(&target).unwrap();
        }
        fs::create_dir_all(target).unwrap();
        workspace.rejects(&["init"], "regular file");
    }
}
