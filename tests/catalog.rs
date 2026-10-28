use acme::{ConfigBinding, FactoryConfig, builtin_modules};

const FACTORY: &str = include_str!("../factory/factory.toml");

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
