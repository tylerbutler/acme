# Dispatch declaration contract

Declarations use strict TOML and version `1`. Store one file per task under
`factory/dispatch/`. The coordinator reads declarations as data. It does not
execute their contents or fetch repository state.

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

## Exact-target handoff proof (#52)

**Result: exact targets work; the complete snapshot handoff is blocked.**
This is an offline proof for [spec 0010](../../docs/specs/0010-spec-dispatch-coordinator.md),
not coordinator activation. The proposed spec revision distinguishes approved
request binding from remote builder isolation. Existing assignment code still
requires the stronger guarantee and blocks. This proof does not implement the
revision or unblock #41.

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
handoff. The proposed spec revision accepts this context risk; it does not
change the proof result. This does not claim that every possible platform
interface is incapable of meeting the original contract.

### Interface for the claims slice

`src/assignment.rs` defines the bounded, fakeable interface:

- `AssignmentRequest::from_approved_snapshot(contract, main_revision)` validates
  the declaration and a full revision, derives the target and approved task,
  and records the complete contract hash. Fields are private and read-only.
  Construction is **not authorization**. The caller must obtain these inputs
  from verified protected main and check approval, eligibility and the claim.
- `AssignmentTransport::capabilities()` returns separate exact-target and
  immutable-task results: `Supported`, `Unsupported`, or `Unknown`.
  `AssignmentCapabilities::gh_aw_0_93_5()` reports exact-target support only
  for the restricted numeric/triggering, single-repository path tested here.
  It reports immutable-task authority as `Unsupported`.
- `handoff(transport, request)` blocks before `assign` unless both capabilities
  are `Supported`. It returns the blocked capability and its status.
- `assign` returns `Accepted`, `NotAssigned`, or `Unknown`. Unknown outcomes
  pass through without retries. Future claims code must reconcile them.

There is no production GitHub assignment transport, CLI command, claim engine
or installed workflow in this proof. A fake transport's supported result does
not confer a capability on gh-aw or prove an actual assignment.

### Proposed owner decision and implementation gate

Spec 0010 proposes deterministic dispatch authorization and approved-snapshot
request binding, with explicit acceptance that mutable issue context may
influence the builder. The approved snapshot remains the scope authority;
human review must check the PR against it before merge. Instructions and hashes
do not establish remote isolation.

Human merge of that revision must precede an owner-approved implementation
follow-up. That follow-up must update the capability model, claims integration
and offline tests to enforce the revised requirements. Do not change
`immutable_task` to `Supported`; the proven isolation limitation remains.
Keep #41 and #59 blocked until the follow-up is accepted and human-merged.
A different API or provider is not an implicit fallback.

This docs-only revision does not change code, enroll tasks, change the manual
owner-label route, activate the coordinator or authorize live or paid
validation.
