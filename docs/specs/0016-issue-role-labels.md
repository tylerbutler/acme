# Issue roles: tracking and task

**Status: proposed contract for #122, not implementation approval.**

## Goal and basis

Make coordination issues and individual work items distinct in GitHub issue
lists. Owners should not need to open a parent and read its body to learn that
it tracks several builds rather than defining one build.

The immediate examples are preset trackers #113/#114 and their task children
#116–#119. This change removes repeated explanations about which issues can
be considered for assignment. It does not automate build approval.

Keep the [factory approval boundary](0001-bootstrap.md),
[M3 ownership rules](0004-m3-render-sync-contract.md) and
[Intake classification contract](0012-intake-spec-classification.md).

## Approaches and decision

The owner selected two role labels, `tracking` and `task`, and backfill of
current open issues.

Labels support list scanning and saved filters without changing issue titles.
A `[tracking]` title prefix would be visible in plain-text lists but would not
provide a label filter. Using both creates two role indicators that can drift.
Do not introduce a role-specific title convention.

## Role contract

Add these labels to the existing managed label fixture:

| Label | Color | Description |
| --- | --- | --- |
| `tracking` | `5319E7` | Coordinates work; not an agent build task |
| `task` | `C5DEF5` | One work item; not build approval |

Use the names exactly as shown. Document these meanings in `AGENTS.md`:

- A tracking issue coordinates related issues, milestones or prerequisites.
  Link its child tasks and use a checklist when useful. Do not assign a
  builder or apply `ready-for-agent` to the tracker itself.
- A task defines one bounded work item. It can be a bug fix, implementation,
  documentation or planning task. It may still need a spec or have unresolved
  prerequisites. Human-only steps remain human-only even when labeled `task`.
- Each classified issue has exactly one role label. Neither label grants
  approval, confirms spec coverage or proves that dependencies are complete.
- An issue with neither role is unclassified, not implicitly assignable.
  Both labels indicate conflicting classification. An author or maintainer
  must resolve the conflict; do not infer a role from a title or silently
  choose one.
- Parent/sub-issue links do not set the role. A tracking issue can have no
  children yet; a task can exist without a parent.
- A role can change through an explicit author or maintainer decision.
  Preserve the issue's relevant links and history, remove the old role and
  apply the new one. Do not change readiness or assignment as a side effect.

Role and status are separate. Preserve `needs-spec`, `blocked`, `in-progress`,
`needs-human-review`, `bug`, `idea` and `ready-for-agent` semantics. A blocked
task is still a task; a tracker with a merged spec is still a tracker.
Approval of a tracker never transfers to its children.

The role convention is an authoring rule, not a new machine-enforced
assignment gate. Existing Build-cloud and direct owner assignment remain
unchanged. Do not claim that a `tracking` label mechanically prevents either
route from assigning work.

## Authoring and managed assets

Add `.github/ISSUE_TEMPLATE/tracking.yml` with:

- Name `Tracking issue` and static label `tracking`.
- A required goal, required completion checks, a child-issue checklist field,
  optional spec/prerequisite links and required non-goals.
- Clear guidance that the issue coordinates work and is not assigned to a
  builder. Child links may be empty when the tracker is first opened.

Add static label `task` to the existing bug, idea and spec-request forms.
Preserve their existing labels and required fields. Their descriptions should
direct multi-task proposals to the tracking form. Existing `[bug]`, `[idea]`
and `[spec]` title prefixes describe the request category, not its role;
retain them unchanged.

Authors who use the API, `gh` or the blank issue editor select the appropriate
role explicitly. Add concise examples and filters to contributor guidance:
`is:issue is:open label:tracking` and `is:issue is:open label:task`.

Keep the authored files and their bundled module assets consistent:

| Module | Affected owned paths |
| --- | --- |
| `labels` | `.github/labels.yml` |
| `issue-forms` | `.github/ISSUE_TEMPLATE/bug.yml`, `.github/ISSUE_TEMPLATE/idea.yml`, `.github/ISSUE_TEMPLATE/spec-request.yml`, new `.github/ISSUE_TEMPLATE/tracking.yml` |

