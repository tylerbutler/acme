# Built-in TypeScript library preset

**Status: proposed contract for #114, not implementation approval.**

## Goal and basis

Let a single-package TypeScript library use the shared factory without Cargo,
Rust setup or manual replacement of generated CI. This is the second slice
in the [preset plan](0013-builtin-factory-presets.md), after the
[Rust library contract](0014-rust-library-preset.md).

Prove reuse across both language and project purpose before claiming broader
support. Preserve the existing config, version-pin, ownership, approval and
[M5 compiled-input boundaries](0006-m5-agent-modules-contract.md).

## Approaches and proposed decision

Reuse explicit `profile`/`languages` combinations and the shared engine from
the Rust library slice. A separate TypeScript pipeline would be quick to
start but would duplicate init, sync and safety checks. A general plugin or
provider interface adds contracts that these two examples cannot yet justify.

Use pnpm, as selected by the owner, and Biome for formatting and linting.
The proposed remaining choices are Node 24 LTS, ESM-only output, TypeScript's
compiler for build and type checks, and Node's built-in test runner. This
avoids a bundler, a second formatter and a separate test framework.

## Configuration and versions

```toml
schema_version = 0
profile = "library"
languages = ["typescript"]
```

These fields belong to a complete approved config. Keep schema version 0 and
the Rust slice's compatibility rules. Add exactly this combination; do not
accept JavaScript, TSX, mixed-language arrays or arbitrary profiles by
inference. Rust CLI and Rust library behavior and golden bytes remain
unchanged. Require CI, human merge and the same new-preset policy as the Rust
library slice. Init still requires agent modules and activation flags off.

Use these concrete baseline versions for this first contract and its fixture:

| Tool | Version | Authored source |
| --- | --- | --- |
| Node.js | `24.11.0` | `.node-version`, containing that version and an optional final newline |
| pnpm | `10.20.0` | `package.json` field `packageManager = "pnpm@10.20.0"` |
| TypeScript | `5.9.3` | Exact `devDependencies.typescript` |
| Biome | `2.3.4` | Exact `devDependencies["@biomejs/biome"]` |
| Node type definitions | `24.10.0` | Exact `devDependencies["@types/node"]` |

