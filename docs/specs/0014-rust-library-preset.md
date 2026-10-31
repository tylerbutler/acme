# Built-in Rust library preset

**Status: proposed contract for #113, not implementation approval.**

## Goal and basis

Let a single-package Rust library use acme without replacing CLI-specific
checks or generated assets by hand. This is the first expansion slice in the
[preset plan](0013-builtin-factory-presets.md). The
[TypeScript library slice](0015-typescript-library-preset.md) must then test the
same boundary outside Cargo before acme claims broader preset support.

Keep the [M1 config contract](0002-m1-config-contract.md),
[M3 ownership rules](0004-m3-render-sync-contract.md),
[init approval boundary](0005-m4-init-template-contract.md) and
[M5 compiled-input contract](0006-m5-agent-modules-contract.md).
This contract extends their project-specific support, not their permissions.

## Approaches and proposed decision

Use the existing `profile` and `languages` fields to select a built-in
combination. This adds one explicit combination without duplicate selectors.
A new `preset` field would give each combination a direct name, but would also
require migration and rules for conflicting fields. External packs add
distribution and trust contracts that these first slices do not need.

Use a small internal selection mechanism shared by planning, init and doctor.
Keep module dependencies, rendering, sync and agent compilation shared. Do not
add a public provider API or an unrestricted language/profile cross-product.

## Configuration and compatibility

```toml
schema_version = 0
profile = "library"
languages = ["rust"]
```

These are fields within a complete approved factory config, not a complete
config example. Resolve exactly `cli`/`["rust"]` and `library`/`["rust"]` in
this slice. The next slice adds `library`/`["typescript"]`. Reject unknown
combinations, extra languages and duplicate languages before writes. Do not
detect the preset from source files.

Keep `schema_version = 0`; this is additive support for existing string
fields. Do not add a required field, rewrite approved config or narrow the
existing raw schema to the new presets. Keep unknown-field rejection and the
existing distinction between config parsing and supported command validation.
New-preset validation must agree across the public commands.

Existing CLI/Rust configs, accepted commands, bundled doctor fixtures and
golden output remain unchanged. In particular, do not apply the new library
command restrictions or mandatory project preflight to the legacy CLI path.
Existing non-bootstrap version-pin checks still apply. The selected binary
must support this combination; an older binary's rejection is not migration.

For the new preset, require L3, disabled automatic dispatch, enabled CI and
human merge, and disabled deferred modules. Preserve existing stage approval,
budget and model rules. Init still requires agent modules and activation flags
off. Subsequent explicit compile/render/sync may use supported M5 agent
modules; selecting a library never activates them.

## Supported authored project

Support one Cargo package at the repository root, with this bounded layout:

| Authored input | Required contract |
| --- | --- |
| `Cargo.toml` | Nonempty package name and literal version; `edition = "2024"`; no workspace or inherited fields |
| `src/lib.rs` | Existing nonempty regular Rust source file |
| `rust-toolchain.toml` | `channel = "1.85"`; components include `rustfmt` and `clippy`; no alternate toolchain file |
| `Cargo.lock` | Nonempty parseable lockfile with `version = 4`, authored for version control |

Use the existing Rust 1.85 CI setup. Require the exact channel spelling above
so the authored pin and generated CI cannot select different toolchains.
Additional targets and toolchain components are outside this first contract.
Do not upgrade acme's compiler as part of this slice.

Allow an omitted `[lib]`, or `[lib]` with an omitted path or
`path = "src/lib.rs"`. Support only the ordinary Rust library crate type:
omitted `crate-type`, `["lib"]` or `["rlib"]`, and no enabled `proc-macro`.
Reject binary targets, `src/main.rs`, discovered `src/bin` targets, custom
library paths, examples, benches, root build scripts and Cargo config
overrides. Integration tests in `tests/*.rs` are allowed. Dependency build
scripts are not a claim of sandboxed execution.

Reject `[workspace]`, `package.workspace`, workspace inheritance and local
path dependencies, including target-specific and development dependencies.
These need a separate layout contract. Do not use `cargo metadata` to discover
or repair the layout.

Preflight checks the lockfile's current bytes, not Git index or HEAD state.
It does not require the project to have a Git checkout. The user must commit
the lockfile before CI; acme does not stage it or prove that it is committed.

The user creates and reviews all project inputs. Acme never generates or edits
source, tests, Cargo manifests, lockfiles, toolchain pins, the brief, specs or
authored instructions. Existing M5 authored prerequisites still apply when
their modules are enabled.

## CI contract

