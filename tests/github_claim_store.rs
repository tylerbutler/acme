use acme::claim_store::strict_json;

#[test]
fn strict_json_rejects_duplicate_keys_at_every_depth() {
    for input in [
        r#"{"version":1,"version":1}"#,
        r#"{"operation":{"kind":"acquire","kind":"release"}}"#,
        r#"{"items":[{"id":1,"id":2}]}"#,
    ] {
        assert!(strict_json::<serde_json::Value>(input.as_bytes()).is_err());
    }
    assert!(strict_json::<serde_json::Value>(br#"{"items":[{"id":1}]}"#).is_ok());
}
