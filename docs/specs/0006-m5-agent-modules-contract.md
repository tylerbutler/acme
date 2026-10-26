# M5: rendered Intake and Build-cloud

**Status: proposed milestone plan, not implementation approval.**

## Goal and basis

Replace acme's hand-written Intake and Build-cloud files with reproducible
modules without changing their behavior or current live operation. Follow the
[roadmap](0001-bootstrap.md#next-milestones),
[factory boundaries](../adr/0001-factory-boundaries.md) and
[proposed M3 contract](https://github.com/tylerbutler/acme/pull/30).
The M0 loop is a behavior fixture, not permission to broaden agent authority.

## Contract

- Add manifests, assets, config/schema support and doctor checks for both
  modules. Reuse M3 dependency validation, safe adoption and owned-file sync.
  Declare ownership of workflow Markdown and corresponding compiler output;
  never hand-edit generated lock files.
- Intake handles approved issue-opened events, with read-only agent access
  and declared label/comment safe outputs. It cannot assign a builder, apply
  the ready label, approve its own work or merge.
- Build-cloud retains owner and triggering-owner checks, approved spec/scope
  checks, explicit variable gates, the owner-applied ready-label event and
  one triggering issue/one assignment/one draft PR. Existing labels do not
  replay events, and native dependency completion does not dispatch a build.
  Preserve runtime targets without claiming that an instructional 59-minute
  target is an enforced cloud-runtime or spending limit.
- Keep controller inference, cloud-builder assignment and threat detection
  model settings distinct. Record configured model choices and available
  usage evidence; `copilot` is an engine, not a model-family guarantee.
  Document any API limitation rather than inventing serving-model proof.
- Preserve explicit credential-secret references, pinned compiler/action
  versions, read-only agents, safe-output permissions, staging and per-run
  limits. A main-agent cap is not a total cloud-builder or monthly spend cap.
  New config fields need matching schema and compatibility checks.
- New projects default to staged outputs and disabled activation/assignment
  gates. Existing approved live configs must render as approved; do not
  silently turn acme off or restage it for a golden test.
- Extend read-only doctor to inspect rendered workflow guards, permissions,
  staging, activation variables and credential presence. Presence does not
  prove token scopes or assignment capability. Keep activation and paid
  verification separate from normal rendering, sync and default repairs.
- Preserve durable agent guidance. Any adoption of AGENTS.md content needs
  an explicit ownership boundary and conflict handling; M3 currently protects
  it. Prompts and CODEOWNERS alone do not implement author/path restrictions.

## Acceptance scenarios

1. The approved acme config reproduces reviewed workflow sources and their
   pinned-compiler lock output. Safe adoption followed by sync/check has no
   content diff; a second sync is idempotent.
2. Fixtures exercise valid and rejected events: non-owner labeler, missing
   spec/scope, missing ready label, disabled gates and repeated assignment.
   No rejected path can assign, merge or broaden permissions.
3. New-project fixtures remain staged and inactive. Existing live fixtures
   preserve their approved state. Missing tools, incompatible compiler output
   and credential/API failures remain explicit errors.
4. Deterministic tests need no credentials, remote mutations or paid runs.
   Existing required CI and human-merge boundaries remain intact.
5. After separate owner approval, record staged then live event-path evidence,
   including real assignment, draft PR, CI and human merge. Staged inference
   is still paid; planning or golden success does not authorize these tests.

## Dependencies and decisions before implementation

Depend on completed M3 adoption/version work,
[tylerbutler/acme#26](https://github.com/tylerbutler/acme/issues/26).
M4 is not a prerequisite for adopting these modules on acme; template
integration can follow separately once both milestones are available.

Resolve the pinned compilation boundary before coding: M3 render/sync do not
invoke gh-aw. Choose and test a reproducible compilation/ownership hand-off;
do not add hidden tool installation, network access or compiler invocation.
Also settle engine/model schema fields, activation consent and agent-map
ownership. Do not assume unspecified model IDs or API capabilities.

## Non-goals and approval

No review/holdout, private scenarios, local workers, release/ops, automated
dependency dispatch, CI-repair loop or autonomy change. No workflow, token,
budget or activation changes are authorized by this spec PR.

Keep the milestone tracker blocked and never ready/assigned to a builder.
After prerequisite evidence and design decisions, create small build issues,
each with an approved spec, one draft PR and an owner-applied ready label.
Module/schema/renderer changes remain `human-merge`.
