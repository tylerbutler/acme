# M6: independent review and isolated holdout

**Status: proposed milestone plan; integration choices remain open.**

## Goal and basis

Stop a seeded bad PR that passes normal CI through an independent review or
private holdout gate. Follow the [roadmap](0001-bootstrap.md#next-milestones)
and [factory boundaries](../adr/0001-factory-boundaries.md). This plan does not
enable either module or create a private repository.

## Required outcomes

- Render optional review/holdout modules with typed config, doctor checks,
  declared capabilities, owned outputs and explicit required status names.
  Missing, failed, stale or unverified results must not count as a passing
  required gate. Disabling a module needs a reviewed config/ruleset change.
- Review uses Copilot with a model different from the builder's actual model.
  Define evidence for both identities; distinct aliases, engine names or
  `auto` are not proof. Unknown identity must block an independence claim.
  Tie findings and gate results to the exact PR head, invalidate stale results,
  and block unresolved findings classified as blocking by the approved policy.
- A human still merges. Agent review is not a substitute for human authority
  or a reason to change solo approvals from zero. Define how deterministic
  author/path restrictions become enforceable; do not claim prompts enforce
  `never-agent`.
- Keep real holdout scenarios in a separate private repository without builder
  access. Provisioning and credentials require a separate owner-approved plan.
  Fetch scenarios using a trusted path; execute untrusted PR artifacts without
  repository credentials or uncontrolled network access. Never run PR code in
  a privileged credential-bearing context.
- Publish only approved, bounded result metadata. No private cases, inputs,
  detailed outputs, tokens or scenario-bearing artifacts may leak through
  logs, reviews, caches or public artifacts. Define the trust boundary before
  selecting workflow events or dispatch APIs.
- Record the tested head/artifact identity, holdout version, approved threshold
  and verdict. Thresholds and retry limits must be set before evaluation;
  unknown provenance, unavailable scenarios or evaluation errors fail the
  required gate instead of becoming a green fallback.

## Acceptance scenarios

1. Synthetic public fixtures show a CI-green defective PR blocked by review
   and a controlled holdout failure. A corrected head passes the appropriate
   checks, while stale results cannot satisfy a newer head.
2. Same/unknown builder-reviewer models fail the independence check. A claimed
   different-model pass has verifiable evidence, not just different labels.
3. Missing credentials, private-repo access failure, evaluator failure,
   threshold failure and forged/stale results cannot produce a passing gate.
4. Isolation tests cover credential, log, artifact and network boundaries.
   Public automated tests use synthetic cases, never the real private suite.
5. Separately approved live evidence demonstrates the bad-PR stop and corrected
   pass without disclosing cases. Both gates remain off until that rollout is
   approved; this planning PR supplies no live-run permission.

## Dependencies and required decisions

Depend on M5's agent-module, credential and workflow adoption contract.
Before build slices, verify supported model-identity evidence and Copilot/API
behavior; approve the holdout trust model, runner/isolation design, repository
owner/visibility, credential scope, result-authentication channel, status names
and threshold policy. Decide ownership of the author/path policy gate.
Record unsupported capabilities explicitly and keep the affected gate blocked.

## Non-goals and approval

No case publication, automatic merge, paid/live tests, repository or secret
creation, local workers or release/ops activation in this planning change.
Do not expand the existing continuous-operation approval to these stages.

The milestone tracker is not a builder task. Keep it blocked, unassigned and
without `ready-for-agent`; research/design can precede upstream implementation.
Create bounded implementation issues only after decisions and prerequisites
are accepted. All operational rollout and private provisioning need separate
owner consent and all code changes need human merge.
