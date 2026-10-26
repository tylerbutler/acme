use acme::{ConfigError, FactoryConfig, config::factory_schema};
use serde_json::Value;

const FACTORY: &str = include_str!("../factory/factory.toml");

fn assert_schema_and_rust_agree(schema: &Value, input: &str) {
    let toml_value: toml::Value = toml::from_str(input).expect("test case should be valid TOML");
    let json_value = serde_json::to_value(toml_value).expect("TOML value should convert to JSON");
    let validator = jsonschema::validator_for(schema).expect("generated schema should be valid");

    assert_eq!(
        FactoryConfig::from_toml(input).is_ok(),
        validator.is_valid(&json_value),
        "Rust and JSON Schema validation should agree"
    );
}

#[test]
fn generated_schema_declares_a_dialect_and_validates_raw_factory_toml() {
    let schema = serde_json::to_value(factory_schema()).expect("schema should serialize");
    assert!(schema["$schema"].is_string());

    let toml_value: toml::Value = toml::from_str(FACTORY).expect("fixture should be valid TOML");
    let json_value = serde_json::to_value(toml_value).expect("fixture should convert to JSON");
    let validator = jsonschema::validator_for(&schema).expect("generated schema should be valid");

    assert!(validator.is_valid(&json_value));
}

#[test]
fn schema_and_rust_reject_the_same_parsed_toml_contract_errors() {
    let schema = serde_json::to_value(factory_schema()).expect("schema should serialize");

    let unsupported_version = FACTORY.replace("schema_version = 0", "schema_version = 1");
    assert_schema_and_rust_agree(&schema, &unsupported_version);
    assert!(FactoryConfig::from_toml(&unsupported_version).is_err());

    let unknown_nested_field = FACTORY.replace(
        "[modules.review]\nenabled = false",
        "[modules.review]\nenabled = false\nunknown = true",
    );
    assert_schema_and_rust_agree(&schema, &unknown_nested_field);
    assert!(FactoryConfig::from_toml(&unknown_nested_field).is_err());

    let wrong_type = FACTORY.replace("solo = true", "solo = \"true\"");
    assert_schema_and_rust_agree(&schema, &wrong_type);
    assert!(FactoryConfig::from_toml(&wrong_type).is_err());

    let missing_required_field = FACTORY.replace("repository = \"tylerbutler/acme\"\n", "");
    assert_schema_and_rust_agree(&schema, &missing_required_field);
    assert!(FactoryConfig::from_toml(&missing_required_field).is_err());
}

#[test]
fn malformed_toml_remains_a_toml_parser_error() {
    let input = "schema_version = [";

    assert!(matches!(
        FactoryConfig::from_toml(input),
        Err(ConfigError::Parse(_))
    ));
    assert!(toml::from_str::<toml::Value>(input).is_err());
}
