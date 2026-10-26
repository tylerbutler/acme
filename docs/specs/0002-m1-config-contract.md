# M1: typed configuration and module contracts

## Goal and basis

Turn the hand-written `factory/factory.toml` into a typed, testable contract.
M1 follows the [M0 roadmap](0001-bootstrap.md#next-milestones),
[project brief](../../factory/brief.md) and
[deterministic-engine decision](../adr/0001-factory-boundaries.md).
The existing Cargo scaffold is reused; this is not a CLI implementation.

## Contract

- Add reusable Rust `FactoryConfig` types and TOML loading for every field in
  the current fixture, including disabled-module stubs. Preserve
  `schema_version = 0`, existing names and values; do not rewrite the fixture
  to make validation pass.
- Reject malformed TOML, unknown fields, unsupported schema versions, wrong
  types and missing required fields with useful errors. Loading is local and
  read-only: no LLM, network, credential lookup or provisioning.
- Generate `schema/factory.schema.json` from the same Rust types. Declare its
  JSON Schema dialect and keep generation deterministic. Validate the raw
  parsed TOML as JSON, not a filtered or repaired configuration.
- Add `just generate-schema` and a read-only `just check-schema` that fails on
  stale output. Include the check in `just check` and required CI.
- Define typed TOML module manifests with required `name`, `needs`, `provides`
  and `gate` fields. Reject unknown fields and empty names, gates or capability
  entries; empty capability lists are valid. Start with metadata only:

```toml
# modules/ci/module.toml
name = "ci"
needs = []
provides = ["ci"]
gate = "required-status-check"
```

Module manifests describe static capabilities and gates, not project-specific
commands, credentials or enablement. Provider resolution and gate enforcement
come later; parsing a manifest does not prove its gate is implemented.

## Acceptance scenarios

1. The unchanged factory fixture loads as `FactoryConfig` and passes the
   generated schema, including the disabled stages.
2. Focused invalid fixtures exercise malformed TOML, unknown nested fields,
   unsupported versions, wrong types and missing required fields. For parsed
   TOML, Rust validation and schema validation agree on these cases.
3. Repeated schema generation has no diff. A stale schema makes
   `just check-schema` and required CI fail without rewriting files.
4. The CI manifest loads as `ModuleManifest`; malformed or invalid manifests
   fail explicitly. Tests cover an empty dependency list and invalid fields.
5. `just check` passes on pinned Rust 1.85. The binary still exits unsuccessfully
   for unimplemented commands.

## Implementation slices

| Slice | Deliverable | Prerequisite |
| --- | --- | --- |
| A | Config types, TOML loader and focused tests | This spec merged |
| B | Generated factory schema, validation and freshness checks | A merged |
| C | Manifest types, loader and CI metadata fixture | A merged |

## Non-goals and approval

No doctor, render, sync, init, templates, profiles, dependency resolver, release
or second repository. Do not change active factory settings, credentials,
budgets, rulesets or agent workflows. The only CI change is the schema check.
Do not claim risk-path enforcement exists.

Merge this spec before implementation. Each slice needs a separate small issue,
one draft PR and the owner's `ready-for-agent` label after its prerequisites
merge. All changes remain `human-merge`; no auto-merge. Crates must support the
pinned toolchain; M1 does not authorize a Rust upgrade.
