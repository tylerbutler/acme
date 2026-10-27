# Dispatch declaration contract

Declarations use strict TOML and version `1`. Store one file per task under
`factory/dispatch/`. The pure planner reads declarations as data. It does not
execute their contents or fetch repository state. The read-only adapter below
supplies verified GitHub observations.

Each declaration binds a repository and issue to a spec path and SHA-256 hash,
the complete approved issue-body snapshot and its SHA-256 hash, a task kind, and
an ordered prerequisite list. Spec paths must be Markdown files under
`docs/specs/`. Only `implementation` tasks are eligible. `planning_tracker` and
`manual_operation` declarations are valid data but are never dispatchable.
Prerequisites are unique and ordered by increasing issue number. An
`owner_acceptance_required = true` prerequisite needs a separate acceptance
receipt from the repository owner after its implementation PR merges.

The strict TOML fields are:

| Field | Meaning |
| --- | --- |
| `version` | Contract version; currently `1`. |
| `repository`, `issue` | Exact `owner/repository` and positive issue number. |
| `spec.path`, `spec.sha256` | Approved spec path and content hash. |
| `issue_body.content`, `issue_body.sha256` | Approved body snapshot and content hash. |
| `kind` | `implementation`, `planning_tracker`, or `manual_operation`. |
| `prerequisites` | Ordered entries with a positive `issue` and optional boolean `owner_acceptance_required`. |

Hash content as UTF-8 after converting CRLF and lone CR line endings to LF.
Do not trim whitespace or normalize Unicode. Hash the resulting bytes with
SHA-256 and write 64 lowercase hexadecimal characters. The issue-body snapshot
must be included in the declaration; the current issue body and spec content
must still match their recorded hashes at planning time.

The spec PR merge is the approval. A trusted approval receipt must identify the
repository, issue, declaration fields, spec PR, authorized human merger, and
exact protected-main revision that contains the declaration. A label, comment,
open PR, issue close, or agent decision is not approval. Changed declarations,
specs, or issue bodies require a new reviewed declaration and merge.

The repository's existing `factory.toml` may omit coordinator settings. This
means automatic dispatch is off. Activation requires explicit positive limits
for active tasks, assignments per reconciliation, and maximum attempts. These
settings do not enable a workflow or authorize an assignment by themselves.

No task declarations are enrolled by this contract change.

## Approved-snapshot request binding (#52, #62)

**Result: exact targets and configured snapshot instructions are request-bound.
Remote builder context isolation remains unsupported.**
This is an offline proof for [spec 0010](../../docs/specs/0010-spec-dispatch-coordinator.md),
not coordinator activation or a production transport. The approved revision
requires exact-target and approved-snapshot request binding as separate
capabilities. Unknown or unsupported required capabilities block before claims
or assignment. `immutable_task` remains `Unsupported`; it is not the revised
required guarantee. Keep #41 and draft #59 blocked until this implementation
is accepted and human-merged. Then resume #59 rather than open another rollout PR.

### Reproduce without credentials

Use the existing Rust and Python checks plus Node.js, actionlint v1.7.12,
and the gh-aw v0.93.5 compiler. Obtain the public compiler and runtime archives
outside the repository. These downloads are preparation, not part of the test:

```sh
mkdir -p /tmp/acme-assignment-tools /tmp/acme-assignment-actions
curl -fsSL https://github.com/github/gh-aw/releases/download/v0.93.5/linux-amd64 \
  -o /tmp/acme-assignment-tools/gh-aw
chmod +x /tmp/acme-assignment-tools/gh-aw
curl -fsSL https://codeload.github.com/github/gh-aw-actions/tar.gz/de9d416cac8ccea1a2d582cb07c4b7db96946792 \
  | tar -xz -C /tmp/acme-assignment-actions --strip-components=1

cd /home/runner/work/acme/acme
ACME_GH_AW_BINARY=/tmp/acme-assignment-tools/gh-aw \
ACME_GH_AW_ACTIONS=/tmp/acme-assignment-actions just prove-assignment
cargo test --locked --test assignment
cargo test --locked --test claims
just check
```

The compiler download above is for Linux amd64. Other hosts need their matching
v0.93.5 release binary. The tested Linux binary has SHA-256
`a7b06bffaf7c24f713d10ec0a865514b764e62af639b13df953a480d74e8a8de`.
The runtime action SHA matches `.github/aw/actions-lock.json`. The proof checks
the compiler version and the hash of all runtime `.cjs` files, except test
files, before it loads the handler. A changed or missing runtime fails.

