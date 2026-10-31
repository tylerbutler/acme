use acme::{ConfigBinding, FactoryConfig, builtin_modules};
use sha2::{Digest, Sha256};

const FACTORY: &str = include_str!("../factory/factory.toml");

#[test]
fn role_assets_preserve_task_fields_and_separate_tracking_from_approval() {
    let modules = builtin_modules();
    let asset = |path: &str| {
        modules
            .iter()
            .flat_map(|module| module.assets)
            .find(|asset| asset.output_path == path)
            .unwrap_or_else(|| panic!("missing catalog asset {path}"))
    };
    let labels: serde_json::Value =
        serde_json::from_slice(asset(".github/labels.yml").contents).unwrap();
    for (name, color, description) in [
        (
            "tracking",
            "5319E7",
            "Coordinates work; not an agent build task",
        ),
        ("task", "C5DEF5", "One work item; not build approval"),
    ] {
        let matches: Vec<_> = labels
            .as_array()
            .unwrap()
            .iter()
            .filter(|label| label["name"] == name)
            .collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["color"], color);
        assert_eq!(matches[0]["description"], description);
    }
    for (filename, prefix, labels, fields_hash) in [
        (
            "bug.yml",
            "title: \"[bug] \"",
            "labels: [bug, needs-spec, task]",
            "d96af8ebda30fba77e5a09f4ad1ec4ef4726e67488103dfc5e5a0839723960c9",
        ),
        (
            "idea.yml",
            "title: \"[idea] \"",
            "labels: [idea, needs-spec, task]",
            "27b6a40f232c0d488cd5b5e80ea84d775979475e654d1e732e72b34d3591eea1",
        ),
        (
            "spec-request.yml",
            "title: \"[spec] \"",
            "labels: [needs-spec, task]",
            "2b55071b9e67c2533fd179962f43d3cc60a95869dce25d8f38214fb3ded065c2",
        ),
    ] {
        let path = format!(".github/ISSUE_TEMPLATE/{filename}");
        let form = std::str::from_utf8(asset(&path).contents).unwrap();
        assert!(form.lines().any(|line| line == prefix));
        assert!(form.lines().any(|line| line == labels));
        assert!(
            form.lines().any(|line| {
                line.starts_with("description: ") && line.contains("tracking form")
            })
        );
        let (_, fields) = form.split_once("body:\n").unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(fields)), fields_hash);
    }
    let tracking =
        std::str::from_utf8(asset(".github/ISSUE_TEMPLATE/tracking.yml").contents).unwrap();
    assert!(tracking.starts_with("name: Tracking issue\n"));
    assert!(tracking.contains("labels: [tracking]\n"));
    assert!(!tracking.lines().any(|line| line.starts_with("title:")));
    for (id, required) in [
        ("goal", true),
        ("completion", true),
        ("children", false),
        ("spec", false),
        ("non-goals", true),
    ] {
        let field = tracking
            .split("  - type:")
            .find(|field| {
                field
                    .lines()
                    .any(|line| line.strip_prefix("    id: ") == Some(id))
            })
            .unwrap_or_else(|| panic!("missing tracking field {id}"));
        let validation = if required {
            "      required: true"
        } else {
            "      required: false"
        };
        assert!(field.lines().any(|line| line == validation));
    }
}

#[test]
fn loads_all_builtin_manifests_and_expected_mappings() {
    let modules = builtin_modules();
    let config = FactoryConfig::from_toml(FACTORY).expect("factory fixture should load");
    let expected = [
        (
            "ci",
            "required-status-check",
            ConfigBinding::ModuleSection("modules.ci"),
        ),
        (
            "labels",
            "enabled-intake",
            ConfigBinding::EnabledDependency("modules.intake"),
        ),
        ("ruleset", "human-merge", ConfigBinding::HumanMerge),
        (
            "issue-forms",
            "enabled-intake",
            ConfigBinding::EnabledDependency("modules.intake"),
        ),
    ];

    assert_eq!(modules.len(), expected.len());
    for (module, (name, gate, binding)) in modules.iter().zip(expected) {
        let manifest = module.manifest().expect("built-in manifest should load");

        assert_eq!(manifest.name, name);
        assert_eq!(manifest.gate, gate);
        assert_eq!(module.binding, binding);

        match module.binding {
            ConfigBinding::ModuleSection(section) => {
                assert_eq!(section, "modules.ci");
                assert!(config.modules.ci.enabled);
            }
            ConfigBinding::EnabledDependency(section) => {
                assert_eq!(section, "modules.intake");
                assert!(config.modules.intake.enabled);
                assert!(config.modules.intake.needs.contains(&manifest.name));
            }
            ConfigBinding::HumanMerge => {
                assert!(config.modules.merge.enabled);
                assert_eq!(config.modules.merge.implementation, "human");
                assert!(
                    config
                        .risk
                        .human_merge
                        .iter()
                        .any(|path| path == ".github/**")
                );
            }
        }
    }
}

#[test]
fn catalog_declares_exact_fixture_inventory_and_embeds_matching_bytes() {
    let expected = [
        (
            ".github/workflows/ci.yml",
            include_bytes!("../.github/workflows/ci.yml").as_slice(),
        ),
        (
            ".github/actions/setup-rust/action.yml",
            include_bytes!("../.github/actions/setup-rust/action.yml").as_slice(),
        ),
        (
            ".github/labels.yml",
            include_bytes!("../.github/labels.yml").as_slice(),
        ),
        (
            ".github/rulesets/main.json",
            include_bytes!("../.github/rulesets/main.json").as_slice(),
        ),
        (
            ".github/ISSUE_TEMPLATE/idea.yml",
            include_bytes!("../.github/ISSUE_TEMPLATE/idea.yml").as_slice(),
        ),
        (
            ".github/ISSUE_TEMPLATE/bug.yml",
            include_bytes!("../.github/ISSUE_TEMPLATE/bug.yml").as_slice(),
        ),
        (
            ".github/ISSUE_TEMPLATE/spec-request.yml",
            include_bytes!("../.github/ISSUE_TEMPLATE/spec-request.yml").as_slice(),
        ),
        (
            ".github/ISSUE_TEMPLATE/tracking.yml",
            include_bytes!("../.github/ISSUE_TEMPLATE/tracking.yml").as_slice(),
        ),
        (
            ".github/ISSUE_TEMPLATE/config.yml",
            include_bytes!("../.github/ISSUE_TEMPLATE/config.yml").as_slice(),
        ),
    ];
    let modules = builtin_modules();
    let assets: Vec<_> = modules.iter().flat_map(|module| module.assets).collect();

    assert_eq!(assets.len(), expected.len());
    for (asset, (path, contents)) in assets.iter().zip(expected) {
        assert_eq!(asset.output_path, path);
        assert_eq!(asset.contents, contents);
    }
}
