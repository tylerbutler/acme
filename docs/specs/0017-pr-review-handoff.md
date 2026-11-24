# From an approved issue to a PR the owner can assess

**Status:** Direction for the next iteration of acme. The pilot is planned,
not running. This spec does not approve implementation or claim time savings.

## Decision and basis

Use acme's own approved work to test one change: have the builder show the
owner which acceptance scenarios the PR demonstrates and which still need
a decision. The outcome is less work to accept or return a PR, not more
PRs opened, more checks run, or a higher merge rate.

[PR #101](https://github.com/tylerbutler/acme/pull/101) disclosed that the
installed compiler check had not run. Ordinary CI passed, but the first
adoption attempt exposed incompatibilities that
[PR #110](https://github.com/tylerbutler/acme/pull/110) fixed. The useful
distinction for review was: local checks passed; compatibility with the
pinned tools remained unknown. A better summary would not itself have found
or fixed the defect. The [owner's review](https://github.com/tylerbutler/acme/issues/113#issuecomment-6538120618)
also lacked comparable active-time data for direct assignment.

Start with a paragraph in the PR template and a builder instruction. Keep
ordinary direct assignment as the comparison. A new CLI check could detect
an empty field, but not whether a test demonstrates the claimed behavior.
Leave repository conventions to tools such as repopo and execution to
existing CI. Neither preset expansion nor another agent stage solves the
review problem under test.

## Pilot contract

1. **Before building:** Use the approved issue as written. For each acceptance
   scenario, identify a check, artifact or manual observation that would
   demonstrate it. Flag a decisive check that cannot run or needs owner
   approval before relying on it. Do not ask the owner to write a second spec.
2. **At handoff:** Fix routine failures within scope. Put unresolved owner
   decisions first in the draft PR.
   For each scenario, state the claim, the evidence and the gap. Link the
   specific run or artifact, or give the command, scope and observed result.
   Label inference as inference. A test's existence, a skipped check or a
   green aggregate CI result is not proof of an untested scenario.
3. **At review:** The owner uses the evidence to accept the change, request
   rework or defer a decision. A summary never approves a merge. Record
   active owner minutes, clarification rounds and missed criteria in the
   [existing acceptance record](../bootstrap.md#factory-value-evidence) or
   linked PR. Count time spent checking or correcting the summary. Keep waits
   separate. Include returned or unfinished PRs as well as merged ones.

## Acceptance scenarios

- A reviewer can trace each scenario in the approved issue to a specific
  result or an explicit gap on the PR under review.
- When ordinary CI passes but a decisive check cannot run, the handoff says
  what remains unknown, why it matters and what decision or check is needed.
  It does not report a pass or silently narrow the criterion.
- When evidence is stale or the PR changes the relevant behavior, the
  builder updates it or marks the claim unverified before handing it back.
- At the pilot review, the owner compares similar directly assigned work
  against pilot work, or records why comparison was unavailable. The owner
  decides whether to keep, revise or stop the practice; a completed template
  alone is not evidence of value.

## Scope and approvals

Follow the [pilot plan](../superpowers/plans/2026-11-24-pr-review-pilot.md).
Use normal acme work only, after the spec merges and the owner approves the
relevant issue. Keep existing build approvals, CI and human merge. No new
CLI, schema, renderer module, policy engine, dashboard or merge gate. Do not
activate automated review, start extra paid runs, change workflows or create
another repo for the pilot. If the manual practice is enough, stop there.
Only propose automation for a costly step that recurs in the observations.
