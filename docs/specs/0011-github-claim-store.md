# GitHub claim journal and non-expiring ownership

**Status: paused with [spec 0010](0010-spec-dispatch-coordinator.md).**
Do not install the journal, create the `acme-claims` branch or extend the
adapter. A revisit must first show why a simpler exclusion is not enough for a
single-owner repository with one active pull request.

**Earlier status: proposed implementation contract. No production store is installed.**

## Goal, basis and approval

Resolve the storage and exclusion choices for [#64](https://github.com/tylerbutler/acme/issues/64)
and its draft [#70](https://github.com/tylerbutler/acme/pull/70).
Preserve [spec 0010](0010-spec-dispatch-coordinator.md), the version-2 receipt
binding and the existing owner approval policy. Attestations authenticate the
existing trusted writer; they do not grant task approval.

Select one GitHub Git branch as the journal. Append through GraphQL
`createCommitOnBranch` with `expectedHeadOid`. Authenticate every entry with
a GitHub Actions artifact attestation. Acquire exclusive ownership by appending
an authenticated ownership record to that same journal.

Ownership has **no expiry and no takeover**. Only its original producer can
release it, after its operations have settled. Cancellation, process death,
an unknown assignment or an unresolved write can therefore block all dispatch.
This availability cost is deliberate. A lease, Actions concurrency group or
client-side token cannot replace the ownership protocol.

Human merge approves this design for later implementation. It does not complete
#64, approve the empty #70 for merge, install resources, enroll tasks, change
live workflow settings or authorize a paid test. Keep
[#41](https://github.com/tylerbutler/acme/issues/41) open for its remaining slices.

## Supported boundary

Support this public, user-owned `github.com` repository. Use GitHub Actions'
built-in job identity and `GITHUB_TOKEN`; add no stored secret, credential,
external queue or separately operated service. Use GitHub's artifact-attestation
integration, including its documented Sigstore signing infrastructure.
GitHub Enterprise Server and other providers are not implicit fallbacks.

The deterministic privileged producer must run from protected main in
`.github/workflows/dispatch-claims.yml`. Both manual and automatic routes must
eventually invoke this same main-ref workflow. Route inputs are hints, not
approval or trusted receipt fields. The producer must check the existing
owner-label or declaration approval evidence itself.

There must be one producer process per invocation. Give each process a fresh
non-secret session nonce. Never restore ownership from a run ID, journal entry
or nonce supplied by another process. A restarted process and a rerun are new
contenders, not the old owner. A nonce distinguishes producers; it does not
authenticate them or enforce exclusion.

Keep the producer, synchronous persistence helper and pinned assignment handler
inside one deterministic job. Do not execute PR code, downloaded build artifacts,
issue text or agent output there. Do not move assignment to a later job, queued
safe output or detached process that can outlive the owning producer.
Agent jobs remain read-only.

## Journal identity and owner setup

Use `refs/heads/acme-claims` in this repository. Before activation, the owner must
approve a strict version-1 JSON policy at `factory/dispatch/claim-store.json`.
It is installation data, not a task declaration or a change to the M1 TOML
schema. Unknown fields, missing values and unsupported versions must block.

| Policy value | Required meaning |
| --- | --- |
| `version` | Integer `1`. |
| `repository`, `repository_id`, `owner_id` | Exact repository name and immutable GitHub numeric identities. Check them against authenticated repository metadata. |
| `journal_ref`, `genesis_oid` | The exact ref above and the full owner-approved genesis commit OID. |
| `workflow_path` | Exactly `.github/workflows/dispatch-claims.yml`. |
| `max_entries`, `max_entry_bytes`, `max_bundle_bytes`, `max_api_requests` | Owner-approved positive integer read/write budgets. Exhaustion blocks; it never permits a partial load. |

Load the policy from the approved protected-main revision. Prove its owner merge
and unchanged content with the same approval standard as the read-only adapter.
Never learn the genesis anchor from the journal being verified, an issue,
workflow input or a mutable local checkout.

Owner setup must create the genesis and protect the branch before its first
operational append. Separate active rules must prohibit deletion and
non-fast-forward updates, and require linear history. These permanence rules
must have no bypass actor, including the writer. Keep existing main protections.
The producer checks the applicable effective rules and repository identity
before acquisition and before assignment. Missing or incomplete rule evidence
blocks.

The reviewed genesis contains only `journal/genesis.json`, with format version,
repository identities and journal ref. Its OID is the trust anchor; it need not
pretend to be an operational receipt. Its creation and policy merge are future
owner operations, not automatic provisioning.

The trust boundary includes GitHub's enforcement and the owner's continued
protection of this branch. An ancestry check alone cannot detect rollback to a
valid old prefix. Server-enforced permanence prevents that rollback. Known
protection changes, a missing ref or uncertain continuity require owner
investigation, not adoption of a shorter journal. This design does not protect
against an administrator who deliberately replaces the trust root.

## Entry format and complete loading

Every later commit has exactly one parent and adds exactly two regular files:

- `journal/entries/<sequence>/entry.json`
- `journal/entries/<sequence>/entry.sigstore.json`

`<sequence>` is a 20-digit, zero-padded positive decimal number. Sequence 1
follows genesis. No commit may replace a file, delete a file, change a mode,
add other paths or add a merge parent.

The strict entry envelope contains `version = 1`, the sequence, expected parent
OID, repository identities, operation ID, session nonce, recording workflow run
and one operation. Operation IDs are unique within the recording run/attempt
and producer. The four operation types are:

| Operation | Payload and state transition |
| --- | --- |
| `acquire` | Records a new owner while the folded journal is free. The acquisition entry digest identifies the ownership generation. |
| `receipt` | Contains one unchanged version-2 `Receipt`. Only the current owner can append it. |
| `arm_assignment` | Binds the current owner and generation to exactly one pending attempt and its complete request binding. |
| `release` | Records that this owner has no outstanding operation or usable assignment permit. Makes the journal free. |

All operations after acquisition identify its generation. Bind the envelope's
recording run to the existing `Receipt.recorded_by` for receipt operations.
Reject a second arm for the same attempt, even with a new operation ID.
Reject release while this session has a pending or unknown attempt, an
unsettled write or an open reserved call.
Preserve the complete approved snapshot, contract hash, exact issue/repository,
approved main revision, route and attempt identity. Do not downgrade receipt
version 2 or infer missing snapshot fields.

Serialize the envelope as UTF-8 JSON with a final LF. Sign and store those
exact bytes; verification must not parse and reserialize them before hashing.
Reject duplicate object keys, unknown fields, wrong types and unsupported
versions. Preserve the existing contract-content canonicalization inside
receipts; do not apply it to the envelope or bundle.

Resolve the ref once and walk its commit parents by immutable OID back to the
reviewed genesis. Verify all commit trees, entry pairs, contiguous sequences,
parent bindings, attestations and ownership transitions. Return receipts in
oldest-first order. Do not rely on API list order or a filtered history endpoint
to establish completeness.

Use authenticated Git object reads and commit-pinned blob reads. Check every
page, truncation indicator, object type, byte limit and response error. Reject
missing ancestors, missing bundles, repeated operations, invalid state
transitions or a chain that cannot reach genesis within the budget.
Do not silently skip an invalid entry, even if its JSON claims to be irrelevant.

Re-read the ref after loading. If it moved, restart from a fresh full snapshot
within the request budget or return a conflict. An append must still use the
verified head as its exact expected parent. GitHub API errors or incomplete
history cannot become an empty journal.

## Independently verified provenance

Use the supported `@actions/attest` SDK `attest` operation in a small,
deterministic Node helper. It signs one subject: the entry file and its SHA-256
digest. Use predicate type
`https://github.com/tylerbutler/acme/attestations/claim-entry/v1` and an empty
predicate object. Save the returned Sigstore bundle in the same Git commit as
the entry. Attestation API storage and run artifacts are not the journal.

The helper receives only the typed entry bytes and job identity. It cannot
choose task approval, substitute a target or accept caller-supplied provenance.
Pin the reviewed SDK and its dependencies in a lockfile; pin the verification
tool and any workflow Actions. Do not install dependencies dynamically from
issue text or rely on a floating release. This spec adds none of these files.

Verify each saved bundle against its entry bytes with `gh attestation verify`,
using the exact repository, signer workflow, signer digest, source ref
`refs/heads/main`, source digest, custom predicate type, hosted-runner policy
and JSON output. Use `--bundle` for the journal's stored bundle. Cryptographic
verification must check the signature, trusted certificate chain and witnessed
signing time. Never disable digest, identity or timestamp checks.

After verification, derive provenance from the verified certificate, not the
entry's claims or `statement.predicate`:

| Certificate evidence | Required check |
| --- | --- |
| OIDC issuer | `https://token.actions.githubusercontent.com`. |
| Source repository and owner identifiers | Equal the policy's immutable IDs and current authenticated repository metadata. Names alone are insufficient for offline bundle verification. |
| Source repository URI/ref/digest | This repository, `refs/heads/main` and the entry's full source revision. |
| Signer and build-config URI/digest | The selected main-ref workflow and its approved revision. Reject a different reusable signer or top-level caller. |
| Runner environment | GitHub-hosted, not self-hosted. |
| Run invocation URI | Exactly this repository's `/actions/runs/<run_id>/attempts/<run_attempt>` with positive numeric values. |

Cross-check the certified run and attempt through the Actions API, including
repository, workflow path, source revision and main branch. Verify that the
workflow and policy revisions came through owner-merged protected main.
Historical entries need their historical approved revisions, not today's SHA.
Missing run metadata or approval evidence blocks; do not replace it with a
receipt field. Run deletion can therefore stop future dispatch.

Normalize `ReceiptProvenance.writer` to the verified signer workflow URI.
Derive `repository`, `run` and `protected_main` only after all checks pass.
Use that same trusted workflow URI in `ReconcileContext.trusted_writer`.
Commit authors, GitHub's commit signature, comment authors and workflow names
are not substitutes for this proof.

The CLI documents that certificate fields and verified timestamps are the
independently authenticated fields; predicate metadata can be caller-controlled.
Fulcio documents the run-invocation extension, including the attempt number.
Reject an absent extension rather than trusting an asserted run ID.
An attestation authenticates its producer and bytes, not task eligibility.

## Atomic append and acknowledgement

Expose only the fixed journal branch and the next two entry files to the write
adapter. Submit one `createCommitOnBranch` mutation with the verified branch
identity, `expectedHeadOid` and both files. Never omit the expected head.
Do not use a read followed by an unconditional ref update.

Acknowledge only after a fresh journal read proves that the exact operation,
entry bytes and bundle were durably appended to the expected parent. Check the
mutation's returned commit too, when available. A stale-head error is a
conflict, not permission to append against a newer head without reloading and
validating it.

An error, timeout or lost response can mean the commit succeeded. Disable blind
mutation retries. The original process may perform bounded read-only
reconciliation for its exact operation ID, generation and bytes. A verified
matching append can establish success. Absence from one read does not prove
failure or authorize a second mutation. An unresolved pre-release append
poisons the session and leaves ownership held.

The mutation's `clientMutationId` is correlation data, not a documented
idempotency key. A bundle generated or uploaded without its Git commit is not
a durable journal append and cannot authorize assignment.

## Exclusion lifetime and assignment boundary

To acquire ownership, validate the entire journal and append `acquire` only
when its final state is free. The expected-head check arbitrates simultaneous
contenders. A loser returns a conflict or busy result; it cannot adopt the
winner's session, even if both executions share a run ID. A successful owner
must confirm its exact acquisition before returning `ClaimSession`.

While held, only that original producer can append receipts, arm an assignment
or release. An `acquire` from another generation, a release by another producer
or an unexplained journal mutation invalidates the session. Poison it and stop
new writes and assignment. The invalid history cannot grant another valid
owner. Verify current ownership before each write and at the assignment boundary.

There is no heartbeat, TTL, lease renewal, force unlock, cross-run resume or
automatic recovery. A cancellation handler, `Drop`, job completion and workflow
concurrency-slot release must not append `release`. A terminal Actions status
does not prove an old request or producer cannot create more work.
Do not implement an operator reset command in this slice.

The assignment sequence is:

1. Acquire and verify ownership; load the complete receipt journal.
2. Observe fresh GitHub state and authorize through the existing claims engine.
3. Durably append the version-2 `Pending` receipt.
4. Append `arm_assignment` with the exact pending attempt/request binding using
   expected-head comparison. Confirm it; this reserves a call but does not yet
   authorize the handler.
5. Re-observe and revalidate the same approval, scope, gates and capacity.
   Verify ownership and create a one-use permit only after this final check.
6. Consume that permit in the pinned handler, synchronously within this session.
   Disable HTTP/client/handler retries of the assignment mutation.
7. Durably record the exact accepted, not-assigned or unknown outcome.
8. Explicitly release only when all operations are settled and no permit or
   child process can create more work. Otherwise retain ownership and report
   the block.

A failed final eligibility check records `NotAssigned`, closes the reserved
call and invokes no assignment. It can release after that append is confirmed.
An accepted response can release only after its outcome receipt is confirmed
and the handler has returned with no outstanding request. The builder may
continue remotely; the existing claims and capacity checks still count that work.

**Git compare-and-swap is not server-side fencing of the Copilot API.**
Safety instead requires no ownership transfer while an old producer might
still act, and a one-use synchronous handler that cannot escape the session.
An in-flight assignment can complete after a timeout; retain its pending claim
and ownership and never retry it blindly. A local "run is active" read alone
does not establish this boundary.

An uncertain `release` response is different: all permits and possible
assignment calls must already be irreversibly closed before sending it.
The old session stays unusable. A new process can acquire only if the journal
proves that release committed. It must otherwise treat ownership as held.

Actions concurrency may reduce contention, but is not the lock. A new run,
cancellation or freed concurrency slot cannot transfer journal ownership.
No cancellation hook or cleanup step may bypass this rule.

## Required interface changes

The existing `lock`/`load`/`append` interfaces and process-local fake do not
establish this production contract. #64 must make the directly dependent
claims interfaces and tests consistent with these requirements:

- The session carries its authenticated acquisition generation, original
  producer identity, current verified head and poisoned/closed state.
- Add explicit session-bound assignment authorization and explicit settled
  release. A one-use permit must be non-cloneable and non-restorable from JSON.
  Bind it to the complete pending attempt, request and owner generation.
- The production transport cannot assign from `AssignmentRequest` alone.
  It must consume the permit while the original session remains held.
  Update `reconcile_inner`, `assign_and_record` and the handoff boundary so
  an unguarded production call is not an alternative path.
- Every error path prevents later assignment. Session destruction never
  releases remote ownership. A receipt append error cannot be hidden by
  successful cleanup.
- Keep authorization, request construction, persistence and transport distinct.
  Keep the existing version-2 receipt and recovery checks. A permit is not
  approval, and an attestation is not a new task-approval authority.

#66 must connect the production handler to this boundary, not just declare
that a lock is held. #68 must route both workflows through it before coordinator
activation. If the pinned handler cannot run within this lifetime without
independent retries or delayed side effects, report the capability unsupported
and keep installation blocked. Do not silently substitute another provider.

## Failure and recovery policy

| Failure | Required result |
| --- | --- |
| Busy journal or conflicting acquisition | No assignment; no adopted session or hidden queue. |
| Forged provenance, invalid history, missing protection or incomplete API data | Block before assignment; do not skip, truncate, repair or reinitialize the journal. |
| Unconfirmed acquisition | No usable session; no assignment. Reconcile only the original operation through reads. |
| Unresolved pending/arm/outcome append | Poison the session, preserve ownership and report the operation identity. |
| Unknown assignment, cancellation or lost producer | Preserve the claim and ownership. No new producer takes over, regardless of age or terminal workflow status. |
| Confirmed release with lost acknowledgement | Old producer stays closed; a new contender may act only after independently verifying the release. |
| Changed approval or disabled gate | No new assignment. Record the known no-call outcome and release only if all operations are settled. |

The accepted #65 reader returns `Evidence::Unknown` when it cannot prove the
previous producer stopped. Preserve that result. This version supplies no
additional producer-termination proof and no recovery write for an orphaned
owner. Owner intervention means investigate and propose a separately reviewed
recovery mechanism, not delete the branch, clear a nonce or expire a claim.
Unknown work can block the whole repository indefinitely.

## Permissions and installation

The future deterministic writer needs job-scoped `contents: write`,
`id-token: write`, `attestations: write` and `actions: read`, plus read access
for repository and rule metadata.

Keep its job-token client separate from the existing user-scoped state and
assignment clients. The #65 agent-task endpoint does not support installation
tokens; `GITHUB_TOKEN` cannot replace its user token. Use the existing approved
`GH_PAT` reference only for those bounded deterministic reads and the assignment
handler. Do not read local CLI credentials or copy the secret into receipts,
logs, artifacts or the signing helper.

`contents: write` is a repository permission, not a branch-scoped token.
The narrow write boundary is the reviewed deterministic adapter plus branch
rules; do not claim GitHub restricts this token to two files. No token reaches
an agent job or untrusted code. Do not require package publishing or
`artifact-metadata: write`; no registry storage record is created.

The owner must separately approve branch setup, positive budgets, pinned tools,
privileged job permissions and manual-route integration before installation.
Keep existing live Intake and Build-cloud settings and secret references
unchanged in this spec PR and #64. Automatic dispatch remains false/missing-safe.

## Implementation acceptance

Use the real production adapter with a fake GitHub server and fake attester/
verifier process boundary. Run without credentials, network writes, inference
or paid assignments. Preserve the existing claims tests and add focused cases:

| Case | Required evidence |
| --- | --- |
| Two manual/automatic producers read a free head | Exactly one expected-head acquisition succeeds; only its original session can arm and assign. |
| Same run/attempt, different producer nonces | Neither can adopt or release the other's ownership. A restarted process cannot resume it. |
| Forged receipt, caller predicate, workflow, source revision, repository ID or run attempt | The production verifier rejects it before yielding trusted provenance or assignment. Include a valid signature from the wrong workflow. |
| Missing ancestor/bundle/page, reordered list, truncated blob/tree, duplicate sequence or operation | Full load fails. Valid old-prefix rollback/deletion attempts are rejected by the modeled permanence rules; do not claim genesis hashing detects them alone. |
| Changed head or ownership before arm, poisoned session or reused permit | No further assignment; stale expected-head writes fail. Test the actual guarded transport path. |
| Cancellation, session destruction, job completion or old workflow terminal status | No release and no takeover. A newer run cannot assign even if the old handler remains alive. |
| Append commits then times out, or remains unresolved | Exact readback can confirm one append; otherwise no next mutation, release or assignment. |
| Assignment commits then times out or returns an unknown result | Exactly one mutation; retained pending/unknown claim and ownership; no retry or age expiry. |
| Release commits then times out | Old session cannot act again. Only verified release permits a new acquisition. |
| Scope/gate change at final revalidation | No assignment; close the reserved call. A settled, durably recorded no-call path may release. |
| Accepted assignment and known outcome | Outcome is durable before explicit release; another issue still observes the accepted work against capacity. |

Run focused store/claims/assignment tests and `just check`. The later #66/#68
handoff proof must exercise the real pinned gh-aw handler and both routes with
fake APIs, including retry and process-lifetime negative controls.
In-memory mutex tests or a helper unused by the assignment path are insufficient.

Before any live activation, separate owner-approved evidence must establish the
effective branch rules, atomic-head conflicts, certificate/run-attempt fields,
tool versions, durable readback and the actual handler lifetime. Fake tests
cannot prove these hosted-platform properties. Missing evidence keeps the
capability unknown and activation off. This docs-only proposal runs no such
proof and does not report the adapter implemented.

## Sources and guarantee limits

- [GitHub GraphQL Git reference](https://docs.github.com/en/graphql/reference/git)
  and [schema](https://github.com/github/docs/blob/main/src/graphql/data/fpt/schema.docs.graphql):
  branch commits, expected head and immutable Git objects.
- [Available ruleset rules](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/available-rules-for-rulesets):
  linear history, deletion and force-push controls, including bypass behavior.
- [GitHub attestation SDK](https://github.com/actions/toolkit/tree/main/packages/attest):
  supported signing API, stored bundle, built-in identity and public-repository
  signing infrastructure.
- [`gh attestation verify`](https://cli.github.com/manual/gh_attestation_verify):
  identity policy, verified certificate output, caller-controlled predicates
  and offline name-reuse limitations.
- [Fulcio certificate extensions](https://github.com/sigstore/fulcio/blob/main/docs/oid-info.md):
  immutable identities, signer/source digests and run-invocation URI.
- [Workflow cancellation](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-cancellation):
  asynchronous cancellation, not assignment fencing or automatic safe recovery.

The APIs and identity fields above are documented primitives. The ownership
state machine, guarded transport and no-takeover rule are requirements on our
implementation, not a GitHub assignment-lock service. Production integration
and owner-approved hosted evidence remain required.
