use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const FACTORY: &str = include_str!("../factory/factory.toml");
const CI: &str = ".github/workflows/ci.yml";
const RULESET: &str = ".github/rulesets/main.json";
const GOLDENS: &[(&str, &[u8])] = &[
    (
        ".github/ISSUE_TEMPLATE/bug.yml",
        include_bytes!("../.github/ISSUE_TEMPLATE/bug.yml"),
    ),
    (
        ".github/ISSUE_TEMPLATE/config.yml",
        include_bytes!("../.github/ISSUE_TEMPLATE/config.yml"),
    ),
    (
        ".github/ISSUE_TEMPLATE/idea.yml",
        include_bytes!("../.github/ISSUE_TEMPLATE/idea.yml"),
    ),
    (
        ".github/ISSUE_TEMPLATE/spec-request.yml",
        include_bytes!("../.github/ISSUE_TEMPLATE/spec-request.yml"),
    ),
    (
        ".github/actions/setup-rust/action.yml",
        include_bytes!("../.github/actions/setup-rust/action.yml"),
    ),
    (
        ".github/labels.yml",
        include_bytes!("../.github/labels.yml"),
    ),
    (RULESET, include_bytes!("../.github/rulesets/main.json")),
    (CI, include_bytes!("../.github/workflows/ci.yml")),
];

fn config() -> toml::Value {
    let mut config: toml::Value = toml::from_str(FACTORY).unwrap();
    config["toolchain"]["acme"]["target_version"] = env!("CARGO_PKG_VERSION").into();
    config
}

struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "acme-render-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("factory")).unwrap();
        fs::write(
            path.join("factory/factory.toml"),
            toml::to_string(&config()).unwrap(),
        )
        .unwrap();
        Self(path)
    }

    fn render(&self, destination: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_acme"))
            .current_dir(&self.0)
            .args(["render", "--output"])
            .arg(destination)
            .env("PATH", self.0.join("no-programs"))
            .output()
            .unwrap()
    }

    fn changed(&self, change: impl FnOnce(&mut toml::Value)) -> BTreeMap<String, Vec<u8>> {
        let mut config = config();
        change(&mut config);
        fs::write(
            self.0.join("factory/factory.toml"),
            toml::to_string(&config).unwrap(),
        )
        .unwrap();
        let destination = self.0.join("changed");
        succeeds(&self.render(&destination));
        tree(&destination)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn succeeds(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace('\\', "/"),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn golden_tree() -> BTreeMap<String, Vec<u8>> {
    GOLDENS
        .iter()
        .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec()))
        .collect()
}

fn assert_changed_only(actual: &BTreeMap<String, Vec<u8>>, changes: &[(&str, String)]) {
    let mut expected = golden_tree();
    for (path, contents) in changes {
        expected.insert((*path).to_owned(), contents.as_bytes().to_vec());
    }
    assert_eq!(*actual, expected);
}

fn golden(path: &str) -> String {
    String::from_utf8(golden_tree().remove(path).unwrap()).unwrap()
}

#[test]
fn unchanged_factory_matches_every_golden_byte_and_repeats_identically() {
    let workspace = Workspace::new();
    let first = workspace.0.join("first");
    let second = workspace.0.join("nested/second");
    fs::create_dir(&first).unwrap();
    succeeds(&workspace.render(&first));
    succeeds(&workspace.render(&second));
    assert_eq!(tree(&first), golden_tree());
    assert_eq!(tree(&first), tree(&second));
    assert!(tree(&first).values().all(|bytes| !bytes.contains(&b'\r')));
}

#[test]
fn matching_pin_and_bootstrap_bypass_render_the_golden_inventory() {
    for (bootstrap, pin) in [(false, env!("CARGO_PKG_VERSION")), (true, "HEAD")] {
        let workspace = Workspace::new();
        let actual = workspace.changed(|config| {
            config["toolchain"]["acme"]["bootstrap"] = bootstrap.into();
            config["toolchain"]["acme"]["target_version"] = pin.into();
        });
        assert_eq!(actual, golden_tree());
    }
}

