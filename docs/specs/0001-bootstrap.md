# M0: the first factory

## Goal

Create `tylerbutler/acme` as a public Rust repository with the minimum hand-built
factory. Use the Rust template as a basis. These files define the first
renderer fixtures, not a claim that the CLI exists.

## Inputs and outputs

| Stage | Input | Output | Gate |
| --- | --- | --- | --- |
| Intake | Owner-opened issue | Staged labels and clarification comment | Goal, acceptance scenarios, non-goals |
| Spec | Triaged issue and brief | Human-written spec | Owner adds `ready-for-agent` |
| Build-cloud | Approved issue and spec | Live assignment; one draft PR | Owner actor, ready label, explicit enablement |
| CI | PR | `CI` status | fmt, Clippy, tests, config consistency |
| Merge | Green PR | Main commit | Human merge, required CI, resolved threads |

The assignment workflow itself has read-only agent permissions. Any eventual
write must go through its declared safe output. The builder is a separate
Copilot cloud agent and must follow `AGENTS.md`.

## Repository acceptance

1. An independent public repository exists with the adapted Cargo scaffold.
2. `just check` passes; the CI workflow reports a `CI` check.
3. GitHub has the issue-state labels and the active main ruleset from the fixtures.
4. The ruleset requires a PR and CI, uses zero approvals, and has no bypass.
5. Both compiled agent workflows are disabled, and their enablement variables
   are false. Intake stays staged; build-cloud assignment is live when enabled.
   Each paid test needs explicit owner approval.
6. Only an owner-applied `ready-for-agent` event can reach the assignment job.
7. Missing credentials and all bootstrap exceptions are documented.
8. The draft config matches the installed modules. It marks later modules off.

## M0 exit check, pending live verification

An issue goes from opened, through human-approved spec and cloud assignment,
to a draft agent PR, green checks and a gated human merge.

This check needs credentials and approval for a paid run. Repository setup
alone does not satisfy it.

## Non-goals

Do not implement CLI commands, the schema generator, renderer, doctor, init
skills, holdout, build-local, release, ops or a second template in M0.
Do not create a release tag for a non-working binary.
Do not claim that CODEOWNERS enforces `never-agent`.

## Next milestones

| Milestone | Deliverable | Exit check |
| --- | --- | --- |
| M1 | Rust config types, generated schema, module manifests | Schema validates this config |
| M2 | Deterministic modules and doctor | Detect and repair deliberate setting drift |
| M3 | Render, sync and version pinning | Zero golden diff, idempotent sync and clean version bump |
| M4 | Thin template and interview skill | Brief to merged init PR on a sample project |
| M5 | Rendered intake and build-cloud | No manual workflow drift; staged then live |
| M6 | Review and isolated holdout | Block a seeded bad PR that passes CI |
| M7 | Local builds, provenance, release and ops | Independent tasks through gated merges |
| M8 | Second project from template | No hand edits to rendered files |
