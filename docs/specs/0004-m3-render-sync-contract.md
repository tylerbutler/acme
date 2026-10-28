# M3: deterministic render, sync and version pins

## Goal and basis

Generate the M2 modules from approved config, synchronize only owned files,
and make upgrades explicit. Follow the
[M0 roadmap](0001-bootstrap.md#next-milestones),
[M1 contracts](0002-m1-config-contract.md),
[M2 contract](0003-m2-doctor-contract.md) and
[factory boundaries](../adr/0001-factory-boundaries.md).

The golden check covers the deterministic M2 inventory, not every file in the
repository. Rendering Intake and Build-cloud remains M5. The existing live
factory must keep working during implementation.

## Render contract

- Share M1 config/manifest loading and the M2 module catalog. Read
  `factory/factory.toml` by default; support `--config PATH`. Resolve only the
  supported deterministic modules. Validate enabled-stage capability needs,
  including dependencies on or from documented hand-built stages. Reject
  missing or ambiguous providers, cycles and output-path collisions. Each
  renderable module needs an implemented M2 gate check. Hand-built stages stay
  external; accepting their declared capabilities does not verify their gates
  or make them implemented render modules.
- Build and validate the complete render plan before writing. The same config,
  module assets and acme version produce the same sorted paths and bytes.
  Do not depend on timestamps, host paths, filesystem enumeration or network
  responses, and do not execute config commands or template content.
- `acme render --output DIR` writes a fresh output tree for inspection and
  golden comparison. Refuse a nonempty destination; use `sync` to update a
  project. Rendering is local and does not provision or contact GitHub.
- Generate only the M2 owned inventory. Preserve GitHub expression literals,
  pinned action SHAs, required check names and the configured CI commands.
  The unchanged post-M1 factory must reproduce its reviewed files byte for
  byte; do not normalize the fixtures to hide a mismatch.
- Write LF line endings. Compare raw bytes. Golden tests must not depend on the
  checkout's line-ending settings.

## Config-to-output mapping

Module assets become templates. Use plain placeholder substitution with no
conditionals, loops in template text or code execution. The renderer builds
lists, such as CI steps, in Rust and substitutes the rendered text. Template
text that has no config field stays fixed and reviewed.

| Config field | Output |
| --- | --- |
| `modules.ci.enabled` | Presence of `.github/workflows/ci.yml` and `.github/actions/setup-rust/action.yml` |
| `modules.ci.commands` | `run` steps in `.github/workflows/ci.yml`, in config order |
| `modules.ci.step_names` | The corresponding command step `name` in `.github/workflows/ci.yml`, in config order |
| `modules.ci.required_checks` | CI job `name`; required status check contexts in `.github/rulesets/main.json`. The current single-job template requires exactly one nonempty check when CI is enabled. |
| `modules.merge.enabled` | Presence of `.github/rulesets/main.json` |
| `modules.merge.required_approvals` | `required_approving_review_count` in the ruleset |
| `modules.merge.require_extra_approval_for_unattributed_changes` | `require_extra_approval_for_unattributed_changes` in the ruleset |
| `modules.intake.enabled` | Presence of `.github/labels.yml` and the issue forms |

Enabled merge requires nonempty required-check names even when an external
stage provides `ci` and the bundled CI module is disabled.

Slice A proposes the required `modules.ci.step_names` string list in schema
version 0. Keep `commands` as strings. Each enabled CI command needs one nonempty
step name at the same index. The checked-in config supplies the six reviewed
names and commands, including the schema freshness check. This schema change
needs human merge. The live files remain the reviewed source of truth; do not
change them to match incomplete config.

These are all config-dependent bytes in the M2 output inventory. Everything else
in these assets stays fixed, including workflow and job IDs, the workflow's
top-level name, action pins, Rust 1.85, runner, triggers, concurrency expressions,
permissions, label and form definitions, ruleset name, branch, enforcement,
integration ID, allowed merge methods and resolved-thread requirement.

The other config fields do not substitute output bytes:

- Enabled stages and their `needs` and `provides` declarations affect capability
  validation. Built-in manifest capabilities also apply. Enabled human merge
  additionally needs `ruleset`; its required-CI gate needs `ci`.
- CI's gate must be `required-status-check`. Merge must use the human
  implementation and `required-ci-and-resolved-threads`, with `auto_merge = false`.
  Built-in gates and config bindings must match implemented M2 checks. This
  validates supported declarations, not installed GitHub state.
- Intake, Spec, Build-cloud and Merge remain external stages. Their capabilities
  can satisfy dependencies, but the plan does not verify their external gates.
  Their other settings do not alter deterministic assets or agent workflows.
- The supported preset is `profile = "cli"` with `languages = ["rust"]`. Other
  profiles, languages and enabled stub modules are unsupported.
- Repository identity, autonomy, solo mode, engine, budget, risk and bootstrap
  metadata do not change these assets. Toolchain pins do not change them:
  gh-aw applies to hand-built workflows; acme version enforcement is slice D.

### Slice A plan interface

`acme::render::build_render_plan(&FactoryConfig)` uses the bundled M2 catalog.
`build_render_plan_with_catalog` accepts a supplied catalog for validation tests.
Both functions return a complete plan or an explicit error. They do not read
project files, inspect an output directory, execute commands or contact GitHub.
The caller loads approved config through M1 before planning.

The plan records the binary version, dependency-ordered renderable manifests,
name-ordered external stage declarations and path-ordered outputs. Each output
contains its module owner, relative path, unchanged bundled template source
bytes and typed CI, ruleset or static inputs. Capability lists are sorted and
deduplicated; dependency ties use module-name order. CI steps retain config
order. Duplicate providers and dependency cycles fail, including those that
involve external stages. The template source remains the M2 fixture in slice A;
placeholder substitution and final output bytes belong to slice B.

Path validation rejects absolute paths, empty or dot components, parent
traversal, control characters, backslashes, drive prefixes and Git metadata.
Paths use ASCII letters, digits, `-`, `_`, `.`, and `/`. Reject exact and
case-insensitive collisions, and file/directory overlaps. This is lexical plan
validation only. A supplied catalog must include each selected module's full M2
inventory and cannot claim other paths. Filesystem and symlink checks remain
required before writes in later slices.

## Sync and ownership contract

- `acme sync` applies the same plan to the current repository.
  `acme sync --check` lists the proposed creates, updates and removals, exits
  unsuccessfully on drift, and never writes, including ownership metadata.
  Both commands support the same `--config PATH`.
- Maintain a deterministic ownership record at `factory/managed-files.json`.
  Record its format version, generating acme version, module, relative output
  path and last generated content hash. Reject malformed or unsupported records.
  The record is bookkeeping, not permission to write arbitrary paths: validate
  it against the built-in module inventory and checkout boundary.
- A first sync may adopt an existing hand-built path only when its bytes
  already match the plan. A conflicting unowned path is an error, not an
  overwrite. For owned files, compare current bytes with the recorded hash
  before updating or deleting; edited files require human resolution.
- Disabling/removing a supported module may delete only its previously owned,
  unmodified outputs. Never remove a parent directory recursively, unrelated
  files, specs, skills, AGENTS.md or hand-built agent workflows.
- Reject absolute/escaping paths, symlink escapes, unsafe ownership records
  and overlapping outputs. Detect conflicts before mutation. Use safe file
  replacement; report I/O failures and partial application accurately.
  Never mark a failed sync complete in the ownership record.
- A clean second sync with the same binary has no diff, including the
  ownership record. After an upgrade that changes no output, sync may change
  only the recorded acme version. Sync does
  not call doctor `--fix`, compile gh-aw, change GitHub settings, assign work
  or merge a PR. Run read-only doctor separately to check installed settings.

## Version contract

Keep the existing `[toolchain.acme]` fields. In bootstrap mode,
`target_version` is a build target, not proof that a release exists.
Outside bootstrap, require a concrete version whose release tag is
`v<target_version>` and whose version matches the running binary before any
write. Reject moving references such as HEAD, `latest` and version ranges.
Do not download or execute another binary automatically.

An upgrade is a human-approved config pin change plus an explicitly installed
matching binary, followed by sync and review of the diff. Keep schema and
module contracts compatible or fail with migration guidance before writing.
Use controlled versioned fixtures to test an old-to-new upgrade; do not invent
a published release or change the live bootstrap flag to make tests pass.

## Acceptance scenarios

1. A supported config produces a stable plan. Missing/ambiguous capabilities,
   cycles, unsupported gates, colliding paths and unsafe paths fail before
   any write. Hand-built Intake and Build-cloud remain untouched.
2. Rendering the unchanged factory into an empty temporary directory matches
   every file in the M2 golden inventory byte for byte.
3. Each mapped config field changes its output as expected. For example, an
   added CI command adds exactly one step to `ci.yml`, and a changed required
   check changes the job name and the ruleset context. No other bytes change.
4. First sync adopts matching hand-built files. A second sync is a no-op.
   Check mode detects drift without changing files or bookkeeping.
5. Removing a supported module removes only its recorded, unmodified outputs.
   Modified owned files, conflicting unowned files, forged records, symlink
   escapes and write failures fail safely and preserve unrelated files.
6. Matching pinned versions succeed; mismatches and unsupported schemas fail
   before writes. A controlled version bump re-renders cleanly and is then
   idempotent. `just check` and required CI pass on pinned Rust 1.85.

**Owner-only adoption:** after the CLI is usable, manually build the first
stage0 release, publish its real tag, and approve the matching non-bootstrap
pin. Run golden comparison, sync and read-only doctor; review and human-merge
the ownership/pin changes. Only then claim self-hosting adoption. No
implementation agent may create that release or flip the live bootstrap flag.

## Implementation slices

| Slice | Deliverable | Prerequisite |
| --- | --- | --- |
| [A: tylerbutler/acme#28](https://github.com/tylerbutler/acme/issues/28) | Deterministic module selection, dependency/gate checks and render plan | M2 module catalog merged |
| [B: tylerbutler/acme#23](https://github.com/tylerbutler/acme/issues/23) | Local render command and exact golden fixtures | A merged |
| [C: tylerbutler/acme#27](https://github.com/tylerbutler/acme/issues/27) | Ownership record, safe sync, check mode and removal tests | B merged |
| [D: tylerbutler/acme#26](https://github.com/tylerbutler/acme/issues/26) | Version compatibility, upgrade tests and owner adoption guide | C and M2 repair slice merged |

M3's core exit needs slices A to C. Slice D waits until the first stage0
build exists; until then, bootstrap mode stays on and no pin is enforced.

Planning can proceed alongside M2. Rendering work can follow its catalog
without waiting for remote repairs; adoption requires the completed M2 doctor.

## Non-goals and approval

No init interview, template repository, new profiles, rendered agent modules,
automatic tool installation, remote sync/provisioning, release automation,
holdout, review, local workers or autonomy changes. Do not relax gates or
modify live agent settings to obtain a clean golden result.

Merge this spec before implementation. Each slice needs one issue, one small
draft PR and an owner-applied `ready-for-agent` label after its prerequisites
merge. Schema, module and renderer changes remain `human-merge`; no auto-merge.
