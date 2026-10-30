use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "support/compiled_fixture.rs"]
pub mod compiled_fixture;

const FACTORY: &str = include_str!("../factory/factory.toml");
const RECORD: &str = "factory/managed-files.json";
const CI: &str = ".github/workflows/ci.yml";
const LABELS: &str = ".github/labels.yml";

struct Workspace(PathBuf, compiled_fixture::ReviewFixture);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "acme-sync-{}-{}",
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
        fs::create_dir(path.join("factory")).unwrap();
        let mut config: toml::Value = toml::from_str(FACTORY).unwrap();
        config["toolchain"]["acme"]["target_version"] = env!("CARGO_PKG_VERSION").into();
        fs::write(
            path.join("factory/factory.toml"),
            toml::to_string(&config).unwrap(),
        )
        .unwrap();
        Self(path, compiled_fixture::ReviewFixture::new())
    }

    fn run(&self, args: &[&str]) -> Output {
        let config_path = args
            .windows(2)
            .find(|pair| pair[0] == "--config")
            .map_or_else(
                || Path::new("factory/factory.toml"),
                |pair| Path::new(pair[1]),
            );
        let review = self.1.refresh(&self.0, config_path);
        let mut command = Command::new(env!("CARGO_BIN_EXE_acme"));
        command
            .current_dir(&self.0)
            .args(args)
            .env("PATH", self.0.join("no-programs"));
        if let Some(review) = review {
            command.arg("--compiled-inputs").arg(review);
        }
        command.output().unwrap()
    }

    fn sync(&self) {
        succeeds(&self.run(&["sync"]));
    }

    fn change(&self, change: impl FnOnce(&mut toml::Value)) {
        let mut config: toml::Value =
            toml::from_str(&fs::read_to_string(self.0.join("factory/factory.toml")).unwrap())
                .unwrap();
        change(&mut config);
        fs::write(
            self.0.join("factory/factory.toml"),
            toml::to_string(&config).unwrap(),
        )
        .unwrap();
    }

    fn record(&self) -> Value {
        serde_json::from_slice(&fs::read(self.0.join(RECORD)).unwrap()).unwrap()
    }

    fn write_record(&self, record: &Value) {
        fs::write(self.0.join(RECORD), serde_json::to_vec(record).unwrap()).unwrap();
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

fn fails(output: &Output, message: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn disable_intake(config: &mut toml::Value) {
    for module in ["intake", "spec", "build_cloud"] {
        config["modules"][module]["enabled"] = false.into();
    }
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if fs::symlink_metadata(&path).unwrap().is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn check_lists_creates_without_creating_files_or_directories() {
    let workspace = Workspace::new();
    let before = tree(&workspace.0);
    let output = workspace.run(&["sync", "--check"]);
    fails(&output, "drift");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("create {CI}")));
    assert!(stdout.contains(RECORD));
    assert_eq!(tree(&workspace.0), before);
    assert!(!workspace.0.join(".github").exists());
}

#[test]
fn sync_creates_sorted_hashed_inventory_and_second_sync_is_a_noop() {
    let workspace = Workspace::new();
    workspace.sync();
    let record = workspace.record();
    assert_eq!(record["format_version"], 1);
    assert_eq!(record["acme_version"], env!("CARGO_PKG_VERSION"));
    let all_entries = record["entries"].as_array().unwrap();
    assert_eq!(all_entries.len(), 13);
    let entries: Vec<_> = all_entries
        .iter()
        .filter(|entry| !matches!(entry["module"].as_str(), Some("intake" | "build_cloud")))
        .collect();
    assert_eq!(entries.len(), 8);
    let paths: Vec<_> = entries
        .iter()
        .map(|entry| entry["path"].as_str().unwrap())
        .collect();
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    for entry in entries {
        let bytes = fs::read(workspace.0.join(entry["path"].as_str().unwrap())).unwrap();
        assert_eq!(entry["sha256"], format!("{:x}", Sha256::digest(bytes)));
    }
    assert_eq!(
        fs::read(workspace.0.join(CI)).unwrap(),
        include_bytes!("../.github/workflows/ci.yml")
    );
    let before = tree(&workspace.0);
    succeeds(&workspace.run(&["sync", "--check"]));
    workspace.sync();
    assert_eq!(tree(&workspace.0), before);
}

#[test]
fn rejected_version_pins_block_sync_and_check_without_writes() {
    for owned in [false, true] {
        let workspace = Workspace::new();
        if owned {
            workspace.sync();
        }
        for pin in [
            "0.0.1", "0.1.0", "HEAD", "latest", "^0.1.0", "v0.1.0", "0.1.0 ",
        ] {
            workspace.change(|config| {
                config["toolchain"]["acme"]["bootstrap"] = false.into();
                config["toolchain"]["acme"]["target_version"] = pin.into();
                config["modules"]["ci"]["step_names"][0] = "Changed step".into();
            });
            let before = tree(&workspace.0);
            for args in [&["sync"][..], &["sync", "--check"][..]] {
                fails(&workspace.run(args), "toolchain.acme.target_version");
                assert_eq!(tree(&workspace.0), before);
                assert_eq!(workspace.0.join(".github").exists(), owned);
            }
        }
    }
}

#[test]
fn bootstrap_sync_keeps_bypassing_version_enforcement() {
    let workspace = Workspace::new();
    workspace.change(|config| {
        config["toolchain"]["acme"]["bootstrap"] = true.into();
        config["toolchain"]["acme"]["target_version"] = "HEAD".into();
    });
    workspace.sync();
    let before = tree(&workspace.0);
    succeeds(&workspace.run(&["sync", "--check"]));
    workspace.sync();
    assert_eq!(tree(&workspace.0), before);
}

#[test]
fn controlled_version_upgrade_syncs_then_repeats_without_changes() {
    for output_changed in [false, true] {
        let workspace = Workspace::new();
        workspace.sync();
        let mut old_bytes = fs::read_to_string(workspace.0.join(RECORD))
            .unwrap()
            .replace(
                &format!("\"acme_version\": \"{}\"", env!("CARGO_PKG_VERSION")),
                "\"acme_version\": \"0.0.1\"",
            );
        if output_changed {
            let old_ci = fs::read_to_string(workspace.0.join(CI))
                .unwrap()
                .replace("name: Check formatting", "name: Legacy formatting");
            fs::write(workspace.0.join(CI), &old_ci).unwrap();
            let old_record = workspace.record();
            let entry = old_record["entries"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["path"] == CI)
                .unwrap();
            old_bytes = old_bytes.replace(
                entry["sha256"].as_str().unwrap(),
                &format!("{:x}", Sha256::digest(old_ci.as_bytes())),
            );
        }
        fs::write(workspace.0.join(RECORD), &old_bytes).unwrap();
        workspace.change(|config| {
            config["toolchain"]["acme"]["bootstrap"] = false.into();
            config["toolchain"]["acme"]["target_version"] = "0.0.1".into();
            if output_changed {
                config["modules"]["ci"]["step_names"][0] = "Legacy formatting".into();
            }
        });
        let old_tree = tree(&workspace.0);
        fails(&workspace.run(&["sync"]), "toolchain.acme.target_version");
        assert_eq!(tree(&workspace.0), old_tree);

        workspace.change(|config| {
            config["toolchain"]["acme"]["target_version"] = env!("CARGO_PKG_VERSION").into();
            config["modules"]["ci"]["step_names"][0] = "Check formatting".into();
        });
        let before = tree(&workspace.0);
        let check = workspace.run(&["sync", "--check"]);
        fails(&check, "drift");
        let mut expected = String::new();
        if output_changed {
            expected.push_str(&format!("update {CI}\n"));
        }
        expected.push_str(&format!("update {RECORD}\n"));
        assert_eq!(String::from_utf8(check.stdout).unwrap(), expected);
        assert_eq!(tree(&workspace.0), before);

        workspace.sync();
        assert_eq!(
            fs::read(workspace.0.join(CI)).unwrap(),
            include_bytes!("../.github/workflows/ci.yml")
        );
        assert_eq!(
            workspace.record()["acme_version"],
            env!("CARGO_PKG_VERSION")
        );
        if !output_changed {
            let expected_record = old_bytes.replace(
                "\"acme_version\": \"0.0.1\"",
                &format!("\"acme_version\": \"{}\"", env!("CARGO_PKG_VERSION")),
            );
            let mut expected_tree = before;
            expected_tree.insert(RECORD.into(), expected_record.into_bytes());
            assert_eq!(tree(&workspace.0), expected_tree);
        }
        let upgraded = tree(&workspace.0);
        let second = workspace.run(&["sync"]);
        succeeds(&second);
        assert_eq!(second.stdout, b"Sync is clean.\n");
        succeeds(&workspace.run(&["sync", "--check"]));
        assert_eq!(tree(&workspace.0), upgraded);
    }
}

#[test]
fn matching_hand_built_files_are_adopted_but_check_does_not_adopt() {
    let workspace = Workspace::new();
    let config = acme::FactoryConfig::load(workspace.0.join("factory/factory.toml")).unwrap();
    let rendered = workspace.0.join("rendered");
    let review = workspace
        .1
        .refresh(&workspace.0, Path::new("factory/factory.toml"))
        .unwrap();
    acme::render::render_to_directory_with_compiled_inputs(
        &config,
        &workspace.0,
        &rendered,
        Some(&review),
    )
    .unwrap();
    fs::rename(rendered.join(".github"), workspace.0.join(".github")).unwrap();
    fs::rename(rendered.join("scripts"), workspace.0.join("scripts")).unwrap();
    fs::remove_dir(rendered).unwrap();
    let before = tree(&workspace.0);
    let output = workspace.run(&["sync", "--check"]);
    fails(&output, "drift");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("create .github"));
    assert_eq!(tree(&workspace.0), before);
    workspace.sync();
    assert!(workspace.0.join(RECORD).is_file());
    for (path, bytes) in before {
        assert_eq!(fs::read(workspace.0.join(path)).unwrap(), bytes);
    }
}

