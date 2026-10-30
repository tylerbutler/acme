use std::process::Command;

#[test]
fn doctor_compiled_inputs_rejects_missing_and_duplicate_values() {
    for (arguments, message) in [
        (
            vec!["--compiled-inputs"],
            "--compiled-inputs requires a path.",
        ),
        (
            vec!["--compiled-inputs", "--fix"],
            "--compiled-inputs requires a path.",
        ),
        (
            vec!["--compiled-inputs", "one", "--compiled-inputs", "two"],
            "--compiled-inputs must be specified once.",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_acme"))
            .arg("doctor")
            .args(arguments)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(message));
    }
}

#[test]
fn init_requires_an_init_compatible_approved_config() {
    let output = Command::new(env!("CARGO_BIN_EXE_acme"))
        .arg("init")
        .output()
        .expect("acme should start");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("agent"));
}

#[test]
fn doctor_validates_config_before_contacting_github() {
    let path =
        std::env::temp_dir().join(format!("acme-invalid-config-{}.toml", std::process::id()));
    std::fs::write(&path, "schema_version = [").expect("write invalid config");
    let output = Command::new(env!("CARGO_BIN_EXE_acme"))
        .args(["doctor", "--config"])
        .arg(&path)
        .output()
        .expect("acme should start");
    std::fs::remove_file(path).expect("remove invalid config");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed to parse factory TOML"));
    assert!(!stderr.contains("authentication"));
}

#[test]
fn doctor_fix_validates_config_before_contacting_github() {
    let path = std::env::temp_dir().join(format!(
        "acme-invalid-fix-config-{}.toml",
        std::process::id()
    ));
    std::fs::write(&path, "schema_version = [").expect("write invalid config");
    let output = Command::new(env!("CARGO_BIN_EXE_acme"))
        .args(["doctor", "--fix", "--config"])
        .arg(&path)
        .output()
        .expect("acme should start");
    std::fs::remove_file(path).expect("remove invalid config");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to parse factory TOML"));
}