Keep the existing module bindings and enablement rules. Register the new
form in the finite built-in inventory, including safe ownership validation.
Do not add a module, config field, schema version or ownership-record format.
This intentionally changes the label/form golden bytes and adds one form;
unrelated assets, including CI and agent workflows, stay unchanged.

Render and sync must reproduce the selected assets deterministically.
Preserve exact-byte adoption, preflight conflict checks and removal of only
recorded, unmodified module outputs. Include the tracking form in module
removal tests. Do not update recorded hashes by hand to bypass conflicts.
Changes to live managed files and metadata need a reviewed adoption path.

Use the existing label-fixture readers for doctor and supported provisioning.
Doctor should detect missing or drifted managed role labels through its
normal fake-remote-tested checks. Do not add a new remote writer. Publishing
a fixture does not install labels on GitHub; live label provisioning requires
separate owner approval.

Do not extend Intake's allowed labels, output limits, triggers, tools or
authority. Role labels come from forms or explicit author/maintainer actions,
not model inference. This contract does not add automated role correction
or change how Intake assesses specification evidence.

## Open-issue backfill

Implementation includes preparing a finite backfill plan for the owner's
review. List each open issue, its current labels, proposed role and reason.
Use current issue content, not title matching or the mere presence of children.
Classify known trackers and bounded tasks; flag ambiguous issues for a human
decision.

Apply the plan only after separate owner approval. Do not treat approval of
the code change as approval of bulk GitHub edits. Recheck that each issue is
still open and its classification inputs have not changed. Report conflicts
instead of overwriting a newer decision.

A reviewed list of `gh issue edit` commands is sufficient. Do not add a
bulk-classification service or require a new rollout tool.

Change only role labels. Preserve all other labels, titles, assignments,
issue state and approval state. Repeating the approved plan makes no further
changes. Leave closed history untouched. Do not reopen issues, create test
issues or dispatch workflows to test the convention.

The expected initial examples are `tracking` on #113/#114 and `task` on
#116–#119 and #122. Recheck these examples during rollout; this list is not
permission to mutate them during specification work.

## Acceptance scenarios

1. Reviewed fixtures render both role labels and the tracking form. Existing
   task forms retain their prior labels and fields and add only the intended
   role/guidance changes. Repeated renders are identical; unrelated golden
   output remains unchanged.
2. Guidance and form previews make tracking versus task visible without
   interpreting approval status. They cover unclassified/conflicting roles,
   blocked tasks, human-only work and explicit role changes without implying
   that a task label authorizes assignment.
3. Focused render, catalog, sync and fake-remote doctor tests cover the added
   inventory, missing/drifted role labels, safe adoption/conflicts and removal
   of only owned forms. Existing repository checks pass.
4. An owner-reviewed open-issue plan distinguishes the preset trackers from
   their child tasks. Applying the separately approved plan changes only
   role labels; repeating it is a no-op and closed issues remain unchanged.
   Record unapproved or unavailable live checks as pending.
5. Intake and Build-cloud sources, compiled locks, authority and approval
   behavior remain unchanged. No readiness label or assignment is produced
   merely by classifying an issue.

Run `just check`, including the focused catalog, render, sync and doctor
coverage. Use local fixtures and fake GitHub state for automated checks.
The backfill plan and any approved execution report belong in #122 or the
implementation PR, not a new tracking system. Offline checks do not prove
live scan usability; include the form preview and example issue-list filters
in human review.

## Scope and approval

Implement this as one bounded issue and one PR after this contract merges
and the owner approves #122. The builder chooses internal test names and
the exact tracking-form wording within this contract.

No title migration, closed-issue backfill, public issue-type API, automatic
role inference, new approval gate, automatic dispatch, workflow recompilation
or remote provisioning follows from writing this spec.
