# Intake: classify specification gaps from evidence

**Status: proposed; not implementation or rollout approval.**

## Goal and basis

Stop Intake from adding `needs-spec` to every issue. Distinguish missing
specification from unfinished dependencies without granting build approval.
This is a scoped behavior change to the
[M5 contract](0006-m5-agent-modules-contract.md), not a change to Build-cloud.

## Approaches

- **Selected:** update the Intake prompt to inspect linked specification
  evidence through bounded, read-only GitHub tools. Reuse its existing
  label/comment outputs and leave existing-label cleanup to humans.
- **Not selected:** add a deterministic specification-check job. This adds
  workflow machinery and still cannot establish semantic scope coverage.

## Contract

- Check the issue's goal, acceptance scenarios and non-goals. Read linked
  specifications in this repository on its default branch. For explicit
  spec PR links, verify merge metadata and that the PR changes a linked spec.
  Issue prose, comments and a closed issue state are not merge evidence.
- Do not propose `needs-spec` when the required issue detail is present and
  verified specification evidence covers the scope. A pending implementation
  dependency alone is not a specification gap.
- Propose `needs-spec` when required detail or specification coverage is
  missing, a linked spec is still proposed, or evidence cannot be verified.
  The comment must distinguish a known gap from unavailable evidence and
  name the specific missing detail or failed check.
- Reuse a suitable existing merged spec; do not require a new spec for every
  implementation issue. Intake's coverage assessment is advisory, not owner
  approval or a replacement for Build-cloud's approval checks.
- Keep the existing output limits and allowed labels. Intake may report
  pending prerequisites in its comment, but it does not apply `blocked`,
  remove labels, assign work, or apply `ready-for-agent`.
- Add only the read-only tool access and permissions needed for linked
  repository contents and PR evidence. Preserve event filters, activation,
  staging, models, budgets and safe-output authority.
- Update the authored workflow and recompile with the pinned gh-aw compiler.
  Keep affected M5 source assets and golden fixtures consistent when present.
  Coordinate with #91 rather than silently accepting changed golden bytes.

## Acceptance scenarios

1. A complete issue linked to a covering default-branch spec receives no
   proposed `needs-spec`, including when an implementation prerequisite is
   unfinished. An explicit matching merged spec PR is accepted as evidence.
2. Missing issue detail, a missing or draft spec, a mismatched spec PR, and
   incomplete scope coverage produce `needs-spec` with a concrete reason.
3. API or permission failures produce an explicit unverified result, not a
   claim that the spec is absent or that a build is approved.
4. An issue that already has `needs-spec` keeps its labels unchanged unless
   an allowed additive output is needed. No fixture grants Intake label
   removal, `blocked`, readiness, assignment or merge authority.
5. The pinned compiler and required repository checks pass. Offline fixtures
   check prompt requirements and compiled permissions/output bounds without
   credentials or inference. These checks do not prove model compliance;
   any live behavioral evaluation needs separate owner approval.

## Non-goals and approval

No retrospective label cleanup, new event triggers, automatic dispatch,
Build-cloud changes, activation changes or paid tests. Do not edit generated
lock files by hand.

Merge this spec and obtain owner approval of the implementation issue before
coding. Review the intentional Intake behavior change separately from M5's
existing zero-diff adoption requirement. Publishing changed workflows or
running live acceptance requires separate owner approval.