#[test]
fn rejected_version_pins_leave_missing_and_empty_destinations_untouched() {
    for pin in [
        "0.0.1", "0.1.0", "HEAD", "latest", "^0.1.0", "v0.1.0", "0.1.0 ",
    ] {
        for exists in [false, true] {
            let workspace = Workspace::new();
            let mut config: toml::Value = toml::from_str(FACTORY).unwrap();
            config["toolchain"]["acme"]["bootstrap"] = false.into();
            config["toolchain"]["acme"]["target_version"] = pin.into();
            fs::write(
                workspace.0.join("factory/factory.toml"),
                toml::to_string(&config).unwrap(),
            )
            .unwrap();
            let destination = workspace.0.join("output");
            if exists {
                fs::create_dir(&destination).unwrap();
            }
            let before = tree(&workspace.0);
            let output = workspace.render(&destination);
            assert!(!output.status.success(), "accepted {pin:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("toolchain.acme.target_version")
            );
            assert_eq!(tree(&workspace.0), before);
            assert_eq!(destination.exists(), exists);
        }
    }
}

#[test]
fn an_added_command_adds_exactly_one_step_and_does_not_execute_it() {
    let workspace = Workspace::new();
    let actual = workspace.changed(|config| {
        config["modules"]["ci"]["commands"]
            .as_array_mut()
            .unwrap()
            .push("echo executed > renderer-must-not-execute".into());
        config["modules"]["ci"]["step_names"]
            .as_array_mut()
            .unwrap()
            .push("Extra check".into());
    });
    assert_changed_only(
        &actual,
        &[(
            CI,
            format!(
                "{}      - name: Extra check\n        run: echo executed > renderer-must-not-execute\n",
                golden(CI)
            ),
        )],
    );
    assert!(!workspace.0.join("renderer-must-not-execute").exists());
}

#[test]
fn command_and_step_name_changes_affect_only_their_own_scalar() {
    for (field, value, before, after) in [
        (
            "commands",
            "cargo fmt --all",
            "run: cargo fmt --all -- --check",
            "run: cargo fmt --all",
        ),
        (
            "step_names",
            "Formatting",
            "name: Check formatting",
            "name: Formatting",
        ),
    ] {
        let workspace = Workspace::new();
        let actual = workspace.changed(|config| {
            config["modules"]["ci"][field][0] = value.into();
        });
        assert_changed_only(&actual, &[(CI, golden(CI).replace(before, after))]);
    }
}

#[test]
fn required_check_changes_only_the_job_name_and_ruleset_context() {
    let workspace = Workspace::new();
    let actual = workspace.changed(|config| {
        config["modules"]["ci"]["required_checks"][0] = "Quality".into();
    });
    assert_changed_only(
        &actual,
        &[
            (CI, golden(CI).replace("    name: CI", "    name: Quality")),
            (
                RULESET,
                golden(RULESET).replace("\"context\": \"CI\"", "\"context\": \"Quality\""),
            ),
        ],
    );
}

#[test]
fn approval_settings_change_only_their_ruleset_values() {
    for (field, value, before, after) in [
        (
            "required_approvals",
            toml::Value::Integer(2),
            "\"required_approving_review_count\": 0",
            "\"required_approving_review_count\": 2",
        ),
        (
            "require_extra_approval_for_unattributed_changes",
            toml::Value::Boolean(true),
            "\"require_extra_approval_for_unattributed_changes\": false",
            "\"require_extra_approval_for_unattributed_changes\": true",
        ),
    ] {
        let workspace = Workspace::new();
        let actual = workspace.changed(|config| config["modules"]["merge"][field] = value);
        assert_changed_only(
            &actual,
            &[(RULESET, golden(RULESET).replace(before, after))],
        );
    }
}