#[test]
fn owned_updates_and_missing_outputs_are_checked_then_applied() {
    let workspace = Workspace::new();
    workspace.sync();
    fs::remove_file(workspace.0.join(LABELS)).unwrap();
    workspace.change(|config| {
        config["modules"]["ci"]["step_names"][0] = "Changed step".into();
    });
    let before = tree(&workspace.0);
    let output = workspace.run(&["sync", "--check"]);
    fails(&output, "drift");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("create {LABELS}")));
    assert!(stdout.contains(&format!("update {CI}")));
    let changes: Vec<_> = stdout
        .lines()
        .filter(|line| !line.contains(RECORD))
        .collect();
    assert!(
        changes
            .windows(2)
            .all(|pair| pair[0].split_once(' ').unwrap().1 < pair[1].split_once(' ').unwrap().1)
    );
    assert_eq!(tree(&workspace.0), before);
    workspace.sync();
    assert!(
        fs::read_to_string(workspace.0.join(CI))
            .unwrap()
            .contains("Changed step")
    );
    assert!(workspace.0.join(LABELS).is_file());
    succeeds(&workspace.run(&["sync", "--check"]));
}

#[test]
fn disabling_intake_removes_only_recorded_outputs_and_keeps_directories() {
    let workspace = Workspace::new();
    workspace.sync();
    let unrelated = workspace.0.join(".github/ISSUE_TEMPLATE/custom.yml");
    fs::write(&unrelated, "keep").unwrap();
    // Keep an explicitly unowned source, as before agent workflow adoption.
    let mut record = workspace.record();
    record["entries"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["path"] != ".github/workflows/intake.md");
    workspace.write_record(&record);
    fs::write(workspace.0.join(".github/workflows/intake.md"), "keep").unwrap();
    workspace.change(disable_intake);
    let before = tree(&workspace.0);
    let output = workspace.run(&["sync", "--check"]);
    fails(&output, "drift");
    assert!(String::from_utf8_lossy(&output.stdout).contains(&format!("remove {LABELS}")));
    assert_eq!(tree(&workspace.0), before);
    workspace.sync();
    assert!(!workspace.0.join(LABELS).exists());
    assert!(!workspace.0.join(".github/ISSUE_TEMPLATE/bug.yml").exists());
    assert!(workspace.0.join(".github/ISSUE_TEMPLATE").is_dir());
    assert_eq!(fs::read_to_string(unrelated).unwrap(), "keep");
    assert_eq!(
        fs::read_to_string(workspace.0.join(".github/workflows/intake.md")).unwrap(),
        "keep"
    );
    assert_eq!(workspace.record()["entries"].as_array().unwrap().len(), 3);
    succeeds(&workspace.run(&["sync", "--check"]));
}