`tests/fixtures/assignment-handoff/workflow.txt` is not an installed workflow.
The test copies it to a temporary Git repository, compiles numeric, quoted
numeric, `triggering`, and wildcard variants with
`--strict --no-check-update --actionlint`, and removes the workspace afterward.
It pins the runtime with `--action-mode action --action-tag` and the SHA above.
It requires a completed compiler-integrated actionlint check. gh-aw's built-in
actionlint exclusions cover generated fields such as `concurrency.queue` and
`job.workflow_ref`; plain actionlint does not accept those fields yet.
The fixture also has an impossible event condition. Do not copy it into
`.github/workflows/` or run it.

The tests do not download tools, use credentials, start inference, or send
GitHub requests. Child processes receive a clean environment. The Node runner
loads the real pinned handler and its helpers, permits only its local modules
and required built-ins, and injects an in-memory GitHub transport. Unhandled API
routes fail the proof. No agent-output fixture is executed as code.
`just prove-assignment` fails if its tool paths are missing. Normal Python
discovery skips this opt-in proof when those paths are absent; Rust capability
tests still run in `just check`.

### Evidence and limits

| Check | Observed result |
| --- | --- |
| Configured numeric `target: 17` and `target: "17"` | Compile and assign only issue 17, even with no triggering issue context. |
| `target: triggering` | Assigns the trusted event's issue 17; no issue context means no assignment. |
| Substituted issue/PR aliases, target, instructions, contract hash, main revision and task text | Cannot change the configured target or the approved instructions sent by this handler. |
| Foreign issue or PR repository | Rejected before an assignment POST with the proof's single-repository configuration. |
| Wildcard negative control | Agent-supplied issue 999 reaches the fake POST. Wildcard is not suitable for this boundary. |
| Live issue body changes after approval | The handler still sends the same assignment POST. It does not check the approved body hash. |
| Comment text changes | Cannot replace configured instructions, but the handler does not establish comment isolation for the builder. |

The synthetic contract is under `tests/fixtures/`, not `factory/dispatch/`.
It enrolls no real issue. Repository, issue and approved body come from that
contract alone. The proof's contract hash is SHA-256 of the complete approved
declaration's UTF-8 bytes after the existing LF canonicalization. Main revision
comes from a separate trusted synthetic receipt value. Agent output supplies
neither input. Full-file hashing is conservative: formatting changes also
require a new binding. This is a local request binding, not a new declaration
schema or a GitHub enforcement field.

Rust tests compare `AssignmentRequest::gh_aw_config()` with the shared
`tests/fixtures/assignment-handoff/config.json` fixture. The pinned compiler
proof consumes that same configuration's target, repository and complete
instructions, then checks the real pinned handler's outgoing request.
Substituted agent fields cannot replace the snapshot, hash or revision.
The wildcard variant is only a negative control, not a supported configuration.

Pinned runtime source:

