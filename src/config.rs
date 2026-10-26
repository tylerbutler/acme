use serde::Deserialize;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq)]
pub struct FactoryConfig {
    pub schema_version: u32,
    pub repository: String,
    pub profile: String,
    pub languages: Vec<String>,
    pub autonomy: String,
    pub solo: bool,
    pub toolchain: ToolchainConfig,
    pub budget: BudgetConfig,
    pub engines: EnginesConfig,
    pub modules: ModulesConfig,
    pub risk: RiskConfig,
    pub bootstrap: BootstrapConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFactoryConfig {
    schema_version: u32,
    repository: String,
    profile: String,
    languages: Vec<String>,
    autonomy: String,
    solo: bool,
    toolchain: ToolchainConfig,
    budget: BudgetConfig,
    engines: EnginesConfig,
    modules: ModulesConfig,
    risk: RiskConfig,
    bootstrap: BootstrapConfig,
}

impl FactoryConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let input = fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_owned(),
            source,
        })?;
        Self::from_toml(&input)
    }

    pub fn from_toml(input: &str) -> Result<Self, ConfigError> {
        let parsed: RawFactoryConfig = toml::from_str(input).map_err(ConfigError::Parse)?;
        if parsed.schema_version != 0 {
            return Err(ConfigError::UnsupportedSchemaVersion(parsed.schema_version));
        }
        let config = Self {
            schema_version: parsed.schema_version,
            repository: parsed.repository,
            profile: parsed.profile,
            languages: parsed.languages,
            autonomy: parsed.autonomy,
            solo: parsed.solo,
            toolchain: parsed.toolchain,
            budget: parsed.budget,
            engines: parsed.engines,
            modules: parsed.modules,
            risk: parsed.risk,
            bootstrap: parsed.bootstrap,
        };

        for (name, module) in [
            ("review", &config.modules.review),
            ("holdout", &config.modules.holdout),
            ("build_local", &config.modules.build_local),
            ("release", &config.modules.release),
            ("ops", &config.modules.ops),
        ] {
            if module.enabled {
                return Err(ConfigError::EnabledStubModule(name));
            }
        }

        Ok(config)
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io { path: PathBuf, source: io::Error },
    Parse(toml::de::Error),
    UnsupportedSchemaVersion(u32),
    EnabledStubModule(&'static str),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to read factory config {}: {source}",
                    path.display()
                )
            }
            Self::Parse(source) => write!(f, "failed to parse factory TOML: {source}"),
            Self::UnsupportedSchemaVersion(version) => {
                write!(f, "unsupported factory schema version {version}")
            }
            Self::EnabledStubModule(name) => {
                write!(f, "module {name} has no enabled configuration")
            }
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse(source) => Some(source),
            Self::UnsupportedSchemaVersion(_) | Self::EnabledStubModule(_) => None,
        }
    }
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ToolchainConfig {
    pub acme: AcmeToolchainConfig,
    pub gh_aw: GhAwToolchainConfig,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AcmeToolchainConfig {
    pub bootstrap: bool,
    pub target_version: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GhAwToolchainConfig {
    pub version: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BudgetConfig {
    pub enforce_monthly: bool,
    pub enforce_ai_credits: bool,
    pub max_ai_credits_per_run: u32,
    pub reason: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EnginesConfig {
    pub intake: EngineConfig,
    pub build_cloud: BuildCloudEngineConfig,
    pub review: ReviewEngineConfig,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EngineConfig {
    pub engine: String,
    pub token_secret: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BuildCloudEngineConfig {
    pub engine: String,
    pub model: String,
    pub token_secret: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReviewEngineConfig {
    pub engine: String,
    pub enabled: bool,
    pub model_policy: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModulesConfig {
    pub intake: IntakeModuleConfig,
    pub spec: SpecModuleConfig,
    pub build_cloud: BuildCloudModuleConfig,
    pub ci: CiModuleConfig,
    pub merge: MergeModuleConfig,
    pub review: DisabledModuleConfig,
    pub holdout: DisabledModuleConfig,
    pub build_local: DisabledModuleConfig,
    pub release: DisabledModuleConfig,
    pub ops: DisabledModuleConfig,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IntakeModuleConfig {
    pub enabled: bool,
    pub gate: String,
    pub staged: bool,
    pub workflow_enabled: bool,
    pub roles: Vec<String>,
    pub needs: Vec<String>,
    pub provides: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpecModuleConfig {
    pub enabled: bool,
    pub implementation: String,
    pub gate: String,
    pub needs: Vec<String>,
    pub provides: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct BuildCloudModuleConfig {
    pub enabled: bool,
    pub gate: String,
    pub staged: bool,
    pub workflow_enabled: bool,
    pub assignment_enabled: bool,
    pub token_secret: String,
    pub token_configured: bool,
    pub token_verified: bool,
    pub max_repositories: u32,
    pub max_pull_requests: u32,
    pub target_minutes: u32,
    pub needs: Vec<String>,
    pub provides: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CiModuleConfig {
    pub enabled: bool,
    pub gate: String,
    pub required_checks: Vec<String>,
    pub commands: Vec<String>,
    pub provides: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MergeModuleConfig {
    pub enabled: bool,
    pub implementation: String,
    pub gate: String,
    pub required_approvals: u32,
    pub require_extra_approval_for_unattributed_changes: bool,
    pub auto_merge: bool,
    pub needs: Vec<String>,
    pub provides: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DisabledModuleConfig {
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RiskConfig {
    #[serde(rename = "default")]
    pub default_level: String,
    pub auto_merge: Vec<String>,
    pub human_merge: Vec<String>,
    pub never_agent: Vec<String>,
    pub path_gate_enforced: bool,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BootstrapConfig {
    pub milestone: String,
    pub golden_source: String,
    pub live_loop_verified: bool,
    pub deferred: Vec<String>,
}
