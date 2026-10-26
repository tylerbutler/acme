use acme::{ConfigError, FactoryConfig};

const FACTORY: &str = include_str!("../factory/factory.toml");

#[test]
fn loads_factory_fixture_and_disabled_module_stubs() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/factory/factory.toml");
    let config = FactoryConfig::load(path).expect("fixture should load");

    assert_eq!(config.schema_version, 0);
    assert_eq!(config.repository, "tylerbutler/acme");
    assert!(!config.coordinator.automatic_dispatch);
    assert_eq!(config.coordinator.max_active_tasks, 0);
    assert_eq!(config.engines.intake.token_secret, "GH_PAT");
    assert!(!config.modules.review.enabled);
    assert!(!config.modules.holdout.enabled);
    assert!(!config.modules.build_local.enabled);
    assert!(!config.modules.release.enabled);
    assert!(!config.modules.ops.enabled);
}

#[test]
fn loads_explicit_coordinator_limits_without_changing_the_fixture_default() {
    let configured = FACTORY.replace(
        "[bootstrap]",
        "[coordinator]\nautomatic_dispatch = true\nmax_active_tasks = 2\nmax_assignments_per_reconciliation = 1\nmax_attempts = 3\n\n[bootstrap]",
    );
    let config = FactoryConfig::from_toml(&configured).expect("coordinator config should load");

    assert!(config.coordinator.automatic_dispatch);
    assert_eq!(config.coordinator.max_active_tasks, 2);
    assert_eq!(config.coordinator.max_assignments_per_reconciliation, 1);
    assert_eq!(config.coordinator.max_attempts, 3);
}

#[test]
fn reports_malformed_toml() {
    let error =
        FactoryConfig::from_toml("schema_version = [").expect_err("malformed TOML should fail");

    assert!(error.to_string().contains("failed to parse factory TOML"));
}

#[test]
fn rejects_unsupported_schema_versions() {
    let input = FACTORY.replace("schema_version = 0", "schema_version = 1");
    let error = FactoryConfig::from_toml(&input).expect_err("unknown version should fail");

    assert!(matches!(error, ConfigError::UnsupportedSchemaVersion(1)));
    assert!(
        error
            .to_string()
            .contains("unsupported factory schema version 1")
    );
}

#[test]
fn rejects_unknown_fields_at_the_root_and_nested_levels() {
    let root_unknown = format!("{FACTORY}\nunknown = true\n");
    assert!(FactoryConfig::from_toml(&root_unknown).is_err());

    let nested_unknown = FACTORY.replace(
        "[modules.review]\nenabled = false",
        "[modules.review]\nenabled = false\nunknown = true",
    );
    assert!(FactoryConfig::from_toml(&nested_unknown).is_err());
}

#[test]
fn rejects_wrong_types_and_missing_required_fields() {
    let wrong_type = FACTORY.replace("solo = true", "solo = \"true\"");
    assert!(FactoryConfig::from_toml(&wrong_type).is_err());

    let missing_field = FACTORY.replace("repository = \"tylerbutler/acme\"\n", "");
    assert!(FactoryConfig::from_toml(&missing_field).is_err());
}

#[test]
fn rejects_enabled_stub_modules() {
    let input = FACTORY.replace(
        "[modules.review]\nenabled = false",
        "[modules.review]\nenabled = true",
    );
    let error = FactoryConfig::from_toml(&input).expect_err("enabled stub should fail");

    assert!(matches!(error, ConfigError::EnabledStubModule("review")));
}
