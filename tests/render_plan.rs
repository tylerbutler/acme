use acme::render::{TemplateInputs, build_render_plan, build_render_plan_with_catalog};
use acme::{ConfigBinding, FactoryConfig, ModuleAsset, builtin_modules};

const FACTORY: &str = include_str!("../factory/factory.toml");

fn config() -> FactoryConfig {
    let mut config = FactoryConfig::from_toml(FACTORY).unwrap();
    env!("CARGO_PKG_VERSION").clone_into(&mut config.toolchain.acme.target_version);
    config
}

#[test]
fn non_bootstrap_pin_must_match_the_running_version_in_both_planners() {
    let mut config = config();
    config.toolchain.acme.bootstrap = false;
    config.toolchain.acme.target_version = env!("CARGO_PKG_VERSION").to_owned();
    let plan = build_render_plan(&config).unwrap();
    assert_eq!(plan.acme_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(
        plan,
        build_render_plan_with_catalog(&config, builtin_modules()).unwrap()
    );

    for pin in [
        "0.0.1",
        "0.1.0",
        "99.0.0",
        "",
        "HEAD",
        "latest",
        "*",
        "^0.1.0",
        "~0.1.0",
        ">=0.1.0",
        "0.1",
        "v0.1.0",
        "00.1.0",
        "0.01.0",
        "0.1.00",
        " 0.1.0",
        "0.1.0 ",
        "0.1.0\n",
        "0.1.0-rc.1",
        "0.1.0+build",
    ] {
        config.toolchain.acme.target_version = pin.to_owned();
        for result in [
            build_render_plan(&config),
            build_render_plan_with_catalog(&config, builtin_modules()),
        ] {
            let error = result.unwrap_err().to_string();
            assert!(error.contains("toolchain.acme.target_version"), "{error}");
            assert!(error.contains(env!("CARGO_PKG_VERSION")), "{error}");
        }
    }
}

#[test]
fn bootstrap_keeps_treating_the_pin_as_a_build_target() {
    let mut config = config();
    config.toolchain.acme.bootstrap = true;
    for pin in ["HEAD", "latest", "", "99.0.0"] {
        config.toolchain.acme.target_version = pin.to_owned();
        assert_eq!(
            build_render_plan(&config).unwrap().acme_version,
            env!("CARGO_PKG_VERSION")
        );
    }
}

#[test]
fn plan_is_stable_and_orders_modules_paths_and_template_inputs() {
    let config = config();
    let plan = build_render_plan(&config).unwrap();
    assert_eq!(plan, build_render_plan(&config).unwrap());
    let mut reversed = builtin_modules().to_vec();
    reversed.reverse();
    assert_eq!(
        plan,
        build_render_plan_with_catalog(&config, &reversed).unwrap()
    );
    assert_eq!(
        plan.modules
            .iter()
            .map(|module| module.name.as_str())
            .collect::<Vec<_>>(),
        ["ci", "issue-forms", "labels", "ruleset"]
    );
    assert_eq!(
        plan.outputs
            .iter()
            .map(|output| output.path)
            .collect::<Vec<_>>(),
        [
            ".github/ISSUE_TEMPLATE/bug.yml",
            ".github/ISSUE_TEMPLATE/config.yml",
            ".github/ISSUE_TEMPLATE/idea.yml",
            ".github/ISSUE_TEMPLATE/spec-request.yml",
            ".github/actions/setup-rust/action.yml",
            ".github/labels.yml",
            ".github/rulesets/main.json",
            ".github/workflows/ci.yml",
        ]
    );
    assert_eq!(
        plan.external_stages
            .iter()
            .map(|stage| stage.name.as_str())
            .collect::<Vec<_>>(),
        ["build_cloud", "intake", "merge", "spec"]
    );
    let ci = plan.outputs.last().unwrap();
    assert_eq!(
        ci.template,
        include_bytes!("../modules/ci/assets/ci.yml.tmpl")
    );
    let TemplateInputs::Ci {
        required_check,
        steps,
    } = &ci.inputs
    else {
        panic!("CI workflow needs typed inputs");
    };
    assert_eq!(required_check, "CI");
    assert_eq!(steps.len(), 6);
    assert_eq!(steps[3].name, "Factory schema freshness");
    assert_eq!(
        steps[3].command,
        "cargo run --quiet --locked --bin generate-schema -- --check"
    );
    let live_workflow = include_str!("../.github/workflows/ci.yml");
    let live_commands: Vec<_> = live_workflow
        .lines()
        .filter_map(|line| line.strip_prefix("        run: "))
        .collect();
    let live_names: Vec<_> = live_workflow
        .lines()
        .filter_map(|line| line.strip_prefix("      - name: "))
        .collect();
    assert_eq!(
        steps
            .iter()
            .map(|step| step.command.as_str())
            .collect::<Vec<_>>(),
        live_commands
    );
    assert_eq!(
        steps
            .iter()
            .map(|step| step.name.as_str())
            .collect::<Vec<_>>(),
        live_names
    );
}

#[test]
fn mapped_fields_reach_only_their_template_inputs() {
    let mut config = config();
    config.modules.ci.required_checks = vec!["Quality".to_owned()];
    config.modules.ci.commands.push("echo extra".to_owned());
    config.modules.ci.step_names.push("Extra check".to_owned());
    config.modules.merge.required_approvals = 2;
    config
        .modules
        .merge
        .require_extra_approval_for_unattributed_changes = true;
    let plan = build_render_plan(&config).unwrap();
    let TemplateInputs::Ci {
        required_check,
        steps,
    } = &plan.outputs.last().unwrap().inputs
    else {
        panic!("missing CI inputs");
    };
    assert_eq!(required_check, "Quality");
    assert_eq!(steps.last().unwrap().name, "Extra check");
    assert_eq!(steps.last().unwrap().command, "echo extra");
    let ruleset = plan
        .outputs
        .iter()
        .find(|output| output.module == "ruleset")
        .unwrap();
    assert_eq!(
        ruleset.inputs,
        TemplateInputs::Ruleset {
            required_checks: vec!["Quality".to_owned()],
            required_approvals: 2,
            require_extra_approval_for_unattributed_changes: true,
        }
    );
    assert!(plan.outputs.iter().filter(|output| output.module != "ruleset" && output.path != ".github/workflows/ci.yml")
        .all(|output| output.inputs == TemplateInputs::Static));
}

#[test]
fn disabled_stages_have_no_outputs_or_capabilities() {
    let mut config = config();
    config.modules.intake.enabled = false;
    assert!(
        build_render_plan(&config)
            .unwrap_err()
            .to_string()
            .contains("triaged-issue")
    );
    config.modules.spec.enabled = false;
    config.modules.build_cloud.enabled = false;
    let plan = build_render_plan(&config).unwrap();
    assert_eq!(plan.outputs.len(), 3);
    assert!(
        plan.outputs
            .iter()
            .all(|output| output.module == "ci" || output.module == "ruleset")
    );
    config.modules.merge.enabled = false;
    config.modules.ci.enabled = false;
    assert!(build_render_plan(&config).unwrap().outputs.is_empty());
}

#[test]
fn missing_ambiguous_and_cyclic_stage_capabilities_fail() {
    let mut missing = config();
    missing.modules.build_cloud.needs.push("missing".to_owned());
    assert!(
        build_render_plan(&missing)
            .unwrap_err()
            .to_string()
            .contains("missing")
    );

    let mut ambiguous = config();
    ambiguous.modules.spec.provides.push("ci".to_owned());
    assert!(
        build_render_plan(&ambiguous)
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );

    let mut cycle = config();
    cycle.modules.intake.needs.push("draft-pr".to_owned());
    assert!(
        build_render_plan(&cycle)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );

    let mut self_cycle = config();
    self_cycle
        .modules
        .spec
        .needs
        .push("approved-spec".to_owned());
    assert!(
        build_render_plan(&self_cycle)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
}

#[test]
fn manifest_dependencies_are_resolved_in_both_directions() {
    let mut catalog = builtin_modules().to_vec();
    catalog[0].manifest_toml =
        "name = 'ci'\nneeds = ['approved-spec']\nprovides = ['ci']\ngate = 'required-status-check'";
    let plan = build_render_plan_with_catalog(&config(), &catalog).unwrap();
    assert!(
        plan.modules
            .iter()
            .find(|module| module.name == "ci")
            .unwrap()
            .needs
            .contains(&"approved-spec".to_owned())
    );
    catalog[0].manifest_toml =
        "name = 'ci'\nneeds = ['draft-pr']\nprovides = ['ci']\ngate = 'required-status-check'";
    assert!(
        build_render_plan_with_catalog(&config(), &catalog)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
}

#[test]
fn unsupported_gates_and_bindings_fail() {
    let mut configured = config();
    configured.modules.ci.gate = "unimplemented".to_owned();
    assert!(
        build_render_plan(&configured)
            .unwrap_err()
            .to_string()
            .contains("gate")
    );
    let mut catalog = builtin_modules().to_vec();
    catalog[0].manifest_toml = "name = 'ci'\nneeds = []\nprovides = ['ci']\ngate = 'unimplemented'";
    assert!(build_render_plan_with_catalog(&config(), &catalog).is_err());
    catalog = builtin_modules().to_vec();
    catalog[0].binding = ConfigBinding::HumanMerge;
    assert!(build_render_plan_with_catalog(&config(), &catalog).is_err());
    configured.modules.ci.gate = "required-status-check".to_owned();
    configured.modules.merge.auto_merge = true;
    assert!(build_render_plan(&configured).is_err());
}

#[test]
fn invalid_ci_inputs_fail_before_a_plan_is_returned() {
    for checks in [
        vec![],
        vec!["CI".to_owned(), "Other".to_owned()],
        vec![String::new()],
    ] {
        let mut config = config();
        config.modules.ci.required_checks = checks;
        assert!(build_render_plan(&config).is_err());
    }
    let mut config = config();
    config.modules.ci.step_names.pop();
    assert!(build_render_plan(&config).is_err());
    config.modules.ci.commands.clear();
    config.modules.ci.step_names.clear();
    assert!(build_render_plan(&config).is_err());
}

#[test]
fn collisions_and_portably_unsafe_paths_fail() {
    macro_rules! rejects_paths {
        ($($path:literal),+ $(,)?) => {
            $({
                let mut catalog = builtin_modules().to_vec();
                catalog[1].assets = &[ModuleAsset { output_path: $path, contents: b"" }];
                assert!(build_render_plan_with_catalog(&config(), &catalog).is_err(), "{:?}", $path);
            })+
        };
    }
    rejects_paths!(
        "",
        "/absolute",
        "../escape",
        "a/../escape",
        "./file",
        "a//b",
        "a/",
        "C:/escape",
        r"a\b",
        "a\nb",
        ".git/config",
        ".github/workflows/ci.yml",
        ".github/workflows",
        ".github/workflows/ci.yml/child",
    );
}

#[test]
fn duplicate_missing_or_malformed_catalog_modules_fail() {
    let mut catalog = builtin_modules().to_vec();
    catalog.push(catalog[0]);
    assert!(build_render_plan_with_catalog(&config(), &catalog).is_err());
    assert!(build_render_plan_with_catalog(&config(), &[]).is_err());
    catalog[0].manifest_toml = "not valid TOML";
    assert!(build_render_plan_with_catalog(&config(), &catalog).is_err());
}

#[test]
fn required_ci_gate_cannot_be_bypassed_by_removing_declared_needs() {
    let mut config = config();
    config.modules.ci.enabled = false;
    config.modules.build_cloud.enabled = false;
    config.modules.merge.needs.clear();
    assert!(
        build_render_plan(&config)
            .unwrap_err()
            .to_string()
            .contains("ci")
    );
}

#[test]
fn unsupported_presets_and_enabled_stubs_fail() {
    let mut configured = config();
    configured.profile = "web".to_owned();
    assert!(build_render_plan(&configured).is_err());
    configured = config();
    configured.languages = vec!["python".to_owned()];
    assert!(build_render_plan(&configured).is_err());
    configured = config();
    configured.profile = "library".to_owned();
    assert!(build_render_plan(&configured).is_err());
    configured = config();
    configured.languages = vec!["rust".to_owned(), "rust".to_owned()];
    assert!(build_render_plan(&configured).is_err());
    configured = config();
    configured.languages = vec!["rust".to_owned(), "typescript".to_owned()];
    assert!(build_render_plan(&configured).is_err());
    configured = config();
    configured.modules.review.enabled = true;
    assert!(build_render_plan(&configured).is_err());
}

#[test]
fn render_plan_rejects_library_preset_even_with_full_library_contract() {
    let mut configured = config();
    configured.profile = "library".to_owned();
    configured.languages = vec!["rust".to_owned()];
    configured.modules.ci.required_checks = vec!["CI".to_owned()];
    configured.modules.ci.commands = vec![
        "cargo fmt --all -- --check".to_owned(),
        "cargo clippy --locked --all-targets --all-features -- -D warnings".to_owned(),
        "cargo test --locked --all-features".to_owned(),
        "cargo doc --locked --no-deps --all-features".to_owned(),
    ];
    configured.modules.ci.step_names = vec![
        "Fmt".to_owned(),
        "Clippy".to_owned(),
        "Test".to_owned(),
        "Doc".to_owned(),
    ];
    configured.modules.review.enabled = false;
    configured.engines.review.enabled = false;
    let error = build_render_plan(&configured).unwrap_err().to_string();
    assert_eq!(
        error,
        "unsupported render preset: require profile cli and languages [rust]"
    );
}

#[test]
fn external_gate_declarations_are_not_claimed_as_verified() {
    let mut config = config();
    config.modules.intake.gate = "external-intake-gate".to_owned();
    config.modules.build_cloud.gate = "external-build-gate".to_owned();
    let plan = build_render_plan(&config).unwrap();
    assert_eq!(plan.external_stages[0].gate, "external-build-gate");
    assert_eq!(plan.external_stages[1].gate, "external-intake-gate");
    assert!(
        !plan
            .outputs
            .iter()
            .any(|output| output.path.contains("build-cloud") || output.path.contains("intake"))
    );
}

#[test]
fn supplied_catalog_cannot_claim_paths_outside_its_m2_inventory() {
    let mut catalog = builtin_modules().to_vec();
    catalog[1].assets = &[ModuleAsset {
        output_path: "AGENTS.md",
        contents: b"",
    }];
    assert!(build_render_plan_with_catalog(&config(), &catalog).is_err());
}

#[test]
fn supplied_catalog_must_include_all_assets_of_each_enabled_module() {
    let mut catalog = builtin_modules().to_vec();
    catalog[0].assets = &[];
    assert!(build_render_plan_with_catalog(&config(), &catalog).is_err());
}

#[test]
fn dependency_order_places_a_provider_before_its_consumer() {
    let mut catalog = builtin_modules().to_vec();
    catalog[0].manifest_toml =
        "name = 'ci'\nneeds = ['labels']\nprovides = ['ci']\ngate = 'required-status-check'";
    let plan = build_render_plan_with_catalog(&config(), &catalog).unwrap();
    assert_eq!(
        plan.modules
            .iter()
            .map(|module| module.name.as_str())
            .collect::<Vec<_>>(),
        ["issue-forms", "labels", "ci", "ruleset"]
    );
}

#[test]
fn external_ci_provider_preserves_required_checks_in_ruleset_inputs() {
    let mut config = config();
    config.modules.ci.enabled = false;
    config.modules.spec.provides.push("ci".to_owned());
    let plan = build_render_plan(&config).unwrap();
    assert!(!plan.modules.iter().any(|module| module.name == "ci"));
    let ruleset = plan
        .outputs
        .iter()
        .find(|output| output.module == "ruleset")
        .unwrap();
    let TemplateInputs::Ruleset {
        required_checks, ..
    } = &ruleset.inputs
    else {
        panic!("missing ruleset inputs");
    };
    assert_eq!(required_checks, &["CI"]);

    config.modules.ci.required_checks.clear();
    assert!(build_render_plan(&config).is_err());
}