#[test]
fn conflict_late_in_plan_leaves_earlier_outputs_unchanged() {
    let workspace = Workspace::new();
    fs::create_dir_all(workspace.0.join(".github/workflows")).unwrap();
    fs::write(workspace.0.join(CI), "hand edit").unwrap();
    let before = tree(&workspace.0);
    fails(&workspace.run(&["sync"]), "unowned");
    assert_eq!(tree(&workspace.0), before);
    assert!(!workspace.0.join(".github/ISSUE_TEMPLATE").exists());
}

#[test]
fn modified_owned_outputs_block_updates_and_removals_before_any_writes() {
    for disabled in [false, true] {
        let workspace = Workspace::new();
        workspace.sync();
        fs::write(workspace.0.join(LABELS), "human edit").unwrap();
        workspace.change(|config| {
            if disabled {
                disable_intake(config);
            }
            config["modules"]["ci"]["step_names"][0] = "Changed step".into();
        });
        let before = tree(&workspace.0);
        fails(&workspace.run(&["sync"]), "modified owned");
        assert_eq!(tree(&workspace.0), before);
    }
}

#[test]
fn record_only_drift_including_generator_version_requires_sync() {
    let workspace = Workspace::new();
    workspace.sync();
    let canonical = fs::read(workspace.0.join(RECORD)).unwrap();
    for modified in [
        String::from_utf8(canonical.clone()).unwrap().replace(
            &format!("\"acme_version\": \"{}\"", env!("CARGO_PKG_VERSION")),
            "\"acme_version\": \"0.0.1\"",
        ),
        serde_json::to_string(&workspace.record()).unwrap(),
    ] {
        fs::write(workspace.0.join(RECORD), modified).unwrap();
        let before = tree(&workspace.0);
        let output = workspace.run(&["sync", "--check"]);
        fails(&output, "drift");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .collect::<Vec<_>>(),
            vec![format!("update {RECORD}")]
        );
        assert_eq!(tree(&workspace.0), before);
        workspace.sync();
        assert_eq!(fs::read(workspace.0.join(RECORD)).unwrap(), canonical);
    }
}

