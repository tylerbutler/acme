# PR review pilot implementation plan

> **For agentic workers:** After separate build approval, use
> `subagent-driven-development` or `executing-plans` for the implementation
> task. The owner makes the review decision. Unchecked steps are planned work.

**Goal:** Find out whether the owner spends less time deciding if an acme PR
meets its approved issue, without missing more acceptance criteria.

**Architecture:** One builder instruction and one paragraph in the PR body.
Use existing issues, PRs and CI results. Do not build a reporting service.

**Tech stack:** Markdown and GitHub. No binary changes or new dependencies.

**Spec:** [0017: From an approved issue to a PR the owner can assess](../../specs/0017-pr-review-handoff.md)

## Global constraints

Use already-approved work, the usual CI and human review. Do not run a task
twice, trigger new events for comparison, or change either assignment route.
Review this plan and merge the spec before requesting separate approval for
the template and instruction. This plan does not install either one.

## Review focus

Check that a reviewer can distinguish a direct result from inference, a
skipped check from a pass, and an earlier result from evidence for the
current change. Count rework even if the PR never merges. Missing comparison
data stays unknown, not zero.

## Prepare the handoff

- [ ] In a separately approved change, add a pilot-only instruction under
  the issue and PR flow in `AGENTS.md`: before editing, map the approved
  scenarios to checks; flag unavailable or approval-dependent checks; at
  handoff, fill in the PR paragraph for the current revision. The builder
  owns this work. Do not add a required issue field or a second checklist.
- [ ] Add `.github/pull_request_template.md` with this prompt:
  **For a pilot PR, put the decision you need from the owner first. For each
  acceptance scenario, state what the change demonstrates, point to the
  result that shows it, and say what remains unverified. If a check did not
  run, explain why and what that means for this change.**
- [ ] Try the wording against the recorded [compiler PR](https://github.com/tylerbutler/acme/pull/101)
  without rerunning it. A reviewer should read that ordinary CI passed but
  the pinned compiler was unavailable, not that the tools were compatible.
  Ask the same question for a stale result: if it no longer covers the
  current change, the claim must have fresh evidence or an explicit gap.
  This is a test of the wording, not a pilot observation.

## Observe normal work

- [ ] Select comparable ordinary direct-assignment PRs without the new
  handoff. If past records omit active minutes, leave them unknown; collect
  a baseline on upcoming normal work where feasible. Note the assignment
  route, task size and verification demands. Prefer the same route and
  review standard when comparing handoff formats.
- [ ] Identify pilot work in the existing handoff before the outcome is
  known. Use the prompt in its PR body; ordinary comparison PRs keep their
  usual format. When review ends or stalls, add one observation per PR to
  the PR or the
  [acceptance record](../../bootstrap.md#factory-value-evidence):

  `Issue/PR; route; pilot or baseline; active owner minutes; clarification
  rounds; missed criteria/rework; accepted, returned or unfinished.`

  Count active review and follow-up time across rounds, including summary
  corrections. One request for more evidence and its reply is one
  clarification round. A missed criterion is one the handoff omits or
  presents as met that review finds unmet. An explicit gap is not a miss.
  Keep waits separate. For unfinished work, record time so far and do not
  count it as an acceptance.
- [ ] Suggested first checkpoint: the next three normal pilot handoffs,
  or sooner if the prompt adds work. Link the observations in the existing
  record. List differences in scope or assignment route that limit the
  comparison. Do not start another build just to fill the sample.

## Make the decision

- [ ] The owner records **keep**, **revise** or **stop**, with a PR example.
  Keep the manual practice if comparable reviews take less owner effort
  without more missed criteria. Revise one concrete source of confusion if
  the evidence is mixed. Stop if writing or correcting summaries costs as
  much as they save. Without a usable baseline, report the result as
  inconclusive; do not claim savings.
- [ ] Leave the CLI unchanged if the manual practice is sufficient. Only
  propose a separate automation issue if multiple observations name the
  same costly step and show what the automation would remove. None of these
  outcomes restarts preset expansion or automatic dispatch.
