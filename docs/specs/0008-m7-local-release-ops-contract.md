# M7: local workers, provenance and operations

**Status: proposed milestone plan; external integrations are unverified.**

## Goal and basis

Support a bounded epic through independent worker PRs and the required gates,
then provide verifiable release artifacts and actionable operational feedback.
Follow the [roadmap](0001-bootstrap.md#next-milestones) and
[factory boundaries](../adr/0001-factory-boundaries.md). These are three
optional module tracks, not permission to enable them together.

## Required outcomes

| Track | Input and output | Required boundary |
| --- | --- | --- |
| `build-local` | Approved dependency graph to separate worker PRs | Only independent, owner-approved tasks; bounded worktrees and workers |
| `release` | Approved main/tag to versioned artifacts and provenance | Verify artifact identity before consumption or deployment |
| `ops` | Errors, flakes, drift and cost evidence to actionable issues | Read-only agents, bounded safe outputs, deduplicated ownership |

Use typed config, manifests, doctor checks, pinned tools and M3 ownership.
Each track may be implemented separately after its own design review.

- Issues and native dependencies remain shared state, not a private scheduler
  queue. Each worker issue needs owner build approval; an epic or bot comment
  cannot apply ready labels. Give independent workers separate worktrees,
  branches and PRs. Dependency completion means accepted merged work, not
  merely a finished agent run.
- Require explicit concurrency, retry, time and spend policies. Stop on
  conflicts, failed gates, unsupported dependencies or exhausted limits.
  One system owns dispatch, CI repair and each other responsibility; resolve
  overlap with Copilot/ops before enabling another loop. Never auto-merge.
- Treat Orchestrator.inc as a candidate integration until supported config,
  headless operation, authentication, license and stop/resume semantics are
  verified. Do not invent its API or silently substitute another service.
- Release only reviewed code through a pinned build path. Bind artifacts,
  source/tag, digest and attestation; reject tampering, wrong identity and
  missing proof. Use `gh attestation verify` where supported. Verify current
  platform/plan support first; a signed-tag fallback needs explicit approval
  and must not be described as equivalent attestation evidence.
- The manual M3 stage0 release remains the predecessor, not a release created
  retrospectively by this module. Define signing identity, release approval,
  target platforms and artifact installation policy before implementing.
- Ops creates or updates bounded, deduplicated issues from documented
  schedules/events. Propose a regression test, lint, instruction or scenario
  for each failure; do not edit protected guidance or private scenarios
  automatically. Spend reports must distinguish main-agent limits from other
  bills. Reports may propose changes, not silently disable or enable modules.

## Acceptance scenarios

1. A synthetic epic has two independent tasks and one dependent task. Only the
   first pair run concurrently; the dependent task waits for merged inputs.
   A conflict or exhausted retry/concurrency limit stops dispatch explicitly.
2. An owner-approved live epic produces separate small PRs that pass the
   applicable CI/review/holdout gates and the owner-approved merge queue.
   No worker or scheduler can promote its own issue or merge.
3. Provenance fixtures accept the correct source/artifact identity and reject
   tampered artifacts, mismatched tags and absent/invalid evidence.
4. Repeated ops signals do not create an issue flood or duplicate a repair
   owner. Missing access, stale data and collection failures are visible.
5. Automated tests use fake services and synthetic data. Live workers, release
   publication and scheduled paid runs each need separate owner approval.

## Dependencies and required decisions

The milestone exit depends on M6: parallel PRs must traverse its review and
holdout gates. Research and independent track design need not wait for M6.
Confirm the released M3 predecessor before release-track implementation.

Before build slices, record verified Orchestrator capabilities, scheduling and
state ownership, resource/cost limits, worker isolation, cancellation/recovery,
merge-queue availability, provenance support and operational credentials.
If a track is unsupported,
leave it blocked; do not mark the entire milestone complete by omitting it.

## Non-goals and approval

No L4, automatic merge, unbounded retries, silent provider fallback, real
workers, publication, scheduled automation, new secrets or runtime changes
in this planning PR. Profile expansion requires its own accepted scope.

Keep the milestone tracker unassigned, blocked and never `ready-for-agent`.
Split verified designs into small implementation issues later. Existing
Intake/Build-cloud approval does not authorize these new loops or paid runs.
