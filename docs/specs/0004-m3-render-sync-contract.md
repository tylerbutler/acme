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
- A clean second sync has no diff, including the ownership record. Sync does
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
   every file in the M2 golden inventory byte for byte. A changed config
   changes only the corresponding planned outputs.
3. First sync adopts matching hand-built files. A second sync is a no-op.
   Check mode detects drift without changing files or bookkeeping.
4. Removing a supported module removes only its recorded, unmodified outputs.
   Modified owned files, conflicting unowned files, forged records, symlink
   escapes and write failures fail safely and preserve unrelated files.
5. Matching pinned versions succeed; mismatches and unsupported schemas fail
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
| A | Deterministic module selection, dependency/gate checks and render plan | M2 module catalog merged |
| B | Local render command and exact golden fixtures | A merged |
| C | Ownership record, safe sync, check mode and removal tests | B merged |
| D | Version compatibility, upgrade tests and owner adoption guide | C and M2 repair slice merged |

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
