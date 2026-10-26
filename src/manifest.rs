use serde::Deserialize;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq)]
pub struct ModuleManifest {
    pub name: String,
    pub needs: Vec<String>,
    pub provides: Vec<String>,
    pub gate: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModuleManifest {
    name: String,
    needs: Vec<String>,
    provides: Vec<String>,
    gate: String,
}

impl ModuleManifest {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path = path.as_ref();
        let input = fs::read_to_string(path).map_err(|source| ManifestError::Io {
            path: path.to_owned(),
            source,
        })?;
        Self::from_toml(&input)
    }

    pub fn from_toml(input: &str) -> Result<Self, ManifestError> {
        let parsed: RawModuleManifest = toml::from_str(input).map_err(ManifestError::Parse)?;
        let manifest = Self {
            name: parsed.name,
            needs: parsed.needs,
            provides: parsed.provides,
            gate: parsed.gate,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        for (field, value) in [("name", self.name.as_str()), ("gate", self.gate.as_str())] {
            if value.trim().is_empty() {
                return Err(ManifestError::EmptyValue(field));
            }
        }

        for (field, values) in [("needs", &self.needs), ("provides", &self.provides)] {
            if values.iter().any(|value| value.trim().is_empty()) {
                return Err(ManifestError::EmptyValue(field));
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum ManifestError {
    Io { path: PathBuf, source: io::Error },
    Parse(toml::de::Error),
    EmptyValue(&'static str),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to read module manifest {}: {source}",
                    path.display()
                )
            }
            Self::Parse(source) => write!(f, "failed to parse module manifest TOML: {source}"),
            Self::EmptyValue(field) => {
                write!(f, "module manifest field {field} must not be empty")
            }
        }
    }
}

impl Error for ManifestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse(source) => Some(source),
            Self::EmptyValue(_) => None,
        }
    }
}