Require `required_checks = ["CI"]` and these four commands in this order:

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
cargo doc --locked --no-deps --all-features
```

Require one nonempty step name per command. Reject other commands, shell
operators and additional flags for this new preset. Commands remain explicit
approved config; the CLI does not insert missing commands into it.
`cargo test` covers unit, integration and documentation tests. Do not skip
documentation tests or add package publication.

Retain the existing workflow events, read-only permissions, timeout,
concurrency, checkout and SHA-pinned Rust setup. Dependency fetching and
project execution happen in CI, not during local preset validation. This
contract does not make Cargo commands safe for untrusted projects.

## Exact managed inventory

Reuse the current module names and paths:

| Module | Paths | Selection |
| --- | --- | --- |
| `ci` | `.github/workflows/ci.yml`, `.github/actions/setup-rust/action.yml` | Required |
| `ruleset` | `.github/rulesets/main.json` | Required through human merge |
| `labels` | `.github/labels.yml` | Existing Intake dependency |
| `issue-forms` | `.github/ISSUE_TEMPLATE/idea.yml`, `.github/ISSUE_TEMPLATE/bug.yml`, `.github/ISSUE_TEMPLATE/spec-request.yml`, `.github/ISSUE_TEMPLATE/config.yml` | Existing Intake dependency |
| `intake` | `.github/workflows/intake.md`, `.github/workflows/intake.lock.yml` | Only when enabled and validated under M5 |
| `build_cloud` | `.github/workflows/build-cloud.md`, `.github/workflows/build-cloud.lock.yml`, `scripts/check-build-approval.py` | Only when enabled and validated under M5 |

No project file becomes a module-owned output. Init may copy the approved
config to `factory/factory.toml`. Init and sync write
`factory/managed-files.json` as existing bookkeeping, not a new module asset.
Rendered directories contain planned outputs, not config or ownership records.

## Command behavior and safe changes

Pure config planning resolves the combination without filesystem access.
Root-aware render, init, sync and compile validate the authored project before
creating outputs. Read-only doctor reports the same project failures.
The source root is the checkout/current repository root, not the render
destination or the parent of an alternate config file.

Preflight reads inputs through the existing regular-file and safe-path checks.
Reject missing, malformed, symlinked and unsupported inputs before mutation.
Do not run Cargo, install Rust, fetch dependencies or contact GitHub to validate
the project. Parsing a lockfile does not prove that Cargo can resolve it;
locked CI is the execution check.

- `render --output DIR` keeps new-or-empty destination behavior. Validate
  project inputs and any M5 review bytes before creating the destination.
- `init --config PATH` preserves approved config bytes and the configured-project
  rejection. Prepare only the supported local change set; direct later runs
  to sync. No remote action follows.
- `sync` and `sync --check` use the same validated plan. Check mode reports
  output or metadata drift without writes. An unchanged second sync is clean.
- `doctor` checks new-preset assets against their deterministic planned bytes,
  including configured CI commands, rather than acme's CLI-specific fixture.
  Keep common GitHub checks and existing CLI fixture behavior. Doctor performs
  no project execution or local repair. Remote reads still need authenticated
  `gh`; this slice does not extend `doctor --fix`.

Support an explicit config change between Rust CLI and Rust library after the
user supplies the destination's prerequisites. Switching back retains existing
CLI preflight behavior; it does not add a new CLI-target requirement.
The CLI never converts project source or updates the approved config itself.

Keep ownership record format 1 and existing `acme_version` handling. No preset
field is needed in the record: it records module/path/hash ownership, not
project identity. Validate old entries against the finite union of shipped
inventories, not only the selected destination. A path outside that union or
with the wrong module is still invalid.

Before any writes, verify recorded hashes and all destination conflicts.
Update shared CI paths only when unmodified. Remove only recorded, unmodified
outputs absent from the destination plan. Preserve authored and unrelated
files, including unowned obsolete-looking files. Missing owned outputs are
recreated; missing obsolete outputs need no deletion. Stage replacements and
write the record last. Existing partial-I/O and concurrent-writer limitations
remain; do not promise rollback.

## Acceptance scenarios and local proof

1. A local single-package library fixture with unit, integration and
   documentation tests produces reviewed golden output. Two renders match
   byte-for-byte; init, sync, a clean second sync and no-write check mode use
   the same inventory. Its four CI commands pass on Rust 1.85.
2. Table-driven negative cases reject unsupported config combinations,
   missing or malformed inputs, workspace/path dependency layouts, alternate
   targets, mismatched toolchain pins and unsupported commands before writes.
   Failed render leaves a new destination absent; failed init/sync preserves
   a captured checkout snapshot.
3. Rust CLI golden bytes and existing accepted behavior remain unchanged.
   CLI-to-library and library-to-CLI fixtures preserve authored files.
   Modified owned files and conflicting unowned files block all changes;
   exact-byte adoption, missing outputs and unsafe records retain M3 behavior.
4. With fake GitHub state, read-only doctor accepts the library golden output
   and identifies deliberate asset, project and CI-command drift. Enabled M5
   modules retain explicit compile/review validation and their exact ownership.
   No test invokes a live agent or changes remote settings.
5. Before implementation approval, the issue links the owner's
   [factory-value review](../bootstrap.md#factory-value-evidence), its outcome
   and any required friction fixes. Run `just check`, the focused preset
   tests, the four fixture CI commands and actionlint on the generated CI.
   Record unavailable checks; fixtures alone do not satisfy the value review.

Local fixtures belong under the existing test tree. Include complete authored
inputs and golden inventories, not a new remote repository or private holdout.
The builder chooses internal file names and focused test commands and records
them in the implementation PR. Missing toolchain availability is a blocked
execution check, not permission to change the contract.

## Implementation boundaries and approval

After this spec merges and the value-review prerequisite is met, split work
into separately approved issues:

| Slice | Deliverable | Prerequisite |
| --- | --- | --- |
| A | Internal preset selection and Rust library project/command validation; focused compatibility tests | Merged contract and owner value review |
| B | Root-aware command integration, selected CI assets and doctor expectations; safe transition fixtures and user docs | A merged |

Slice A is internal groundwork, not public library support. Slice B enables
the complete contract only after all command paths pass. Do not change
`AGENTS.md` to claim support earlier. TypeScript implementation depends on
the completed Rust library boundary, not just its parser.

## Non-goals

No workspaces, binary/library hybrid packages, procedural macros, arbitrary
command execution, project scaffolding, publication, public plugins, new
sample repositories, workflow activation or changes to paused dispatch.
This spec does not record an owner value-review decision or authorize a build,
live adoption, remote repair, paid test or merge.
