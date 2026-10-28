# Factory coordinator: spec-approved automatic dispatch

**Status: proposed dispatch-assurance revision, not an enabled workflow.**

This revision requires human merge before implementation. Existing assignment
code still blocks on unsupported immutable-task authority. This document does
not change that code or authorize coordinator activation.

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
  issues, their recorded task snapshots and their prerequisites. A Markdown
  link, open spec PR, label, comment or agent decision alone is not approval.
- Bind each declaration to this repository, issue number, a spec under
  `docs/specs/`, the spec content hash and an approved issue-body snapshot/hash.
  Define a strict typed format and canonicalization. Record the approving
  human merge and exact main revision in the dispatch receipt.
- Issue-body or spec changes found before assignment invalidate the previous
  authorization. Re-approve changed scope through a new reviewed declaration.
  Send the approved snapshot as the task instructions, not a replacement
  derived from live issue text. Labels/comments do not amend the snapshot or
  authorize new scope. They can still influence the builder as described below.
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

## Dispatch assurance and builder-context limit

The required guarantee covers dispatch authorization and request construction,
not isolation of the remote builder from mutable issue context.

Deterministic code must bind each assignment request to the exact authorized
repository and issue, the complete approved task snapshot, its contract hash
and the approved protected-main revision. Derive these values from verified
protected-main declarations and approval evidence, never from agent output.
Record them in trusted claim/attempt evidence. Recheck the live issue-body and
spec hashes immediately before assignment; a mismatch or failed read blocks
the call. Request construction alone does not establish authorization.

Pass the complete approved snapshot, contract hash and approving main revision
through trusted configured assignment instructions. Prove with the pinned
handler that substituted agent output cannot change the target or these
instructions. Hashes and revisions in instructions support traceability;
they do not enforce builder behavior or a checkout revision.

The pinned handoff can supply these instructions, but it does not prove that
the builder ignores the current issue or later comments. The final eligibility
read and assignment are not atomic. An issue edit after that read, or later
comments, may influence the builder. The owner accepts this residual risk
through human merge of this revision. Later context never authorizes scope
changes; a later reconciliation must block further assignment if hashes differ.

Human review must compare the resulting PR with the recorded approved snapshot
and spec before merge. Reject out-of-scope work or obtain a new reviewed
declaration before accepting changed scope. Human review is the final merge
control, not proof of context isolation or prevention of prompt injection.
Agent context cannot grant workflow permissions or approval authority.

The alternative is to retain the stronger isolation requirement and keep
dispatch blocked until a separately approved handoff proves it. This revision
instead accepts the documented context risk while retaining deterministic
dispatch controls and human merge. A different API or provider still needs
explicit approval.

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

The proposed [GitHub claim-store contract](0011-github-claim-store.md) defines
the concrete journal, authenticated provenance and non-expiring ownership
boundary for #64. It preserves the controls above. Its merge approves a design,
not an implemented store, installation or activation.

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
4. Fake transport and pinned-handler tests prove that the target and configured
   snapshot instructions match the authorized claim. Substituted agent output
   cannot change the repository, issue, snapshot, contract hash or approving
   main revision sent by the handler. Missing or unsupported required
   capabilities block before assignment.
5. Changed issue-body or spec hashes at the final eligibility check block the
   call. Offline cases include later body/comment changes and state clearly
   that the request remains bound but builder context is not isolated. A later
   hash mismatch blocks further assignment. Human review compares the PR with
   the recorded scope; tests do not claim to prove remote builder behavior.
6. Global/assignment/coordinator kill switches and exhausted capacity stop
   dispatch before inference or assignment. Reporting is deduplicated; CI and
   tests need no credentials, paid agent runs or real repository writes.
7. Code remains compatible with pinned Rust and gh-aw. Required local/CI checks
   pass; human merge remains necessary for every implementation and spec PR.

## Implementation slices

| Slice | Deliverable | Prerequisite |
| --- | --- | --- |
| [A: tylerbutler/acme#43](https://github.com/tylerbutler/acme/issues/43) | Typed approval/config contract, snapshots and pure eligibility planner | M1 schema and manifest slices merged |
| [B: tylerbutler/acme#42](https://github.com/tylerbutler/acme/issues/42) | Shared claims, reconciliation and exact-target assignment hand-off | A merged |
| [C: tylerbutler/acme#41](https://github.com/tylerbutler/acme/issues/41) | Gated coordinator workflow, manual-route integration, reporting and rollout guide | B merged |

These slices themselves use the existing owner-ready gate while bootstrapping;
this unimplemented coordinator cannot approve or start its own installation.

### Follow-up after this revision merges

The merged handoff proof and claims code retain their original
`immutable_task = Unsupported` result. Do not relabel that guarantee as
supported or bypass the gate. An owner-approved implementation follow-up must
replace the required isolation capability with explicit checks for exact
target binding and approved-snapshot request binding, update claims integration
and add the acceptance cases above. Unsupported or unknown required
capabilities must still block assignment.

Keep #41 and its draft #59 blocked until that follow-up is accepted and
human-merged. Completing a proof, closing an issue or merging this spec revision
alone does not establish a working production transport or satisfy slice B.

## Rollout and non-goals

Install with automatic dispatch off behind a dedicated false/missing-safe gate,
checked before any inference or assignment. Preview safe outputs are an
additional safeguard, not a substitute for the off gate; staged inference
still costs money. Do not equate workflow registration with live activation.

After code review and merge, separately approve concurrency/retry limits,
bounded migration enrollment, staged/live evidence and activation. Keep the
existing two live workflows and their settings unchanged during planning.
Once active, an approved spec/task declaration authorizes its bounded build
dispatch without a per-task label or per-run approval. PR merges remain human.
Merging this policy proposal alone authorizes no live test, paid run or
activation.

No auto-merge, open-ended task generation, hidden provider substitution,
automatic secret/provisioning/release actions or inferred approval from issue
text. Future milestone plans remain plans until their small slices are approved.