- [Configured instructions and target](https://github.com/github/gh-aw-actions/blob/de9d416cac8ccea1a2d582cb07c4b7db96946792/setup/js/assign_to_agent.cjs#L218-L236)
  are read from handler configuration. Instructions are not read from the
  message (`L356-L360`).
- [Target resolution](https://github.com/github/gh-aw-actions/blob/de9d416cac8ccea1a2d582cb07c4b7db96946792/setup/js/safe_output_helpers.cjs#L94-L245)
  uses the configured number or trusted triggering context, not message
  identifiers. Repository overrides still need the restricted allowlist.
- [Live issue lookup](https://github.com/github/gh-aw-actions/blob/de9d416cac8ccea1a2d582cb07c4b7db96946792/setup/js/assign_agent_helpers.cjs#L267-L291)
  reads the current issue. It does not validate the approved snapshot.
- [Assignment POST](https://github.com/github/gh-aw-actions/blob/de9d416cac8ccea1a2d582cb07c4b7db96946792/setup/js/assign_agent_helpers.cjs#L413-L431)
  sends issue identity plus `agent_assignment` options. There is no typed
  approved-contract hash, approved-main revision, or snapshot-only scope field.

`custom-instructions` does support sending the complete snapshot and receipt
values. The proof confirms those bytes reach `agent_assignment.custom_instructions`.
However, GitHub describes this option as
[additional instructions for Copilot](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/cloud-agent/use-cloud-agent-via-the-api#using-the-issues-api),
not a replacement for live issue context. A hash or revision inside prompt
text is not an enforced task or checkout binding. `base-branch` is a branch
option, not proof of an approved-main authorization check.

These offline tests prove request construction, not how the remote builder
uses issue bodies, later comments, or instructions. They do not prove that the
builder ignores changed scope, checks hashes, or uses the approved revision.
A final local body check alone cannot close later edits or comment races.
The original immutable-scope contract is therefore unsupported by this proven
handoff. The merged spec revision accepts this context risk. Instructions do not
enforce a checkout revision. Human review must compare each PR with its recorded
approved snapshot and spec before merge. Reject out-of-scope work or obtain a
new reviewed declaration. This does not claim that every possible platform
interface is incapable of meeting the original contract.

### Interface for shared claims and slice C

`src/assignment.rs` defines the bounded, fakeable interface:

- `AssignmentRequest::from_approved_snapshot(contract, main_revision)` validates
  the declaration and a full revision, derives the target and approved task,
  and records the complete contract hash. Fields are private and read-only.
  Construction is **not authorization**. The caller must obtain these inputs
  from verified protected main and check approval, eligibility and the claim.
  The complete contract uses the same LF canonicalization as its hash.
  Issue numbers must fit the exact JavaScript integer range.
- `AssignmentRequest::gh_aw_config()` derives a numeric target, one repository,
  `max: 1`, the restricted Copilot agent, and complete configured instructions.
  Its fields are private and serializable, not agent-provided overrides.
  Supply this configuration as data to the trusted handler. Do not interpolate
  contract text into executable workflow source. The offline compilation fixture
  is not production wiring.
- `AssignmentTransport::capabilities()` returns separate exact-target and
  approved-snapshot request-binding results: `Supported`, `Unsupported`, or
  `Unknown`. Missing capabilities default to `Unknown`.
  `AssignmentCapabilities::gh_aw_0_93_5()` reports exact-target support only
  for the restricted single-repository path tested here. Snapshot support
  requires the complete trusted configuration, not an agent message.
  The original `immutable_task` result remains `Unsupported` for this platform.
- `handoff(transport, request)` blocks before `assign` unless both capabilities
  required by the revised contract are `Supported`. It returns the blocked
  capability and its status. A transport must use the bound configuration;
  declaring support does not prove that it does so.
- `assign` returns `Accepted`, `NotAssigned`, or `Unknown`. Unknown outcomes
  pass through without retries. Shared claims reconcile them against trusted
  actual-state evidence before any bounded recovery attempt.

Shared claims preserve repository-wide serialization across manual and automatic
routes, external receipt provenance, duplicate checks, capacity limits, and
bounded recovery. Receipt version `2` records the complete approved contract,
target, contract hash, main revision, workflow run and attempt number.
Journal validation rejects a substituted snapshot or inconsistent hash/target.
Old receipts without the snapshot fail closed; do not convert uncertain attempts
into new claims without verified recovery evidence.

Before a durable claim and immediately before assignment, authorization checks
the request against fresh protected-main declarations and approval evidence.
The final read must retain the same approval receipt. Body/spec read failures,
changed hashes, disabled applicable gates and unavailable capacity block the call.
A failed final check keeps the attempt and records `NotAssigned`. Later
reconciliation checks live hashes even when an existing assignment or PR is found.
Verified terminal completion can still close an attempt and release capacity
after scope drift. This bookkeeping cannot authorize another assignment.

An edit after the final read can still reach the builder. Rust tests simulate an
edit in that interval: the outgoing request retains the approved snapshot, and
later reconciliation blocks on the changed hash. The pinned handler proof covers
body and comment changes without claiming atomic checks or remote isolation.

There is no production GitHub assignment transport, CLI command or installed
coordinator workflow in this change. A fake transport's supported result does
not confer a capability on gh-aw or prove an actual assignment. Slice C must
connect trusted state reads, durable storage and this bounded configuration to
the pinned handler, preserving the shared lock through the assignment call.

This implementation enrolls no tasks, changes no installed workflow or manual
owner-label approval policy, and authorizes no live or paid validation.
Workflow wiring, migration enrollment, rollout limits and activation require
their separate owner approvals. A different API or provider is not an implicit
fallback. Human review and merge remain required.

## Read-only GitHub state adapter (#65)

`acme::github_state::GitHubStateReader` implements `claims::GitHubState`.
`GitHubStateReader::new(GhClient)` reads automatic declarations.
`GitHubStateReader::manual(GhClient)` also reads manual-only declarations.
Both use the same claims interface. Neither is wired to a CLI command or an
installed workflow.

`GhClient` uses authenticated `gh api` reads on `github.com`. REST requests use
GET. GraphQL requests contain read queries, not mutations. The fakeable
`GitHubClient` interface exposes only JSON reads, commit-pinned file reads and
GraphQL queries. It has no assignment, inference or write operation. Observation
does not execute checkout files, PR code, artifacts, issue text or receipt data.
Tests replace this API boundary, not the observation logic.

### Supported evidence

| Input | Evidence required by the adapter |
| --- | --- |
| Authentication and identity | Successful `/user` and repository metadata reads. The supported approval policy is a user-owned repository, with its human owner as the sole approver. Organization approval policies remain unsupported. |
| Protected main | The default branch must be `main`, and its branch metadata must report protection. Its full commit SHA must match the request. The adapter checks main again after observation. |
| Files | Config, declarations, specs and acceptance receipts come from the pinned main revision, not the local checkout. A truncated Git tree blocks. Dispatch files must be regular files. |
| Owner approval | File history, associated PRs, complete PR file lists, owner merger metadata and merge ancestry must identify one approved file version. The bytes at that merge must match the current file. Config needs this proof too. Direct pushes and ambiguous approvals block. |
| Current scope | Fresh issue state/body and pinned spec bytes become typed observations. The planner and claims layer check their hashes against the reviewed contract. Read failures cannot become empty content. |
| Dependencies | Paginated native `blocked_by` references must stay in this repository. A closed issue or successful agent task alone is not accepted implementation. |
| Accepted implementation | An immutable issue `ClosedEvent.closer` must identify an owner-merged PR on main. That merge must follow both the approved declaration and spec revisions. Current editable PR links cannot establish acceptance. Reopened issues and ambiguous completions do not satisfy prerequisites. |
| Manual acceptance | A separate owner-merged receipt must follow the implementation merge and bind its issue and merge commit. Receipt fields cannot supply the approver identity. |
| Owner label | Current `ready-for-agent` presence plus the complete ordered issue-event history must identify the owner as its latest applying actor. Bot or non-owner applications do not authorize the manual route. |
| Gates | Configured module enablement, staging, positive limits and credit cap; live global/assignment variables; the remote Build-cloud workflow state; and approved secret-reference names from secret metadata. Automatic dispatch also requires the coordinator variable. Secret values are never read. |
| Capacity | All pages of issues, PRs and both archived/non-archived repository agent tasks. Queued, running, idle and user-waiting tasks count. Open PRs count while waiting for review. A retained Copilot assignee is released only with positive owner-merged closure evidence; active task evidence still counts. |

The variables are `ACME_AGENT_WORKFLOWS_ENABLED`,
`ACME_CLOUD_ASSIGNMENT_ENABLED` and `ACME_COORDINATOR_ENABLED`. Only the exact
value `true` enables a gate. Missing or invalid values block. This change does
not create variables or change their values. Secret metadata proves only that
a configured name exists; it does not prove that the secret is valid or has
assignment permissions.

Repository agent tasks use GitHub's preview
[`GET /agents/repos/{owner}/{repo}/tasks`](https://docs.github.com/en/rest/agent-tasks/agent-tasks).
The client uses API version `2026-03-10`. This API needs a user-scoped token with
Agent tasks read access; installation tokens are not supported. Other reads
need repository content, issues, PRs, Actions variable/secret metadata and
workflow read access. An inaccessible endpoint blocks; no token value is read
or changed by this implementation.

Lists use explicit pages of 100. Counts, duplicate pages, missing pagination
metadata, GraphQL errors and repeated cursors are checked. A pagination bound
blocks instead of returning partial evidence. An active task without a trusted
issue binding, an open PR without closing-issue references, or a foreign
reference also blocks. The reader can conservatively overcount overlapping
assignment, task and PR evidence; it does not infer a free slot.

### Manual route and acceptance receipt data

Both routes require a complete owner-reviewed contract from protected main.
The manual route also requires the current owner label, but does not require
automatic enablement or the coordinator variable. Its snapshots always keep
automatic dispatch off; only claims' explicit manual route can use them.

Use the same declaration format under `factory/dispatch/manual/` for a
manual-only snapshot. Automatic observation excludes that directory. An old
spec merge or an owner label alone cannot manufacture a new contract from
caller-supplied fields. This change enrolls no tasks or migration declarations.
The installed legacy owner-label workflow is unchanged.

Supported manual acceptance data lives at
`factory/dispatch/acceptance/<issue>.json`:

```json
{
  "version": 1,
  "issue": 16,
  "merge_commit": "cccccccccccccccccccccccccccccccccccccccc"
}
```

The format rejects extra fields. The owner must merge the receipt separately,
after the named implementation merge. The adapter derives `accepted_by` and
the receipt ID from authenticated merge evidence. Reapproving a changed task
does not let its old implementation or receipt satisfy the new contract.
No acceptance file is added by this change.

### Remaining limits and verification

GitHub reads are not an atomic repository snapshot. Main changes detected during
observation block. Claims perform another observation immediately before
assignment. Changes after that read and later comments remain the approved
mutable-context risk; this adapter does not isolate the builder or enforce its
checkout revision.

The supported APIs do not prove that an old producer can never create more
work. The adapter therefore returns `resolutions = Evidence::Unknown`.
Missing list entries, elapsed time, terminal workflows and terminal task states
never become retry permission. Uncertain attempts remain blocked with
`RecoveryRequired`; no automatic recovery policy is invented here.

Run the credential-free adapter and claims tests, then the existing checks:

```sh
cargo test --locked --test github_state --test claims
just check
```

The tests cover API failures, pagination, both approval routes, immutable
completion, required receipts, changed scope and exact request binding through
fake reconciliation. No live assignment, paid inference, workflow change,
claim-storage implementation or activation is part of this adapter.
