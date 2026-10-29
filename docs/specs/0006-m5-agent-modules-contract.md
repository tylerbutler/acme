# M5: rendered Intake and Build-cloud

**Status: owner-selected design decisions; pending spec merge, not
implementation or rollout approval.**

## Goal and basis

Replace acme's hand-written Intake and Build-cloud files with reproducible
modules without changing their behavior or current live operation. Follow the
[roadmap](0001-bootstrap.md#next-milestones),
[factory boundaries](../adr/0001-factory-boundaries.md) and
[implemented M3 contract](0004-m3-render-sync-contract.md).
The M0 loop is a behavior fixture, not permission to broaden agent authority.

## Contract

- Add manifests, assets, config/schema support and doctor checks for both
  modules. Reuse M3 dependency validation, safe adoption and owned-file sync.
  Declare exact ownership of workflow Markdown, corresponding compiler output
  and required runtime helpers. Never hand-edit generated lock files.
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
  model settings distinct, as defined below. Support Copilot for these stages;
  M5 does not broaden engine support or enable review.
- Preserve explicit credential-secret references, pinned compiler/action
  versions, read-only agents, safe-output permissions, staging and per-run
  limits. A main-agent cap is not a total cloud-builder or monthly spend cap.
  New config fields need matching schema and compatibility checks.
- Extend read-only doctor to inspect rendered workflow guards, permissions,
  staging, activation variables and credential-name metadata. Do not read
  secret values. Presence does not prove token scopes or assignment capability.
  Doctor must not run the compiler or inference.

## Compilation and ownership handoff

Use an explicit, separate acme compilation command. Normal `render`, `sync`
and `sync --check` never invoke gh-aw. The compilation command uses the same
approved config, module assets, dependency checks and version checks as
render planning.

1. Require an already-installed compiler matching `toolchain.gh_aw.version`,
   currently `v0.93.5`, and the required local validation tools. Do not install
   tools, check for updates, fetch imports or contact GitHub. Missing tools,
   version mismatches, unsupported inputs and compilation errors fail
   explicitly. Compilation must work without credentials or paid inference.
2. Render the expected workflow sources and compile them in an isolated local
   workspace with reviewed, fixed compiler options and action pins. Do not
   execute configured CI commands, repository scripts or workflow jobs.
   Ambient model/compiler settings must not change the result.
3. Write only to a caller-selected new or empty review directory. Include the
   agent-module source and compiled files and a deterministic input/output
   record. Do not update checkout files or `factory/managed-files.json`.
   Do not publish a completed record after failed compilation.
4. Bind the record to the approved config, acme version, bundled asset hashes,
   compiler version/options and sorted output paths and content hashes. Exclude
   timestamps, absolute host paths and credentials. This record detects
   mismatches; it is not a signature or proof of compiler trust.
5. When agent workflows are enabled, require an explicitly supplied compiled
   review directory for full render or sync. Recompute expected sources from
   approved inputs and validate the record, versions, exact inventory, paths
   and file hashes before writes. Reject missing, stale, edited, extra or
   incompatible compiled inputs; never recompile them implicitly.
6. Apply M3 ownership, conflict, symlink and non-regular-file checks to the
   complete source/lock/helper plan before checkout mutation. Record these
   outputs in the existing managed-file record, written last. Check mode
   writes nothing; a second sync is a no-op. Preserve M3's documented partial
   I/O failure and concurrent-writer limitations rather than promising rollback.

Configurations with no rendered agent workflows retain the existing M2
render/sync path without a compiler or compiled-input requirement. Module
removal deletes only recorded, unmodified paths owned by that module; it does
not require compilation of a disabled workflow.

The implementation issue will select command/flag names and the review-record
format. It must test repeatable compilation in different temporary locations
against the pinned compiler. Do not normalize reviewed fixtures or hand-edit
locks to hide compiler drift. Plain hashes do not authenticate an untrusted
review directory; the owner reviews generated changes before publication.

## Model configuration and compatibility

Add these optional nonempty string fields to the Rust config types and
generated schema. Omission preserves current behavior; do not migrate or
rename the existing builder-model field.

| Config fields | Role and mapping |
| --- | --- |
| `engines.intake.controller_model`, `engines.build_cloud.controller_model` | Optional workflow-controller model, rendered to the corresponding workflow's `engine.model` |
| `engines.intake.detection_model`, `engines.build_cloud.detection_model` | Optional detection model, rendered to `safe-outputs.threat-detection.model` |
| `engines.build_cloud.model` | Existing issue-assigned builder model, rendered only to `safe-outputs.assign-to-agent.model` |

With both new fields omitted, preserve the current workflow source bytes and
gh-aw variable/default selection, including its `auto` and detection fallback
paths. A controller-only override must not silently change detection selection.
The pinned compiler can inherit the controller model into detection; preserve
the existing detection selection and fallback when no detection override is
requested. Verify controller-only, detection-only and combined overrides.

Use typed values and safe serialization, not user-supplied workflow fragments
or expressions. Reject empty, malformed or compiler-unsupported overrides
before publication; do not ignore or substitute them. The pinned compiler's
[schema](https://github.com/github/gh-aw/blob/v0.93.5/pkg/parser/schemas/main_workflow_schema.json)
and [model precedence tests](https://github.com/github/gh-aw/blob/v0.93.5/pkg/workflow/threat_detection_model_test.go)
provide the initial mapping evidence. Implementation still needs local
compiler-backed checks; this spec does not claim that they passed.

Keep schema version 0 with additive optional fields and test that existing
approved configs retain their behavior. A binary that predates the new fields
need not accept them; the concrete acme pin still selects the compatible
binary. Do not change the live pin or model choices in this spec PR.

Record configured model choices, runtime selection expressions and available
usage evidence separately. A configured ID, alias, `auto` or engine name does
not prove the serving model or builder/reviewer separation. Do not infer model
availability, token scopes or billing from successful compilation.

## Activation consent and live-state preservation

Owner approval remains a separate operational gate, not a new local approval
flag or ledger. Config review and successful compile/render/sync do not
authorize publication, activation, assignment or paid tests.

- New-project fixtures default to staged safe outputs, disabled workflow and
  assignment flags, and remote activation gates off. Generate guarded files;
  do not provision GitHub variables or enable workflows.
- Existing approved acme configs preserve their current live behavior, gates,
  staging and budgets. Do not disable or restage the live factory for adoption
  or a golden test. Use isolated fixtures for staged/inactive checks.
- Read-only doctor reports drift and permission/API failures. Ordinary
  `doctor --fix` plans must not change workflow activation, assignment gates,
  secrets or budgets, or launch verification runs.
- Publishing workflows, changing remote activation gates and staged/live
  acceptance runs each require separate owner approval. Record approved scope
  and evidence in the rollout issue; staged inference is still paid.
- Existing continuous-operation approval remains in force. It is not approval
  for new acceptance-test events, additional repositories or broader authority.

## Guidance and helper ownership

Keep `AGENTS.md` fully human-owned. Do not adopt marked sections, insert a
managed block or create a separate generated guidance file in M5. Workflow
prompts hold the generated safety instructions and continue to read the
project's authored agent map.

Build-cloud owns the portable approval helper at
`scripts/check-build-approval.py`, in addition to its workflow source and lock.
The helper is required because the current workflow executes it before
assignment. Preserve its read-only GitHub evidence checks and include it in
golden, conflict, ownership and removal tests. Do not claim the whole `scripts/`
directory or copy unrelated acme tooling into projects.

Preflight must check referenced authored prerequisites, including `AGENTS.md`
and `commit-types.json`, with clear guidance when they are missing or invalid.
Do not generate or overwrite these files. The helper also requires its
documented runtime tools; declare these requirements rather than assuming
they exist. The existence of a linked project spec is checked for each build
event, not required before generating a new project's workflows.

Removing either module preserves authored instructions, commit configuration,
briefs, specs and unrelated helpers. Prompts and CODEOWNERS do not implement
agent-author/path restrictions.

## Acceptance scenarios

1. The approved acme config reproduces reviewed workflow sources and their
   pinned-compiler lock output and approval helper. Repeated explicit
   compilation in different temporary locations produces identical output
   bytes and records without credentials or network access. Safe adoption
   followed by sync/check has no content diff; a second sync is idempotent.
2. Fixtures exercise valid and rejected events: non-owner labeler, missing
   spec/scope, missing ready label, disabled gates and repeated assignment.
   No rejected path can assign, merge or broaden permissions.
3. Missing/wrong compiler tools, stale or edited review directories, extra or
   unsafe output paths, missing authored prerequisites and file conflicts
   fail before checkout mutation. No implicit compilation, installation or
   activation occurs. Removal preserves `AGENTS.md` and unrelated files.
4. Existing configs and omitted model fields preserve behavior; controller-only,
   detection-only and combined overrides affect only the intended model roles.
   New-project fixtures remain staged/inactive; live fixtures preserve approved
   state. Deterministic tests need no credentials, remote mutations or paid
   runs. Doctor reports unavailable evidence without activating or repairing
   agent gates. Existing CI and human-merge boundaries remain intact.
5. After separate owner approval, record staged then live event-path evidence,
   including real assignment, draft PR, CI and human merge. Staged inference
   is still paid; planning or golden success does not authorize these tests.

## Dependencies and implementation approval

M3 implementation and stage0 adoption are complete:
[version work #26](https://github.com/tylerbutler/acme/issues/26) and
[human-merged adoption #87](https://github.com/tylerbutler/acme/pull/87).
[M4 #35](https://github.com/tylerbutler/acme/issues/35) is also closed, with its
recorded sample-run exceptions. M4 was not a prerequisite for adoption on
acme; template integration can be a separately scoped child issue.

The owner selected the compilation, model, activation and guidance boundaries
above on 2026-10-29. These replace the draft's open design decisions, not the
requirement to merge this spec before implementation.

Split implementation into bounded child issues for config/module inventory,
explicit compilation and validated handoff, workflow rendering/safe sync,
read-only doctor, and separately approved staged/live acceptance. Combine
closely coupled work when a smaller split would leave an unusable interface.
Each child must name its actual prerequisites and require separate owner
approval. Do not assign the milestone tracker to a builder.

Live adoption of a new CLI version still needs a real owner-published release,
an explicitly installed matching binary, a reviewed config pin upgrade, golden
comparison, safe sync/check and read-only doctor. Do not use a fictitious pin
or bootstrap mode to bypass version enforcement.

## Non-goals and approval

No review/holdout, private scenarios, local workers, release/ops, automated
dependency dispatch, CI-repair loop or autonomy change. No workflow, token,
budget or activation changes are authorized by this spec PR.

Keep the milestone tracker unassigned and never ready for a builder. Until this
spec is merged it remains `needs-spec` and blocked on spec approval, not M3.
Each implementation child needs an approved spec and one draft PR. Only the
owner may approve it with the ready label or direct Copilot assignment.
Module/schema/renderer changes remain `human-merge`.
