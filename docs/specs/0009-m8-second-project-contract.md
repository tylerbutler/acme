# M8: second-project acceptance

**Status: proposed acceptance plan, not permission to create a project.**

## Goal and basis

Initialize a second supported project from the thin template without manual
edits to generated files. Follow the
[roadmap](0001-bootstrap.md#next-milestones) and
[factory boundaries](../adr/0001-factory-boundaries.md).
Use this acceptance plan to guide M4-M7 rather than waiting to define success
after their implementations.

## Test subject and prerequisites

The first qualification target is a small Rust CLI chosen by the owner.
Select its repository owner/name/visibility and any private scenario repository
only with separate approval. Install a real pinned acme release; do not use
HEAD, an unreleased source checkout or a hidden local patch.

M4 supplies the template/interview/init path. M5 supplies rendered Intake and
Build-cloud. The complete CLI profile also needs M6 review/holdout and M7
release provenance; do not describe an earlier core-only smoke test as full
qualification. Optional local workers and ops need separate consent and their
own evidence if selected, not forced activation on every project.

## Acceptance journey

1. **Create and brief:** start from the published pinned template and a minimal
   supported project. Record versions, approved constraints and the brief.
   Missing prerequisites fail clearly; there are no undocumented manual fixes.
2. **Interview and approve:** use the shipped skill to propose config. A human
   approves scope, paths, enabled modules, model policy, credentials by name
   and cost limits. No secret values enter the brief, spec or public evidence.
3. **Generate and review:** init produces one reviewable change set. Required
   checks pass, the owner merges the init PR, and approved provisioning matches
   the config. Keep activation separate; a merged config alone is not consent
   to paid runs or new private resources.
4. **Prove the build path:** with explicit staged/live permission, open a small
   spec-backed issue and have the owner apply its ready label. Record Intake,
   cloud assignment, one draft PR, required gates and human merge. Reject an
   equivalent non-owner promotion attempt without dispatching a builder.
5. **Prove quality and release:** use separately approved controlled evidence
   for independent review and isolated holdout, then verify a release artifact's
   identity/provenance. Do not publish private cases or enable optional loops
   merely to fill in a checklist.
6. **Prove lifecycle:** a second sync/check has no diff. An approved supported
   config change affects only expected owned files; an explicit version-pin
   upgrade with the matching installed release re-renders cleanly. Edited
   owned files produce a conflict, not a forced overwrite.
7. **Confirm no hand fixes:** changes belong in approved config, durable
   authored assets or the acme implementation. Never repair generated output
   by hand or add undocumented setup commands. Fix upstream, publish a reviewed
   release if needed, and repeat the affected journey.

## Evidence and closure

Keep a versioned, non-sensitive acceptance record with config/template/CLI
versions, PR and run links, observed timing, gate outcomes and unresolved
exceptions. Public evidence must exclude secrets and real holdout content.
Record the M4 under-30-minute init target honestly; separate smoke success
from full profile qualification and do not conceal external waits.

Fixture tests may use synthetic repositories and cases. The real journey
needs owner permission for each repository, paid test, activation, private
provisioning and release action. This spec and its tracker authorize none.

## Dependencies and decisions

Full closure needs M4, M5, M6 and M7's release track. Until track issues exist,
use the M7 tracker as the coarse release prerequisite; replace that link with
the release child when scoped, not unrelated optional workers/ops tasks.
The core template smoke journey can start once M4/M5 are available. Choose the
project and approved profile/optional tracks before opening execution issues.
Resolve credential, model-identity or platform limits rather than bypassing
gates.

## Non-goals and approval

No new language/profile matrix, new product features, multi-repo application
builds, blanket activation, auto-merge or manual generated-file patches.
Keep the milestone tracker blocked, unassigned and without `ready-for-agent`.
Later execution issues must be small and separately approved; owner-operated
acceptance is not one unlimited builder task.
