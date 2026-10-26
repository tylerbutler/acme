use crate::{ManifestError, ModuleManifest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigBinding {
    ModuleSection(&'static str),
    EnabledDependency(&'static str),
    HumanMerge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleAsset {
    pub output_path: &'static str,
    pub contents: &'static [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BuiltinModule {
    pub manifest_toml: &'static str,
    pub binding: ConfigBinding,
    pub assets: &'static [ModuleAsset],
}

impl BuiltinModule {
    pub fn manifest(&self) -> Result<ModuleManifest, ManifestError> {
        ModuleManifest::from_toml(self.manifest_toml)
    }
}

const MODULES: [BuiltinModule; 4] = [
    BuiltinModule {
        manifest_toml: include_str!("../modules/ci/module.toml"),
        binding: ConfigBinding::ModuleSection("modules.ci"),
        assets: &[
            ModuleAsset {
                output_path: ".github/workflows/ci.yml",
                contents: include_bytes!("../modules/ci/assets/ci.yml"),
            },
            ModuleAsset {
                output_path: ".github/actions/setup-rust/action.yml",
                contents: include_bytes!("../modules/ci/assets/setup-rust-action.yml"),
            },
        ],
    },
    BuiltinModule {
        manifest_toml: include_str!("../modules/labels/module.toml"),
        binding: ConfigBinding::EnabledDependency("modules.intake"),
        assets: &[ModuleAsset {
            output_path: ".github/labels.yml",
            contents: include_bytes!("../modules/labels/assets/labels.yml"),
        }],
    },
    BuiltinModule {
        manifest_toml: include_str!("../modules/ruleset/module.toml"),
        binding: ConfigBinding::HumanMerge,
        assets: &[ModuleAsset {
            output_path: ".github/rulesets/main.json",
            contents: include_bytes!("../modules/ruleset/assets/main.json"),
        }],
    },
    BuiltinModule {
        manifest_toml: include_str!("../modules/issue-forms/module.toml"),
        binding: ConfigBinding::EnabledDependency("modules.intake"),
        assets: &[
            ModuleAsset {
                output_path: ".github/ISSUE_TEMPLATE/idea.yml",
                contents: include_bytes!("../modules/issue-forms/assets/idea.yml"),
            },
            ModuleAsset {
                output_path: ".github/ISSUE_TEMPLATE/bug.yml",
                contents: include_bytes!("../modules/issue-forms/assets/bug.yml"),
            },
            ModuleAsset {
                output_path: ".github/ISSUE_TEMPLATE/spec-request.yml",
                contents: include_bytes!("../modules/issue-forms/assets/spec-request.yml"),
            },
            ModuleAsset {
                output_path: ".github/ISSUE_TEMPLATE/config.yml",
                contents: include_bytes!("../modules/issue-forms/assets/config.yml"),
            },
        ],
    },
];

#[must_use]
pub const fn builtin_modules() -> &'static [BuiltinModule] {
    &MODULES
}
