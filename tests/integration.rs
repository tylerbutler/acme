use std::process::Command;

#[test]
fn bootstrap_binary_does_not_report_success() {
    let output = Command::new(env!("CARGO_BIN_EXE_acme"))
        .arg("render")
        .output()
        .expect("acme should start");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not implemented"));
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
