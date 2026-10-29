//! Agent sources are separate from the M2 render/sync inventory.

use crate::{ManifestError, ModuleAsset, ModuleManifest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentModule {
    Intake,
    BuildCloud,
}

impl AgentModule {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Intake => "intake",
            Self::BuildCloud => "build_cloud",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentPathKind {
    WorkflowSource,
    CompilerOutput,
    RuntimeHelper,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentOwnedPath {
    pub module: AgentModule,
    pub path: &'static str,
    pub kind: AgentPathKind,
}

#[derive(Debug)]
pub struct AgentModuleDefinition {
    pub module: AgentModule,
    pub manifest_toml: &'static str,
    pub assets: &'static [ModuleAsset],
    pub ownership: &'static [AgentOwnedPath],
    pub authored_prerequisites: &'static [&'static str],
    pub runtime_tools: &'static [&'static str],
}

impl AgentModuleDefinition {
    pub fn manifest(&self) -> Result<ModuleManifest, ManifestError> {
        ModuleManifest::from_toml(self.manifest_toml)
    }
}

const MODULES: [AgentModuleDefinition; 2] = [
    AgentModuleDefinition {
        module: AgentModule::Intake,
        manifest_toml: include_str!("../modules/intake/module.toml"),
        assets: &[ModuleAsset {
            output_path: ".github/workflows/intake.md",
            contents: include_bytes!("../modules/intake/assets/intake.md"),
        }],
        ownership: &[
            AgentOwnedPath {
                module: AgentModule::Intake,
                path: ".github/workflows/intake.md",
                kind: AgentPathKind::WorkflowSource,
            },
            AgentOwnedPath {
                module: AgentModule::Intake,
                path: ".github/workflows/intake.lock.yml",
                kind: AgentPathKind::CompilerOutput,
            },
        ],
        authored_prerequisites: &["AGENTS.md"],
        runtime_tools: &[],
    },
    AgentModuleDefinition {
        module: AgentModule::BuildCloud,
        manifest_toml: include_str!("../modules/build-cloud/module.toml"),
        assets: &[
            ModuleAsset {
                output_path: ".github/workflows/build-cloud.md",
                contents: include_bytes!("../modules/build-cloud/assets/build-cloud.md"),
            },
            ModuleAsset {
                output_path: "scripts/check-build-approval.py",
                contents: include_bytes!("../modules/build-cloud/assets/check-build-approval.py"),
            },
        ],
        ownership: &[
            AgentOwnedPath {
                module: AgentModule::BuildCloud,
                path: ".github/workflows/build-cloud.md",
                kind: AgentPathKind::WorkflowSource,
            },
            AgentOwnedPath {
                module: AgentModule::BuildCloud,
                path: ".github/workflows/build-cloud.lock.yml",
                kind: AgentPathKind::CompilerOutput,
            },
            AgentOwnedPath {
                module: AgentModule::BuildCloud,
                path: "scripts/check-build-approval.py",
                kind: AgentPathKind::RuntimeHelper,
            },
        ],
        authored_prerequisites: &["AGENTS.md", "commit-types.json"],
        runtime_tools: &["python3", "gh"],
    },
];

#[must_use]
pub const fn agent_modules() -> &'static [AgentModuleDefinition] {
    &MODULES
}
