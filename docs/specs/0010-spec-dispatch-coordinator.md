# Factory coordinator: spec-approved automatic dispatch

**Status: proposed approval-model change, not an enabled workflow.**

## Goal and basis

Dispatch small, explicitly specced implementation tasks when their
prerequisites are satisfied, without a separate owner ready-label action for
each task. Every PR still requires human review and merge.

Follow the [factory boundaries](../adr/0001-factory-boundaries.md). This is an
explicit extension of the current owner-label approval path, not permission
to impersonate the owner or silently relax it. Implementation and activation
are separate approvals.

## What a merged spec authorizes

- A spec PR includes versioned task declarations under `factory/dispatch/`.
  Human merge to protected main approves only the listed implementation
  issues, their immutable task contracts and their prerequisites. A Markdown
  link, open spec PR, label, comment or agent decision alone is not approval.
- Bind each declaration to this repository, issue number, a spec under
  `docs/specs/`, the spec content hash and an approved issue-body snapshot/hash.
  Define a strict typed format and canonicalization. Record the approving
  human merge and exact main revision in the dispatch receipt.
- Later issue-body or spec changes invalidate the previous authorization.
  Re-approve changed scope through a new reviewed declaration; do not send
  mutable issue instructions as the approved task. Labels/comments do not
  amend the snapshot and must not become hidden scope changes.
- Only bounded implementation slices qualify. M4-M8 milestone trackers,
  design/research umbrellas, owner-operated tests, provisioning, releases and
  adoption actions are not dispatchable. New child issues need their own
  reviewed declarations; a parent spec cannot authorize an unbounded backlog.
- Existing specs were approved under per-issue gates. Enroll their tasks
  through one owner-reviewed migration declaration after this policy lands;
  do not retroactively infer automatic approval from an old merge.

## Deterministic eligibility and dependencies

Use deterministic code, not an LLM, to make authorization, dependency,
capacity and target decisions. An agent may summarize progress and exceptions,
but cannot invent approval, expand scope or choose an unauthorized target.
The acme binary still never calls an LLM.

Before every assignment, verify the current protected-main declaration,
approving human identity, unchanged contract, open issue, applicable gates and
absence of conflicting active work. Validate the complete dependency graph;
reject missing/ambiguous references, cycles and unsupported manual gates.

Prerequisites require accepted merged implementation, not merely a closed
issue, completed agent run or green CI. Native dependencies remain visible
shared state and cannot weaken the reviewed prerequisites. Missing or changed
dependency evidence requires reconciliation, not optimistic dispatch.
Owner-only milestone checks need an explicit approved acceptance receipt;
closing their issue or merging their code PR does not prove the live check.

Required workflow/assignment kill switches, approved secret references and
positive resource limits must all permit dispatch. Missing configuration,
unknown API state, authentication failure and unsupported capabilities block
the task with a useful reason.

## Workflow and assignment boundary

- Reconcile on trusted spec/task-declaration merges, implementation merges,
  relevant trusted build-status changes and owner-requested reconciliation.
  Read workflow code from protected main; never execute PR code or artifacts
  in a privileged context. Coalesce duplicate events and avoid polling loops,
  self-triggering comments and unnecessary paid inference.
- Use a deterministic planner plus a bounded assignment workflow; optional
  agent reporting can use restricted comment/label safe outputs. Prove the
  exact target-binding mechanism with pinned gh-aw before relying on it.
  No unvalidated agent output may reach the privileged assignment handler.
- Preserve the existing owner-label path as an explicit manual route.
  Automatic dispatch uses the reviewed declaration/receipt as its separate
  authorization; never apply `ready-for-agent` with an owner PAT to make a
  bot action look like human approval.
- Share claims and duplicate checks across both routes. Serialize claim
  changes and persist trusted claim/attempt evidence in GitHub, not a hidden
  local queue. Recheck eligibility immediately before the assignment call.
  One task means one assignment attempt and one intended draft PR.
- Require owner-approved positive limits for active work and assignments per
  reconciliation before activation. Count claimed/assigned work and open
  unmerged implementation PRs, including those waiting for human review.
  Independent tasks may run within that cap; dependents wait for human merge.
- Retries must not duplicate assignments or charges. A timeout/unknown API
  outcome or stale claim needs reconciliation of actual agent/PR state.
  Do not blindly expire a claim or retry an uncertain assignment. Terminal
  failure, cancellation and manual retry need an explicit bounded policy.
- Maintain one replaceable status report with approval evidence, claimed,
  active, blocked and next-eligible tasks, and required human actions. Derived
  `blocked`/`needs-spec` bookkeeping may follow verified state; labels are
  never the authority. Never dispatch a milestone tracker or invent child work.

## Human-only authority

The coordinator cannot approve or merge PRs, enqueue them for automatic merge,
relax required checks, approve privileged CI runs, change secrets, provision
repositories, publish releases or enable deferred modules. It reports those
human actions instead. A merged code PR releases eligible dependents only
when any separate manual acceptance gates also have approved evidence.

M5's assignment module and future M7 workers must consume this shared approval
and claim contract rather than become competing queue owners. Until this
policy is implemented and activated, all current per-issue gates remain.

## Acceptance scenarios

1. Human merge of a matching spec/declaration makes only its named small tasks
   eligible. No ready label is needed on the automatic path. Draft/absent
   specs, changed bodies, unauthorized approvals, planning trackers and
   owner-operated steps cannot dispatch.
2. Synthetic independent tasks run within the configured cap; a dependent
   waits for merged inputs and required manual receipts. Missing API evidence,
   closed-but-unmerged issues and stale declarations stay blocked.
3. Repeated/concurrent events, manual/automatic races, an existing Copilot
   assignment or linked PR, API timeout and stale claims do not cause duplicate
   dispatch. Test unknown outcomes and recovery explicitly.
4. Fake transport tests prove the privileged assignment target is exactly the
   authorized claimed issue, not an arbitrary number from agent output.
   Mutable issue comments cannot change the approved task or permissions.
5. Global/assignment/coordinator kill switches and exhausted capacity stop
   dispatch before inference or assignment. Reporting is deduplicated; CI and
   tests need no credentials, paid agent runs or real repository writes.
6. Code remains compatible with pinned Rust and gh-aw. Required local/CI checks
   pass; human merge remains necessary for every implementation and spec PR.

## Implementation slices

| Slice | Deliverable | Prerequisite |
| --- | --- | --- |
| A | Typed approval/config contract, snapshots and pure eligibility planner | M1 schema and manifest slices merged |
| B | Shared claims, reconciliation and exact-target assignment hand-off | A merged |
| C | Gated coordinator workflow, manual-route integration, reporting and rollout guide | B merged |

These slices themselves use the existing owner-ready gate while bootstrapping;
this unimplemented coordinator cannot approve or start its own installation.

## Rollout and non-goals

Install with automatic dispatch off behind a dedicated false/missing-safe gate,
checked before any inference or assignment. Preview safe outputs are an
additional safeguard, not a substitute for the off gate; staged inference
still costs money. Do not equate workflow registration with live activation.

After code review and merge, separately approve concurrency/retry limits,
bounded migration enrollment, staged/live evidence and activation. Keep the
existing two live workflows and their settings unchanged during planning.
No live test, paid run or activation is authorized by merging this spec alone.

No auto-merge, open-ended task generation, hidden provider substitution,
automatic secret/provisioning/release actions or inferred approval from issue
text. Future milestone plans remain plans until their small slices are approved.