#[test]
fn malformed_forged_and_unsupported_records_are_rejected_without_writes() {
    let workspace = Workspace::new();
    workspace.sync();
    let original = workspace.record();
    let mut records = vec![
        json!({}),
        json!({"format_version": 2, "acme_version": "0.1.0", "entries": []}),
        json!({"format_version": 1, "acme_version": "", "entries": []}),
        json!({"format_version": 1, "acme_version": null, "entries": []}),
    ];
    for (field, value) in [
        ("module", json!("unknown")),
        ("module", json!("ci")),
        ("path", json!("AGENTS.md")),
        ("path", json!("../outside")),
        ("path", json!("/tmp/outside")),
        ("path", json!(".github/../outside")),
        ("path", json!(".github\\labels.yml")),
        ("path", json!(".github/workflows/intake.md")),
        ("sha256", json!("bad")),
        ("sha256", json!("A".repeat(64))),
        ("sha256", json!("0".repeat(64))),
        ("extra", json!(true)),
    ] {
        let mut record = original.clone();
        record["entries"][0][field] = value;
        records.push(record);
    }
    let mut duplicate = original.clone();
    duplicate["entries"]
        .as_array_mut()
        .unwrap()
        .push(original["entries"][0].clone());
    records.push(duplicate);
    let mut unknown_field = original;
    unknown_field["extra"] = true.into();
    records.push(unknown_field);
    for record in records {
        workspace.write_record(&record);
        let before = tree(&workspace.0);
        assert!(!workspace.run(&["sync"]).status.success(), "{record}");
        assert_eq!(tree(&workspace.0), before);
    }
    for malformed in [
        "{",
        r#"{"format_version":1,"format_version":1,"acme_version":"0.1.0","entries":[]}"#,
    ] {
        fs::write(workspace.0.join(RECORD), malformed).unwrap();
        let before = tree(&workspace.0);
        assert!(!workspace.run(&["sync"]).status.success());
        assert_eq!(tree(&workspace.0), before);
    }
}

#[test]
fn forged_disabled_owner_cannot_delete_arbitrary_files() {
    let workspace = Workspace::new();
    workspace.sync();
    fs::write(workspace.0.join("AGENTS.md"), "keep").unwrap();
    workspace.change(disable_intake);
    let mut record = workspace.record();
    record["entries"][0] = json!({
        "module": "issue-forms", "path": "AGENTS.md",
        "sha256": format!("{:x}", Sha256::digest(b"keep"))
    });
    workspace.write_record(&record);
    let before = tree(&workspace.0);
    fails(&workspace.run(&["sync"]), "inventory");
    assert_eq!(tree(&workspace.0), before);
}

