# Built-in factory presets

**Status: owner-approved planning direction, not implementation approval.**

## Goal

Expand acme beyond Rust CLI projects with built-in factory presets. Support
Rust CLI, Rust library, TypeScript library, Gleam library and Astro website
projects. Keep the deterministic engine and human approval rules shared.

Rust CLI is the current supported preset, not the long-term product boundary.
This plan does not make the other presets available or promise all five in the
next release. Existing Rust-only command behavior remains unchanged until
separately approved implementation lands.

## Approaches and decision

Use built-in presets first. New presets ship with acme and its versioned
templates, checks and fixtures. This keeps compatibility and review within
the existing release boundary.

Versioned external packs would let users add project types without an acme
release, but need distribution, pinning, compatibility and trust rules.
Defer that public contract until concrete use cases show what it must expose.
Executable plugins are not part of this plan.

## Shared engine and preset boundaries

Keep these responsibilities shared:

- Approved config, schema and binary version validation.
- Deterministic output planning and compiled workflow input validation.
- Module dependencies, exact output ownership and safe render/sync behavior.
- Factory policy, agent stages and human approval requirements.
- Common GitHub assets and settings checks where their behavior is unchanged.

Each built-in preset supplies project prerequisites, language or framework
setup, CI assets and supported command validation. Doctor must check the
assets selected for that preset rather than assume Rust assets.

Project purpose and language/framework are distinct concerns. A library is
not a language, and Astro is not interchangeable with every TypeScript
project. Support explicit combinations, not an unrestricted cross-product.
Decide exact config fields and identifiers in the implementation contract;
this plan does not change `profile = "cli"` or `languages = ["rust"]`.

Reuse the current module catalog and ownership mechanisms where possible.
Do not introduce a public plugin API or a generic provider resolver merely
to support the first additional preset. Keep command validation explicit;
broader project support does not authorize arbitrary shell execution.

## Delivery sequence

Add a factory-preset expansion phase after M5 and before the deferred M6–M8
work. This changes the roadmap, not current workflow enablement or approvals.
Automatic dispatch remains paused.

| Slice | Target | What it proves |
| --- | --- | --- |
| Baseline | Rust CLI | Existing output and behavior stay compatible |
| 1 | Rust library | Project purpose can change without changing language |
| 2 | TypeScript library | Shared factory behavior works outside Cargo |
| 3 | Gleam library | Another toolchain fits the established boundary |
| 4 | Astro website | Framework-specific checks fit a non-library project |

Use the first two new presets to test the boundary before committing to a
general extension interface. TypeScript library is the required early
non-Rust example; do not call the architecture general based only on Rust
CLI and library fixtures.

Before each slice, merge a bounded implementation contract and obtain owner
approval for its issue. Each contract must select the supported project
layout, toolchain and package-manager versions, lockfile rules, CI commands,
owned paths and config compatibility behavior. It must state which existing
files and authored project inputs the preset requires.

## Acceptance scenarios

1. Each preset has a representative local project fixture and reviewed golden
   output. The same approved inputs produce the same bytes. Existing Rust
   CLI golden output remains unchanged unless a separate change is approved.
2. Missing prerequisites and unsupported combinations fail explicitly before
   writes. A non-Rust preset does not require Cargo or Rust setup merely
   because the shared engine was first built for Rust.
3. Each preset supports render, init, sync and read-only doctor consistently.
   A second sync is clean, and `sync --check` writes nothing. Tests use local
   fixtures and fake remote state; no live activation follows from success.
4. A supported preset change removes only recorded, unmodified managed
   outputs that are no longer needed. Conflicting or edited files block the
   change before writes. Authored source and unrelated files remain intact.
   Version and ownership compatibility must be explicit, not inferred.
5. Before claiming broader support, verify both Rust library and TypeScript
   library through the shared engine. Run `just check` and the focused
   checks defined by each slice's contract.

## Scope and non-goals

This is a planning update, not approval to implement all slices, create
remote sample repositories, or run paid tests. Current release and adoption
requirements still apply.

Do not add package installation, package publication, website deployment,
workflow activation or remote provisioning as an implicit effect of a preset.
Generated CI may use the dependency setup approved by its slice's contract.
Workspace and monorepo support need their own explicit scope; the new preset
names alone do not enable them.

Do not implement third-party pack loading, executable plugins, automatic
preset detection or arbitrary language/profile combinations in this phase.
Publishing and deployment automation remain separate from library and
website factory support.
