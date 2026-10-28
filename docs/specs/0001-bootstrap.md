# M0: the first factory

## Goal

Create `tylerbutler/acme` as a public Rust repository with the minimum hand-built
factory. Use the Rust template as a basis. These files define the first
renderer fixtures, not a claim that the CLI exists.

## Inputs and outputs

| Stage | Input | Output | Gate |
| --- | --- | --- | --- |
| Intake | Admin-opened issue | Labels and clarification comment | Goal, acceptance scenarios, non-goals |
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
5. Both compiled agent workflows are live and enabled, and both enablement
   variables are true under the owner's continuous-operation approval.
   Intake cannot approve a build or merge.
6. Only an owner-applied `ready-for-agent` event can reach the assignment job.
7. Missing credentials and all bootstrap exceptions are documented.
8. The draft config matches the installed modules. It marks later modules off.

## M0 exit check, verified

An issue goes from opened, through human-approved spec and cloud assignment,
to a draft agent PR, green checks and a gated human merge.

Verified on 2026-10-26: [tylerbutler/acme#11](https://github.com/tylerbutler/acme/issues/11)
produced the Copilot draft [tylerbutler/acme#13](https://github.com/tylerbutler/acme/pull/13),
which passed required CI and was merged by the owner. The
[live-loop record](../bootstrap.md#verified-m0-loop) links the assignment,
builder, CI and human-merge evidence. Live Intake writes were not part of this
build test; Intake has separate staged coverage.

## Non-goals

Do not implement CLI commands, the schema generator, renderer, doctor, init
skills, holdout, build-local, release, ops or a second template in M0.
Do not create a release tag for a non-working binary.
Do not claim that CODEOWNERS enforces `never-agent`.

## Next milestones

M1 and M2 code is merged. M3 is the current priority. Merge the M3 spec, then
build its slices in order: plan, render, sync, version pins. Automatic dispatch
([spec 0010](0010-spec-dispatch-coordinator.md)) is paused and is not on the
path to any milestone below.

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
