pub mod assignment;
pub mod catalog;
pub mod claim_store;
pub mod claims;
pub mod config;
pub mod coordinator;
pub mod coordinator_event;
pub mod dispatch;
pub mod doctor;
pub mod github_state;
pub mod manifest;
pub mod render;

pub use catalog::{BuiltinModule, ConfigBinding, ModuleAsset, builtin_modules};
pub use config::{ConfigError, CoordinatorConfig, FactoryConfig};
pub use coordinator::{
    AcceptanceReceipt, ApprovalEvidence, CapacityState, Eligibility, Evidence, ExistingWork,
    IssueState, KillSwitches, PlannerInput, PrerequisiteCompletion, ReasonCode, TaskDecision,
    TaskObservation, plan,
};
pub use dispatch::{
    ContentSnapshot, DispatchContractError, DispatchDeclaration, DispatchReason, Prerequisite,
    SpecBinding, TaskKind, content_sha256,
};
pub use doctor::{
    Check, DoctorReport, GitHubError, GitHubMethod, GitHubTransport, checkout_repository,
    run_doctor, run_doctor_fix,
};
pub use manifest::{ManifestError, ModuleManifest};
