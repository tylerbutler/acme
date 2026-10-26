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
