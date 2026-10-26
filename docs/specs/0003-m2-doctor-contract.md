# M2: deterministic modules and doctor

## Goal and basis

Describe the installed deterministic factory modules and detect and repair
their setting drift. Follow the [M0 roadmap](0001-bootstrap.md#next-milestones),
[M1 contracts](0002-m1-config-contract.md),
[brief](../../factory/brief.md) and
[factory boundaries](../adr/0001-factory-boundaries.md).
This spec does not authorize changes to the live factory.

## Module boundary

Start with the current Rust/CLI factory, not new language or profile presets.
Add module manifests and source assets for this exact output inventory:

| Module | Owned output paths |
| --- | --- |
| `ci` | `.github/workflows/ci.yml`, `.github/actions/setup-rust/action.yml` |
| `labels` | `.github/labels.yml` |
| `ruleset` | `.github/rulesets/main.json` |
| `issue-forms` | `.github/ISSUE_TEMPLATE/idea.yml`, `bug.yml`, `spec-request.yml`, `config.yml` |

Use the reviewed post-M1 files as the baseline, including any M1 schema check.
Declare exact output paths alongside the module implementation; do not infer
ownership from directory globs. Bundle the catalog and assets with the binary;
do not require the acme source checkout at runtime. Keep the M1 manifest
contract. CI maps to `modules.ci`; the enabled Intake stage needs labels and
forms; the enabled human merge stage needs the ruleset. These support modules
do not require new enablement flags in the factory fixture.

Preserve action SHAs, required checks, forms and label definitions. Modules
may need files supplied by the project, such as CI scripts; those files are
not owned just because a workflow references them. Intake, Build-cloud and
their compiled workflows remain hand-built until M5.

## Doctor contract

- `acme doctor` reads `factory/factory.toml` from the current repository.
  `--config PATH` selects another approved config. Validate it through M1
  before checking files or contacting GitHub. Do not execute config commands.
- Default execution is read-only, locally and remotely. Bind remote requests
  to `config.repository`; a conflicting checkout identity blocks repairs.
  Authentication, permission and API failures are errors, not clean reports.
- Check the applicable module assets, CI command/check contract, issue-form
  requirements and action pins. Check managed labels and the named main
  ruleset, including active enforcement, PR-only changes, zero solo approvals,
  the extra-unattributed-approval setting, required CI, resolved threads and
  no bypass. Check the installed repository merge settings and read-only
  Actions defaults against the documented bootstrap baseline.
- Compare meaningful remote fields, not IDs, timestamps or response order.
  Handle paginated responses. Duplicate managed ruleset names are an error;
  unrelated labels and rulesets are not drift and must remain untouched.
- Report stable check IDs, observed/expected differences and repair guidance.
  Drift or an inability to complete a required check exits unsuccessfully.
  A clean report means only the supported M2 checks passed.
- Secret metadata can establish presence, never token value, scope or current
  assignment capability. Do not trust `token_verified` as fresh verification.
  Report unimplemented path-author enforcement, model separation and other
  deferred gates explicitly; do not mark them verified.
- `acme doctor --fix` is an explicit mutation request. Build and validate a
  repair plan first, require authenticated repository-administration access,
  apply only supported managed-label, managed-ruleset and repository/Actions
  setting repairs, then re-read and check the result. Report partial failures
  and remaining drift; never claim a transaction or rollback that did not run.
- Local source drift needs a reviewed edit, or M3 rendering; `--fix` does not
  rewrite project files. It must not invoke the old bootstrap script, which
  disables live workflows. Never change secrets, workflow enablement, variable
  gates, staging, budgets, assignments or issue state.

Use a testable GitHub transport. Tests must not need credentials or mutate a
real repository. The temporary Python tool is a behavior reference, not the
implementation of the Rust command.

## Acceptance scenarios

1. All four modules load through M1 manifests. Their declared assets reproduce
   the reviewed file inventory, including CI's local composite action.
2. Doctor passes against a matching local tree and fake GitHub state. Each
   managed setting can be broken independently and produces a named failure.
   Missing auth, insufficient permissions, duplicate rulesets, pagination and
   API failures have focused coverage.
3. Default doctor performs no writes. Secret-presence output contains no
   secret values and makes no claim about credential scopes.
4. `--fix` repairs supported drift in a fake repository, preserves unrelated
   resources and is idempotent. A failed write or failed verification remains
   an explicit failure. Unsupported repairs remain manual.
5. `just check` and required CI pass on pinned Rust 1.85. Document implemented
   commands accurately; `init`, `render` and `sync` still fail explicitly.

**Owner-only exit check:** run read-only doctor on `tylerbutler/acme`. With
separate approval, alter one managed label in a controlled test, observe the
failure, repair it with `--fix`, and record the final clean result. Do not
perform live drift or repair tests in an implementation agent run or CI.

## Implementation slices

| Slice | Deliverable | Prerequisite |
| --- | --- | --- |
| [A: tylerbutler/acme#25](https://github.com/tylerbutler/acme/issues/25) | Four-module catalog, exact owned paths and baseline assets | M1 schema and manifest slices merged |
| [B: tylerbutler/acme#24](https://github.com/tylerbutler/acme/issues/24) | Read-only doctor, reports, CLI wiring and fake-API tests | A merged |
| [C: tylerbutler/acme#22](https://github.com/tylerbutler/acme/issues/22) | Explicit repair plans, bounded `--fix` and re-verification | B merged |

## Non-goals and approval

No renderer, sync, init, generic provider resolver, language presets, policy
gate implementation, agent-workflow generation, secret provisioning, paid
test runs, release automation or new repositories. Do not change operational
settings merely to make doctor pass.

Merge this spec before implementation. Every slice needs one small issue,
one draft PR and the owner's `ready-for-agent` label after its prerequisites
merge. All changes remain `human-merge`. A ready label authorizes code and
fake-state tests, not live repairs, tags, releases or settings changes.
