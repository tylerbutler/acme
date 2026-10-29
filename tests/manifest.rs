use acme::{ManifestError, ModuleManifest};

const CI_MANIFEST: &str = include_str!("../modules/ci/module.toml");

#[test]
fn loads_ci_manifest_fixture() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/modules/ci/module.toml");
    let manifest = ModuleManifest::load(path).expect("CI manifest should load");

    assert_eq!(manifest.name, "ci");
    assert!(manifest.needs.is_empty());
    assert_eq!(manifest.provides, ["ci"]);
    assert_eq!(manifest.gate, "required-status-check");
}

#[test]
fn allows_empty_capability_lists() {
    let manifest = ModuleManifest::from_toml(
        r#"
name = "example"
needs = []
provides = []
gate = "example-gate"
"#,
    )
    .expect("empty capability lists should be valid");

    assert!(manifest.needs.is_empty());
    assert!(manifest.provides.is_empty());
}

#[test]
fn rejects_malformed_unknown_and_wrongly_typed_manifests() {
    for input in [
        "name = [",
        r#"
name = "example"
needs = []
provides = []
gate = "example-gate"
unknown = true
"#,
        r#"
name = "example"
needs = "none"
provides = []
gate = "example-gate"
"#,
    ] {
        assert!(
            matches!(
                ModuleManifest::from_toml(input),
                Err(ManifestError::Parse(_))
            ),
            "manifest should fail parsing: {input}"
        );
    }
}

#[test]
fn rejects_missing_fields() {
    let error = ModuleManifest::from_toml(
        r#"
name = "example"
needs = []
provides = []
"#,
    )
    .expect_err("missing gate should fail");

    assert!(matches!(error, ManifestError::Parse(_)));
}

#[test]
fn rejects_empty_names_gates_and_capability_entries() {
    for (input, field) in [
        (
            r#"
name = ""
needs = []
provides = []
gate = "example-gate"
"#,
            "name",
        ),
        (
            r#"
name = "example"
needs = []
provides = []
gate = " "
"#,
            "gate",
        ),
        (
            r#"
name = "example"
needs = [""]
provides = []
gate = "example-gate"
"#,
            "needs",
        ),
        (
            r#"
name = "example"
needs = []
provides = [" "]
gate = "example-gate"
"#,
            "provides",
        ),
    ] {
        let error = ModuleManifest::from_toml(input).expect_err("empty values should fail");

        assert!(matches!(error, ManifestError::EmptyValue(actual) if actual == field));
    }
}

#[test]
fn ci_fixture_is_metadata_only() {
    let manifest = ModuleManifest::from_toml(CI_MANIFEST).expect("CI manifest should parse");

    assert_eq!(manifest.name, "ci");
    assert_eq!(manifest.provides, ["ci"]);
}
