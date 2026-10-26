use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DISPATCH_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DispatchDeclaration {
    pub version: u32,
    pub repository: String,
    pub issue: u64,
    pub spec: SpecBinding,
    pub issue_body: ContentSnapshot,
    pub kind: TaskKind,
    pub prerequisites: Vec<Prerequisite>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SpecBinding {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContentSnapshot {
    pub content: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Implementation,
    PlanningTracker,
    ManualOperation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Prerequisite {
    pub issue: u64,
    #[serde(default)]
    pub owner_acceptance_required: bool,
}

impl DispatchDeclaration {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DispatchContractError> {
        let path = path.as_ref();
        let input = fs::read_to_string(path).map_err(|source| DispatchContractError::Io {
            path: path.to_owned(),
            source,
        })?;
        Self::from_toml(&input)
    }

    pub fn from_toml(input: &str) -> Result<Self, DispatchContractError> {
        let declaration: Self = toml::from_str(input).map_err(DispatchContractError::Parse)?;
        declaration.validate()?;
        Ok(declaration)
    }

    pub fn validate(&self) -> Result<(), DispatchContractError> {
        if self.version != DISPATCH_VERSION {
            return Err(DispatchContractError::Invalid(
                DispatchReason::UnsupportedVersion,
            ));
        }
        if !valid_repository(&self.repository) {
            return Err(DispatchContractError::Invalid(
                DispatchReason::InvalidRepository,
            ));
        }
        if self.issue == 0 {
            return Err(DispatchContractError::Invalid(DispatchReason::InvalidIssue));
        }
        if !valid_spec_path(&self.spec.path) {
            return Err(DispatchContractError::Invalid(
                DispatchReason::InvalidSpecPath,
            ));
        }
        if !valid_sha256(&self.spec.sha256) {
            return Err(DispatchContractError::Invalid(
                DispatchReason::InvalidSpecHash,
            ));
        }
        if !valid_sha256(&self.issue_body.sha256)
            || self.issue_body.sha256 != content_sha256(&self.issue_body.content)
        {
            return Err(DispatchContractError::Invalid(
                DispatchReason::InvalidBodyHash,
            ));
        }
        if self.issue_body.content.trim().is_empty() {
            return Err(DispatchContractError::Invalid(
                DispatchReason::EmptyIssueBody,
            ));
        }
        let mut previous_issue = 0;
        for prerequisite in &self.prerequisites {
            if prerequisite.issue == 0 || prerequisite.issue == self.issue {
                return Err(DispatchContractError::Invalid(
                    DispatchReason::InvalidPrerequisite,
                ));
            }
            if prerequisite.issue <= previous_issue {
                return Err(DispatchContractError::Invalid(
                    DispatchReason::UnsortedOrDuplicatePrerequisites,
                ));
            }
            previous_issue = prerequisite.issue;
        }
        Ok(())
    }
}

#[must_use]
pub fn content_sha256(content: &str) -> String {
    let canonical = content.replace("\r\n", "\n").replace('\r', "\n");
    let digest = Sha256::digest(canonical.as_bytes());
    let mut output = String::with_capacity(64);
    for byte in digest {
        use fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

#[must_use]
pub fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_repository(value: &str) -> bool {
    let Some((owner, repository)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty()
        && !repository.is_empty()
        && !repository.contains('/')
        && [owner, repository].iter().all(|part| {
            part.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
}

fn valid_spec_path(value: &str) -> bool {
    value.starts_with("docs/specs/")
        && value.ends_with(".md")
        && !value.starts_with('/')
        && !value.contains('\\')
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchReason {
    UnsupportedVersion,
    InvalidRepository,
    InvalidIssue,
    InvalidSpecPath,
    InvalidSpecHash,
    InvalidBodyHash,
    EmptyIssueBody,
    InvalidPrerequisite,
    UnsortedOrDuplicatePrerequisites,
}

impl DispatchReason {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnsupportedVersion => "unsupported_version",
            Self::InvalidRepository => "invalid_repository",
            Self::InvalidIssue => "invalid_issue",
            Self::InvalidSpecPath => "invalid_spec_path",
            Self::InvalidSpecHash => "invalid_spec_hash",
            Self::InvalidBodyHash => "invalid_body_hash",
            Self::EmptyIssueBody => "empty_issue_body",
            Self::InvalidPrerequisite => "invalid_prerequisite",
            Self::UnsortedOrDuplicatePrerequisites => "unsorted_or_duplicate_prerequisites",
        }
    }
}

#[derive(Debug)]
pub enum DispatchContractError {
    Io { path: PathBuf, source: io::Error },
    Parse(toml::de::Error),
    Invalid(DispatchReason),
}

impl fmt::Display for DispatchContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    f,
                    "failed to read dispatch declaration {}: {source}",
                    path.display()
                )
            }
            Self::Parse(source) => write!(f, "failed to parse dispatch declaration TOML: {source}"),
            Self::Invalid(reason) => write!(f, "invalid dispatch declaration: {}", reason.code()),
        }
    }
}

impl Error for DispatchContractError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse(source) => Some(source),
            Self::Invalid(_) => None,
        }
    }
}