#[test]
fn explicit_config_is_used_and_invalid_arguments_do_not_write() {
    let workspace = Workspace::new();
    fs::rename(
        workspace.0.join("factory/factory.toml"),
        workspace.0.join("approved.toml"),
    )
    .unwrap();
    for args in [
        vec!["sync", "--config"],
        vec!["sync", "--config", "--check"],
        vec!["sync", "--config", ""],
        vec!["sync", "--fix"],
        vec!["sync", "--check", "--check"],
        vec![
            "sync",
            "--config",
            "approved.toml",
            "--config",
            "approved.toml",
        ],
    ] {
        let before = tree(&workspace.0);
        assert!(!workspace.run(&args).status.success());
        assert_eq!(tree(&workspace.0), before);
    }
    let output = workspace.run(&["sync", "--check", "--config", "approved.toml"]);
    fails(&output, "drift");
    succeeds(&workspace.run(&["sync", "--config", "approved.toml"]));
    succeeds(&workspace.run(&["sync", "--config", "approved.toml", "--check"]));
}

#[test]
fn directories_at_output_or_record_paths_block_all_changes() {
    for path in [CI, RECORD] {
        let workspace = Workspace::new();
        fs::create_dir_all(workspace.0.join(path)).unwrap();
        assert!(!workspace.run(&["sync"]).status.success());
        assert!(!workspace.0.join(".github/ISSUE_TEMPLATE").exists());
        assert!(workspace.0.join(path).is_dir());
    }
}

