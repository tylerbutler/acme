use crate::dispatch::{DispatchDeclaration, TaskKind, content_sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Capability {
    Supported,
    Unsupported,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssignmentCapabilities {
    pub exact_target: Capability,
    pub immutable_task: Capability,
}

impl AssignmentCapabilities {
    #[must_use]
    pub const fn gh_aw_0_93_5() -> Self {
        Self {
            exact_target: Capability::Supported,
            immutable_task: Capability::Unsupported,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssignmentRequest {
    declaration: DispatchDeclaration,
    contract_sha256: String,
    approved_main_revision: String,
}

impl AssignmentRequest {
    /// The caller must verify approval, protected-main provenance, eligibility, and the claim before handoff.
    pub fn from_approved_snapshot(contract: &str, main_revision: &str) -> Result<Self, String> {
        let declaration =
            DispatchDeclaration::from_toml(contract).map_err(|error| error.to_string())?;
        if declaration.kind != TaskKind::Implementation {
            return Err("assignment requires an implementation task".to_owned());
        }
        if main_revision.len() != 40
            || !main_revision
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(
                "assignment requires a full lowercase protected-main commit SHA".to_owned(),
            );
        }
        Ok(Self {
            declaration,
            contract_sha256: content_sha256(contract),
            approved_main_revision: main_revision.to_owned(),
        })
    }

    #[must_use]
    pub const fn declaration(&self) -> &DispatchDeclaration {
        &self.declaration
    }

    #[must_use]
    pub fn contract_sha256(&self) -> &str {
        &self.contract_sha256
    }

    #[must_use]
    pub fn approved_main_revision(&self) -> &str {
        &self.approved_main_revision
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssignmentOutcome {
    Accepted,
    NotAssigned,
    Unknown,
}

pub trait AssignmentTransport {
    fn capabilities(&self) -> AssignmentCapabilities;
    fn assign(&self, request: &AssignmentRequest) -> AssignmentOutcome;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssignmentBlocked {
    pub capability: &'static str,
    pub status: Capability,
}

/// This capability gate does not replace approval checks, claim serialization, or final eligibility checks.
pub fn handoff(
    transport: &impl AssignmentTransport,
    request: &AssignmentRequest,
) -> Result<AssignmentOutcome, AssignmentBlocked> {
    let capabilities = transport.capabilities();
    for (capability, status) in [
        ("exact_target", capabilities.exact_target),
        ("immutable_task", capabilities.immutable_task),
    ] {
        if status != Capability::Supported {
            return Err(AssignmentBlocked { capability, status });
        }
    }
    Ok(transport.assign(request))
}
