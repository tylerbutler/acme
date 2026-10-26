use acme::{
    ContentSnapshot, DispatchContractError, DispatchDeclaration, DispatchReason, Prerequisite,
    SpecBinding, TaskKind, content_sha256,
};

fn declaration_toml() -> String {
    let body = "Implement a bounded slice.";
    let spec = "Approved spec text.";
    format!(
        r#"
version = 1
repository = "tylerbutler/acme"
issue = 43
kind = "implementation"
prerequisites = []

[spec]
path = "docs/specs/0010-spec-dispatch-coordinator.md"
sha256 = "{}"

[issue_body]
content = "{}"
sha256 = "{}"
"#,
        content_sha256(spec),
        body,
        content_sha256(body)
    )
}

#[test]
fn parses_strict_versioned_declaration_and_snapshot() {
    let declaration =
        DispatchDeclaration::from_toml(&declaration_toml()).expect("declaration should parse");

    assert_eq!(declaration.version, 1);
    assert_eq!(declaration.repository, "tylerbutler/acme");
    assert_eq!(declaration.issue, 43);
    assert_eq!(declaration.kind, TaskKind::Implementation);
    assert_eq!(
        declaration.spec,
        SpecBinding {
            path: "docs/specs/0010-spec-dispatch-coordinator.md".to_owned(),
            sha256: content_sha256("Approved spec text."),
        }
    );
    assert_eq!(
        declaration.issue_body,
        ContentSnapshot {
            content: "Implement a bounded slice.".to_owned(),
            sha256: content_sha256("Implement a bounded slice."),
        }
    );
}

#[test]
fn rejects_unknown_fields_bad_hashes_and_unsupported_versions() {
    let unknown = format!("{}\nextra = true\n", declaration_toml());
    assert!(matches!(
        DispatchDeclaration::from_toml(&unknown),
        Err(DispatchContractError::Parse(_))
    ));

    let bad_hash = declaration_toml().replace(
        &content_sha256("Implement a bounded slice."),
        &"0".repeat(64),
    );
    assert!(matches!(
        DispatchDeclaration::from_toml(&bad_hash),
        Err(DispatchContractError::Invalid(
            DispatchReason::InvalidBodyHash
        ))
    ));

    let unsupported = declaration_toml().replace("version = 1", "version = 2");
    assert!(matches!(
        DispatchDeclaration::from_toml(&unsupported),
        Err(DispatchContractError::Invalid(
            DispatchReason::UnsupportedVersion
        ))
    ));
}

#[test]
fn rejects_unsafe_spec_paths_and_unsorted_prerequisites() {
    let traversal = declaration_toml().replace(
        "docs/specs/0010-spec-dispatch-coordinator.md",
        "docs/specs/../private.md",
    );
    assert!(matches!(
        DispatchDeclaration::from_toml(&traversal),
        Err(DispatchContractError::Invalid(
            DispatchReason::InvalidSpecPath
        ))
    ));

    let mut declaration =
        DispatchDeclaration::from_toml(&declaration_toml()).expect("declaration should parse");
    declaration.prerequisites = vec![
        Prerequisite {
            issue: 8,
            owner_acceptance_required: false,
        },
        Prerequisite {
            issue: 7,
            owner_acceptance_required: false,
        },
    ];
    assert!(matches!(
        declaration.validate(),
        Err(DispatchContractError::Invalid(
            DispatchReason::UnsortedOrDuplicatePrerequisites
        ))
    ));
}

#[test]
fn hash_canonicalization_normalizes_only_line_endings() {
    assert_eq!(
        content_sha256("line one\r\nline two\r"),
        content_sha256("line one\nline two\n")
    );
    assert_ne!(content_sha256(" text"), content_sha256("text"));
}
