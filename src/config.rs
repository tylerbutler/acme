use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct FactoryConfig {
    pub schema_version: u32,
    pub repository: String,
    pub coordinator: CoordinatorConfig,
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

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RawFactoryConfig {
    #[schemars(range(min = 0, max = 0))]
    schema_version: u32,
    repository: String,
    #[serde(default)]
    coordinator: CoordinatorConfig,
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

#[must_use]
pub fn factory_schema() -> schemars::schema::RootSchema {
    schemars::schema_for!(RawFactoryConfig)
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
            coordinator: parsed.coordinator,
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

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorConfig {
    #[serde(default)]
    pub automatic_dispatch: bool,
    #[serde(default)]
    pub max_active_tasks: u32,
    #[serde(default)]
    pub max_assignments_per_reconciliation: u32,
    #[serde(default)]
    pub max_attempts: u32,
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

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolchainConfig {
    pub acme: AcmeToolchainConfig,
    pub gh_aw: GhAwToolchainConfig,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcmeToolchainConfig {
    pub bootstrap: bool,
    pub target_version: String,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GhAwToolchainConfig {
    pub version: String,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetConfig {
    pub enforce_monthly: bool,
    pub enforce_ai_credits: bool,
    pub max_ai_credits_per_run: u32,
    pub reason: String,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnginesConfig {
    pub intake: EngineConfig,
    pub build_cloud: BuildCloudEngineConfig,
    pub review: ReviewEngineConfig,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineConfig {
    pub engine: String,
    pub token_secret: String,
    pub controller_model: Option<ModelId>,
    pub detection_model: Option<ModelId>,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildCloudEngineConfig {
    pub engine: String,
    pub model: String,
    pub token_secret: String,
    pub controller_model: Option<ModelId>,
    pub detection_model: Option<ModelId>,
}

/// A literal model identifier, never a workflow expression or YAML fragment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "String")]
pub struct ModelId(String);

impl JsonSchema for ModelId {
    fn schema_name() -> String {
        "ModelId".to_owned()
    }

    fn json_schema(generator: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        let mut schema = String::json_schema(generator).into_object();
        let validation = schema.string();
        validation.min_length = Some(1);
        validation.max_length = Some(128);
        validation.pattern = Some("^[A-Za-z0-9][A-Za-z0-9._-]*$".to_owned());
        schema.into()
    }
}

impl ModelId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn validate(value: &str) -> Result<(), &'static str> {
        if value.len() > 128
            || !value.starts_with(|character: char| character.is_ascii_alphanumeric())
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(
                "model must be 1..128 ASCII letters, digits, dots, underscores or hyphens, starting with a letter or digit",
            );
        }
        Ok(())
    }
}

impl TryFrom<String> for ModelId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::validate(&value)?;
        Ok(Self(value))
    }
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewEngineConfig {
    pub engine: String,
    pub enabled: bool,
    pub model_policy: String,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
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

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
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

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpecModuleConfig {
    pub enabled: bool,
    pub implementation: String,
    pub gate: String,
    pub needs: Vec<String>,
    pub provides: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
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

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CiModuleConfig {
    pub enabled: bool,
    pub gate: String,
    pub required_checks: Vec<String>,
    pub commands: Vec<String>,
    pub step_names: Vec<String>,
    pub provides: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
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

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DisabledModuleConfig {
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RiskConfig {
    #[serde(rename = "default")]
    pub default_level: String,
    pub auto_merge: Vec<String>,
    pub human_merge: Vec<String>,
    pub never_agent: Vec<String>,
    pub path_gate_enforced: bool,
}

#[derive(Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapConfig {
    pub milestone: String,
    pub golden_source: String,
    pub live_loop_verified: bool,
    pub deferred: Vec<String>,
}