#[cfg(unix)]
#[test]
fn all_symlink_leaves_and_ancestors_including_dangling_links_are_rejected() {
    use std::os::unix::fs::symlink;
    for relative in [RECORD, CI, ".github", ".github/actions", "factory"] {
        for dangling in [false, true] {
            let workspace = Workspace::new();
            let outside = Workspace::new();
            let target = if dangling {
                outside.0.join("absent")
            } else {
                outside.0.join("target")
            };
            if !dangling {
                if matches!(relative, RECORD | CI) {
                    fs::write(&target, "keep").unwrap();
                } else {
                    fs::create_dir(&target).unwrap();
                }
            }
            let path = workspace.0.join(relative);
            if relative == "factory" {
                fs::rename(&path, workspace.0.join("config-source")).unwrap();
            }
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            symlink(&target, &path).unwrap();
            let before = tree(&outside.0);
            let args = if relative == "factory" {
                vec!["sync", "--config", "config-source/factory.toml"]
            } else {
                vec!["sync"]
            };
            fails(&workspace.run(&args), "symlink");
            assert_eq!(tree(&outside.0), before);
            assert!(fs::symlink_metadata(&path).unwrap().is_symlink());
            assert!(!workspace.0.join(".github/ISSUE_TEMPLATE").exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn special_files_are_rejected_without_opening_them() {
    use std::os::unix::net::UnixListener;
    for relative in [CI, RECORD] {
        let workspace = Workspace::new();
        let path = workspace.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let _listener = UnixListener::bind(&path).unwrap();
        fails(&workspace.run(&["sync"]), "regular file");
        assert!(!workspace.0.join(".github/ISSUE_TEMPLATE").exists());
    }
}

#[test]
fn staged_update_does_not_change_other_hard_links() {
    let workspace = Workspace::new();
    workspace.sync();
    let other = workspace.0.join("other");
    fs::hard_link(workspace.0.join(CI), &other).unwrap();
    let original = fs::read(&other).unwrap();
    workspace.change(|config| config["modules"]["ci"]["step_names"][0] = "Changed step".into());
    workspace.sync();
    assert_eq!(fs::read(other).unwrap(), original);
    assert_ne!(fs::read(workspace.0.join(CI)).unwrap(), original);
}

#[test]
fn missing_obsolete_outputs_are_forgotten_without_deleting_unowned_files() {
    let workspace = Workspace::new();
    workspace.sync();
    fs::remove_file(workspace.0.join(LABELS)).unwrap();
    let mut record = workspace.record();
    record["entries"]
        .as_array_mut()
        .unwrap()
        .retain(|entry| entry["path"] != ".github/ISSUE_TEMPLATE/bug.yml");
    workspace.write_record(&record);
    workspace.change(disable_intake);
    workspace.sync();
    assert!(workspace.0.join(".github/ISSUE_TEMPLATE/bug.yml").is_file());
    assert!(!workspace.0.join(LABELS).exists());
    succeeds(&workspace.run(&["sync", "--check"]));
}

#[cfg(unix)]
#[test]
fn read_only_output_directory_reports_partial_application_and_keeps_record() {
    use std::os::unix::fs::PermissionsExt;
    let workspace = Workspace::new();
    workspace.sync();
    let record = fs::read(workspace.0.join(RECORD)).unwrap();
    let ci = fs::read(workspace.0.join(CI)).unwrap();
    fs::remove_file(workspace.0.join(LABELS)).unwrap();
    workspace.change(|config| config["modules"]["ci"]["step_names"][0] = "Changed step".into());
    let directory = workspace.0.join(".github/workflows");
    let permissions = fs::metadata(&directory).unwrap().permissions();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
    let output = workspace.run(&["sync"]);
    fs::set_permissions(directory, permissions).unwrap();
    fails(&output, "sync I/O failure");
    fails(&output, "after 1 output changes");
    fails(&output, "ownership record was not advanced");
    assert!(workspace.0.join(LABELS).is_file());
    assert_eq!(fs::read(workspace.0.join(CI)).unwrap(), ci);
    assert_eq!(fs::read(workspace.0.join(RECORD)).unwrap(), record);
    assert!(
        !tree(&workspace.0)
            .keys()
            .any(|path| path.to_string_lossy().contains(".acme-sync-"))
    );
}

#[cfg(unix)]
#[test]
fn record_write_failure_leaves_old_bookkeeping_after_successful_outputs() {
    use std::os::unix::fs::PermissionsExt;
    let workspace = Workspace::new();
    workspace.sync();
    let record = fs::read(workspace.0.join(RECORD)).unwrap();
    workspace.change(|config| config["modules"]["ci"]["step_names"][0] = "Changed step".into());
    let directory = workspace.0.join("factory");
    let permissions = fs::metadata(&directory).unwrap().permissions();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
    let output = workspace.run(&["sync"]);
    fs::set_permissions(directory, permissions).unwrap();
    fails(&output, "after 1 output changes");
    fails(&output, "ownership record was not advanced");
    assert!(
        fs::read_to_string(workspace.0.join(CI))
            .unwrap()
            .contains("Changed step")
    );
    assert_eq!(fs::read(workspace.0.join(RECORD)).unwrap(), record);
}

#[cfg(unix)]
#[test]
fn staged_write_failure_cleans_temporary_file_without_replacing_output() {
    let workspace = Workspace::new();
    workspace.sync();
    let before = tree(&workspace.0);
    workspace.change(|config| {
        config["modules"]["ci"]["commands"][0] = "x".repeat(8192).into();
    });
    let review = workspace
        .1
        .refresh(&workspace.0, Path::new("factory/factory.toml"))
        .unwrap();
    let output = Command::new("/bin/sh")
        .current_dir(&workspace.0)
        .args([
            "-c",
            "ulimit -f 2; trap '' XFSZ; exec \"$1\" sync --compiled-inputs \"$2\"",
            "sh",
        ])
        .arg(env!("CARGO_BIN_EXE_acme"))
        .arg(review)
        .output()
        .unwrap();
    fails(&output, "sync I/O failure");
    fails(&output, "after 0 output changes");
    assert_eq!(
        fs::read(workspace.0.join(CI)).unwrap(),
        before[&PathBuf::from(CI)]
    );
    assert_eq!(
        fs::read(workspace.0.join(RECORD)).unwrap(),
        before[&PathBuf::from(RECORD)]
    );
    assert!(
        !tree(&workspace.0)
            .keys()
            .any(|path| path.to_string_lossy().contains(".acme-sync-"))
    );
}

#[test]
fn staged_filename_collisions_never_replace_existing_files() {
    let workspace = Workspace::new();
    let parent = workspace.0.join(".github/ISSUE_TEMPLATE");
    fs::create_dir_all(&parent).unwrap();
    let collisions: Vec<_> = (0..128)
        .map(|number| parent.join(format!(".acme-sync-{}-{number}.tmp", std::process::id())))
        .collect();
    for path in &collisions {
        fs::write(path, "keep").unwrap();
    }
    let config = acme::FactoryConfig::load(workspace.0.join("factory/factory.toml")).unwrap();
    let review = workspace
        .1
        .refresh(&workspace.0, Path::new("factory/factory.toml"))
        .unwrap();
    let error = acme::render::sync_repository_with_compiled_inputs(
        &config,
        &workspace.0,
        false,
        Some(&review),
    )
    .unwrap_err();
    assert!(error.to_string().contains("cannot reserve a staged file"));
    assert!(!workspace.0.join(RECORD).exists());
    acme::render::sync_repository_with_compiled_inputs(&config, &workspace.0, false, Some(&review))
        .unwrap();
    for path in collisions {
        assert_eq!(fs::read_to_string(path).unwrap(), "keep");
    }
}