These are proposed fixed baselines, not claims that they are the latest
releases. The published versions are available from the
[Node distribution](https://nodejs.org/dist/v24.11.0/) and npm registry:
[pnpm](https://registry.npmjs.org/pnpm/10.20.0),
[TypeScript](https://registry.npmjs.org/typescript/5.9.3),
[Biome](https://registry.npmjs.org/@biomejs/biome/2.3.4) and
[Node types](https://registry.npmjs.org/@types/node/24.10.0).
Implementation must execute the fixture to verify compatibility; release
existence alone is not that proof. A baseline change needs a reviewed contract
and golden update, not resolution of `latest` during rendering.

Do not add another version source to factory config. Reject mismatched pins,
version ranges for these development tools, alternate Node version files and
project package-manager configuration that changes the baseline behavior.
The acme binary pin remains separate and must match the running binary.

## Supported authored project

Support one package at the repository root:

| Authored input | Required contract |
| --- | --- |
| `package.json` | Valid JSON object; nonempty name and version; `"type": "module"`; the pins and scripts below |
| `pnpm-lock.yaml` | pnpm 10 lockfile authored for version control, `lockfileVersion: "9.0"`, with exactly root importer `.` |
| `.node-version` | Exact Node pin above |
| `tsconfig.json` | Standalone JSON compiler config described below |
| `biome.json` | Standalone JSON config with formatter and linter enabled |
| `src/index.ts` | Existing nonempty TypeScript library entry point |
| `tests/*.test.js` | At least one nonempty regular test file; the fixture uses ESM, `node:test` and built exports |

Require TypeScript options `target = "ES2022"`, `module = "NodeNext"`,
`moduleResolution = "NodeNext"`, `strict = true`, `declaration = true`,
`rootDir = "src"` and `outDir = "dist"`, with `include = ["src/**/*.ts"]`.
These are the only compiler options in the first baseline. The root config
contains only `compilerOptions` and `include`, plus an optional `$schema`.
Reject extra options, `extends`, project references and alternate includes.
This excludes emit-only declarations, JavaScript/JSX input and incremental
builds without requiring effective-config analysis.

Require this bounded Biome config, with only the optional `$schema` omitted
or present as shown:

```json
{
  "$schema": "https://biomejs.dev/schemas/2.3.4/schema.json",
  "files": {
    "includes": ["src/**/*.ts", "tests/**/*.js", "*.json"]
  },
  "formatter": { "enabled": true },
  "linter": {
    "enabled": true,
    "rules": { "recommended": true }
  }
}
```

Reject extra fields, configuration overrides and nested Biome configs in
the source/test tree. This selects the full supported source/test tree and
root JSON configs, not generated `dist/**` or `node_modules/**`. Broader
configuration choices need a later contract; acme does not normalize or
rewrite authored settings.

Only UTF-8 JSON is supported for these configs in this slice; comments,
trailing commas and JSONC need a separate contract. Reject workspaces,
`pnpm-workspace.yaml`, other package-manager lockfiles, `.npmrc`,
`.pnpmfile.cjs`, package-manager overrides, patch configuration, local
`file:`, `link:` and `workspace:` dependencies, and install lifecycle scripts.
Reject pre/post hooks for the required scripts. Normal extra source files,
tests, docs and registry dependencies are allowed. No registry credentials
or private registry setup are required by the baseline fixture.

Require a single root importer with manifest dependency specifiers matching
the lockfile importer's dependencies, devDependencies and optionalDependencies.
Reject malformed or inconsistent pins before writes. This is a structural
check, not a reimplementation of pnpm's resolver or a dependency integrity
audit. Frozen installation in CI verifies the executable lockfile contract.
Check current lockfile bytes without consulting Git index or HEAD state.
A Git checkout is not a local preflight requirement. The user must commit
the lockfile before CI; acme does not stage it or prove that it is committed.

Preflight checks test-file presence and regular-file safety, not JavaScript
syntax, imports or assertions. Do not add a JavaScript parser to prove test
intent. The reviewed fixture must test built exports with `node:test`;
executing its tests is part of acceptance, not local preset validation.

All project files remain authored, including package exports, dependency
choices and test assertions. Acme never edits package metadata, source,
tests, tool configs, lockfiles or version pins. Preserve the brief, specs and
authored instructions. Enabled M5 stages retain their authored prerequisites.
Package publication metadata is not evidence that publication is supported.

## CI commands and dependency setup

Require these exact authored package scripts:

```json
{
  "check": "biome check .",
  "typecheck": "tsc --noEmit -p tsconfig.json",
  "build": "tsc -p tsconfig.json",
  "test": "node --test"
}
```

Require `required_checks = ["CI"]` and this exact command order in approved
`modules.ci.commands`:

```text
pnpm run check
pnpm run typecheck
pnpm run build
pnpm run test
```

Require one nonempty step name per command. Reject replacements, additional
flags and shell fragments. Never execute these scripts during local acme
commands or infer trust from their names. The manifest checks reject hook
indirection, but project and dependency execution in CI is not sandboxed.

The generated workflow keeps existing CI events, read-only permissions,
timeout, concurrency and credential-free checkout. Its TypeScript setup:

1. Installs the exact Node version with SHA-pinned `actions/setup-node`.
2. Installs the exact pnpm version with SHA-pinned `pnpm/action-setup`, with
   automatic dependency installation disabled. Do not depend on Corepack.
3. Runs `pnpm install --frozen-lockfile --ignore-scripts`.
4. Runs the four approved commands above, in order.

Use a local composite setup action for steps 1 and 2. Keep the frozen install
visible in CI, before project checks. Start without dependency caching; a
cache is not needed to prove the preset boundary. Pin action SHAs with ratchet
comments and verify their declared inputs and Node 24 compatibility when
implementing. Do not invent action SHAs in this planning document.

Preflight rejects root lifecycle scripts structurally. It cannot determine
offline whether every dependency works without its own installation scripts.
Keep those scripts disabled in CI: installation or project-check failures
block CI and require an explicit project change, not automatic retry with
scripts enabled. Do not install Rust, invoke Cargo, publish a package
or upload registry credentials. The local binary does not install Node,
pnpm, project dependencies or Biome.

## Exact managed inventory

Replace only the CI module's setup asset selection:

| Module | TypeScript paths |
| --- | --- |
| `ci` | `.github/workflows/ci.yml`, `.github/actions/setup-node/action.yml` |
| `ruleset` | `.github/rulesets/main.json` |
| `labels` | `.github/labels.yml`, under the existing Intake dependency |
| `issue-forms` | `.github/ISSUE_TEMPLATE/idea.yml`, `.github/ISSUE_TEMPLATE/bug.yml`, `.github/ISSUE_TEMPLATE/spec-request.yml`, `.github/ISSUE_TEMPLATE/config.yml`, under the existing Intake dependency |
| `intake` | `.github/workflows/intake.md`, `.github/workflows/intake.lock.yml`, only when enabled and validated under M5 |
| `build_cloud` | `.github/workflows/build-cloud.md`, `.github/workflows/build-cloud.lock.yml`, `scripts/check-build-approval.py`, only when enabled and validated under M5 |

The TypeScript plan never owns or emits
`.github/actions/setup-rust/action.yml`. Common assets keep their existing
module ownership and behavior. Init's config copy and the format-1 ownership
record retain the Rust slice's bookkeeping rules.

M5's `python3` and `gh` runtime requirements remain separate from project
language. A TypeScript-only project needs no Rust files or installed Rust
tools to run the released acme binary. Building acme itself from source is
not part of that claim.

## Command behavior and safe preset changes

Use the Rust slice's shared selection, root-aware preflight and ownership
rules. Pure config planning does not read the filesystem. Render, init, sync
and compile validate TypeScript inputs from the checkout root before output
writes. Doctor reports the same prerequisite failures. Do not search for
Cargo inputs, run project commands, install packages or read secret values.

`render --output DIR` requires a new or empty destination. Init copies only
approved config and managed outputs and rejects an already configured
project. Sync uses validated plans; a second sync is clean, while
`sync --check` writes nothing and fails on output or metadata drift.
Enabled agent modules still require explicit compiled review inputs under
M5; no preset selection compiles or activates workflows implicitly.

Read-only doctor checks selected TypeScript assets against planned bytes,
the exact pins and scripts, and unchanged common GitHub settings. It does
not require Rust setup, compare against Rust fixture bytes or execute pnpm.
Missing remote access remains an explicit failure, not a passed check.
Do not extend remote repair or claim that a structural check proves that
the project's build and tests pass.

Support explicit transitions between TypeScript library and either supported
Rust preset. Users first supply and approve destination project inputs and
config. Acme changes only managed outputs; it does not convert or remove
authored source, manifests, configs or lockfiles. Leftover authored files for
the old language are allowed and remain outside the destination's checks.

Keep ownership record format 1. Validate prior entries against the finite
union of shipped preset inventories. On Rust-to-TypeScript sync, remove the
Rust setup file only if recorded under `ci` and unchanged; update the shared
CI workflow and add Node setup under normal conflict rules. Reverse that
selection for TypeScript-to-Rust. An unowned old setup action remains intact.
An edited owned old setup action or conflicting new action blocks all writes.

Keep old record compatibility and `acme_version` handling from M3; do not
infer permission from the version string. An older binary rejects records
containing the new Node setup path and must not be used for a downgrade.
Use a binary supporting both inventories for a preset transition.
Unknown paths, wrong module bindings, unsafe paths and symlinks remain errors.
Validate the complete change set before mutation, stage replacements and
write the record last. Retain the documented partial-I/O and
concurrent-writer limitations.

## Acceptance scenarios and local proof

1. A representative local ESM library fixture includes complete authored
   inputs, a real frozen lockfile and Node tests of built exports. Two renders
   match reviewed golden bytes. Init, sync, a clean second sync and read-only
   check mode work without Cargo inputs or Rust tools on the execution PATH.
   The frozen install and all four CI commands pass on the selected versions.
2. Negative fixtures cover unsupported combinations, missing or malformed
   files, mismatched pins/scripts, multiple importers, stale dependency
   specifiers, workspace/local dependencies, hooks and unsupported config.
   Unsafe paths and symlinks fail before writes. Capture checkout snapshots
   and verify failed render does not create its new destination.
3. Round-trip transitions cover both Rust presets and TypeScript. Remove
   only recorded obsolete setup assets; preserve authored and unrelated
   files. Edited owned assets, wrong ownership and unowned conflicts block
   changes. Existing Rust CLI and Rust library golden bytes remain unchanged.
4. Fake-remote doctor tests accept TypeScript golden output and report
   project, CI and asset drift without Rust-specific failures or local
   writes. Exercise existing M5 compilation/review validation with TypeScript
   config and common assets. No live activation, provisioning or paid run
   follows from these tests.
5. Before implementation approval, link the owner's factory-value review
   and resolve the Rust library prerequisites below. Run `just check`,
   focused cross-preset tests, the fixture's frozen install and four CI
   commands, actionlint on generated CI, and validation of the local setup
   action's declared inputs. Record unavailable
   checks explicitly. Both library presets must pass before broader support
   is claimed.

Fixture execution is a separate, explicit development check; it is not an
implicit effect of render, init, sync or doctor. It may fetch public
dependencies and does not authorize agent inference or remote repository
creation. Keep fixtures and reviewed golden output in the existing test tree.

## Prerequisites and implementation slices

Planning can proceed alongside #113. Before TypeScript implementation:

- Merge both contracts and finish the Rust library command/doctor integration
  and ownership-transition tests.
- Link the owner's [factory-value review](../bootstrap.md#factory-value-evidence),
  its decision and any required friction fixes.
- Review the Rust library boundary for Cargo assumptions. Resolve blockers in
  the bounded slices below, not through an unapproved generic extension API.

Open separately approved implementation issues after those prerequisites:

| Slice | Deliverable | Prerequisite |
| --- | --- | --- |
| A | TypeScript selection and authored-input/command validation; negative fixtures and no-Rust preflight tests | Merged contracts, Rust library completion and owner value review |
| B | Node/pnpm CI assets, all command/doctor integration, cross-preset ownership transitions and user docs | A merged |

Slice A remains internal until B proves the full public command contract.
The implementation issues must name the downstream behavior they enable and
link these specs. This proposed contract does not supply missing value-review
evidence or grant build approval.

## Non-goals

No monorepos, additional package managers, CommonJS or dual builds, bundlers,
browser/TSX support, package scaffolding, package publication, registry-secret
management, dependency lifecycle execution, public plugins, new remote
samples, workflow activation or changes to paused dispatch. No claim that
these two presets generalize to Gleam or Astro without their own contracts.
