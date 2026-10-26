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