#[test]
fn disabled_modules_remove_only_their_inventory() {
    for (disabled, removed) in [
        ("ci", vec![CI, ".github/actions/setup-rust/action.yml"]),
        ("merge", vec![RULESET]),
        (
            "intake",
            vec![
                ".github/labels.yml",
                ".github/ISSUE_TEMPLATE/bug.yml",
                ".github/ISSUE_TEMPLATE/config.yml",
                ".github/ISSUE_TEMPLATE/idea.yml",
                ".github/ISSUE_TEMPLATE/spec-request.yml",
            ],
        ),
    ] {
        let workspace = Workspace::new();
        let actual = workspace.changed(|config| {
            config["modules"][disabled]["enabled"] = false.into();
            if disabled == "ci" {
                config["modules"]["spec"]["provides"]
                    .as_array_mut()
                    .unwrap()
                    .push("ci".into());
            } else if disabled == "intake" {
                config["modules"]["spec"]["enabled"] = false.into();
                config["modules"]["build_cloud"]["enabled"] = false.into();
            }
        });
        let mut expected = golden_tree();
        for path in removed {
            expected.remove(path);
        }
        assert_eq!(actual, expected);
    }
}

#[test]
fn external_ci_supports_multiple_json_escaped_required_checks() {
    let workspace = Workspace::new();
    let checks = ["Check \"one\"", "Check\\two\nthree"];
    let actual = workspace.changed(|config| {
        config["modules"]["ci"]["enabled"] = false.into();
        config["modules"]["spec"]["provides"]
            .as_array_mut()
            .unwrap()
            .push("ci".into());
        config["modules"]["ci"]["required_checks"] =
            toml::Value::Array(checks.iter().map(|value| (*value).into()).collect());
    });
    let ruleset: serde_json::Value = serde_json::from_slice(&actual[RULESET]).unwrap();
    assert_eq!(
        ruleset["rules"][3]["parameters"]["required_status_checks"],
        serde_json::json!([
            {"context": checks[0], "integration_id": 15368},
            {"context": checks[1], "integration_id": 15368},
        ])
    );
}

#[test]
fn scalar_escaping_cannot_inject_yaml_or_trigger_recursive_substitution() {
    for value in [
        "Check: value # comment",
        "false",
        "*alias",
        "line one\r\n      - run: injected",
        "quote \" and slash \\",
        "@@ACME_CI_STEPS@@ ${{ github.ref }}",
        "line\u{85}next\u{2028}next\u{2029}end",
    ] {
        let workspace = Workspace::new();
        let actual = workspace.changed(|config| {
            config["modules"]["ci"]["step_names"][0] = value.into();
            config["modules"]["ci"]["commands"][0] = value.into();
            config["modules"]["ci"]["required_checks"][0] = value.into();
        });
        let workflow = std::str::from_utf8(&actual[CI]).unwrap();
        for prefix in ["    name: ", "      - name: ", "        run: "] {
            let scalar = workflow
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap();
            assert_eq!(serde_json::from_str::<String>(scalar).unwrap(), value);
        }
        assert_eq!(workflow.matches("      - name: ").count(), 6);
        assert!(!workflow.contains('\r'));
        let ruleset: serde_json::Value = serde_json::from_slice(&actual[RULESET]).unwrap();
        assert_eq!(
            ruleset["rules"][3]["parameters"]["required_status_checks"][0]["context"],
            value
        );
    }
}

#[test]
fn yaml_forbidden_unicode_is_escaped_without_changing_the_scalar_value() {
    for (character, escaped) in [
        ('\u{7f}', "\\u007f"),
        ('\u{84}', "\\u0084"),
        ('\u{86}', "\\u0086"),
        ('\u{9f}', "\\u009f"),
        ('\u{fffe}', "\\ufffe"),
        ('\u{ffff}', "\\uffff"),
    ] {
        let workspace = Workspace::new();
        let value = format!("Check {character} end");
        let actual = workspace.changed(|config| {
            config["modules"]["ci"]["step_names"][0] = value.clone().into();
        });
        let workflow = std::str::from_utf8(&actual[CI]).unwrap();
        let scalar = workflow
            .lines()
            .find_map(|line| line.strip_prefix("      - name: "))
            .unwrap();
        assert_eq!(scalar, format!("\"Check {escaped} end\""));
        assert_eq!(serde_json::from_str::<String>(scalar).unwrap(), value);
        assert!(!workflow.contains(character));
    }
}

#[test]
fn nonempty_destinations_and_files_are_rejected_without_changes() {
    let workspace = Workspace::new();
    let destination = workspace.0.join("output");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join(".keep"), b"keep me").unwrap();
    let before = tree(&destination);
    let output = workspace.render(&destination);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("nonempty"));
    assert_eq!(tree(&destination), before);
    let file = workspace.0.join("file");
    fs::write(&file, b"keep me").unwrap();
    assert!(!workspace.render(&file).status.success());
    assert_eq!(fs::read(file).unwrap(), b"keep me");
}

#[test]
fn parent_components_cannot_bypass_destination_preflight() {
    let workspace = Workspace::new();
    let existing = workspace.0.join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("keep"), b"keep me").unwrap();
    let before = tree(&existing);
    let output = workspace.render(&existing.join("missing/.."));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(".."));
    assert_eq!(tree(&existing), before);
    assert!(!existing.join("missing").exists());
}

#[cfg(unix)]
#[test]
fn symlink_destinations_and_ancestors_are_rejected_before_writes() {
    use std::os::unix::fs::symlink;
    let workspace = Workspace::new();
    let outside = workspace.0.join("outside");
    fs::create_dir(&outside).unwrap();
    let link = workspace.0.join("link");
    symlink(&outside, &link).unwrap();
    for destination in [&link, &link.join("nested/output")] {
        assert!(!workspace.render(destination).status.success());
        assert!(tree(&outside).is_empty());
    }
    fs::remove_file(&link).unwrap();
    symlink(workspace.0.join("missing"), &link).unwrap();
    assert!(!workspace.render(&link).status.success());
    assert!(!workspace.0.join("missing").exists());
}

#[test]
fn invalid_config_or_plan_fails_before_creating_the_destination() {
    let workspace = Workspace::new();
    let fixture = fs::read_to_string(workspace.0.join("factory/factory.toml")).unwrap();
    for config in [
        "schema_version = [".to_owned(),
        fixture.replace("profile = \"cli\"", "profile = \"web\""),
        fixture.replace(
            "needs = [\"approved-spec\", \"ci\"]",
            "needs = [\"missing\"]",
        ),
    ] {
        fs::write(workspace.0.join("factory/factory.toml"), config).unwrap();
        let destination = workspace.0.join("not-created/output");
        assert!(!workspace.render(&destination).status.success());
        assert!(!workspace.0.join("not-created").exists());
    }
}

#[test]
fn render_requires_output_and_rejects_unknown_or_missing_arguments() {
    let workspace = Workspace::new();
    for args in [
        vec!["render"],
        vec!["render", "--output"],
        vec!["render", "--config"],
        vec!["render", "--fix"],
        vec!["render", "--output", "out", "--unknown"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_acme"))
            .current_dir(&workspace.0)
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!workspace.0.join("out").exists());
    }
}

#[test]
fn copied_binary_renders_with_explicit_config_without_checkout_or_tools() {
    let workspace = Workspace::new();
    let binary = workspace
        .0
        .join(format!("installed-acme{}", std::env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_acme"), &binary).unwrap();
    fs::rename(
        workspace.0.join("factory/factory.toml"),
        workspace.0.join("approved.toml"),
    )
    .unwrap();
    fs::remove_dir(workspace.0.join("factory")).unwrap();
    let output = Command::new(binary)
        .current_dir(&workspace.0)
        .args(["render", "--config", "approved.toml", "--output", "output"])
        .env("PATH", workspace.0.join("no-programs"))
        .output()
        .unwrap();
    succeeds(&output);
    assert_eq!(tree(&workspace.0.join("output")), golden_tree());
}
