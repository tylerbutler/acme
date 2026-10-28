# Bootstrap operations

## What is installed

This is the M0 hand-built factory. Files in `.github/`, `AGENTS.md` and the
approved draft in `factory/factory.toml` are future renderer fixtures. There is
no renderer yet. Doctor checks supported local assets and remote settings.
Keep local config and fixtures consistent by hand.

The `main` ruleset requires a PR, the GitHub Actions `CI` check, an up-to-date
branch and resolved review threads. It has zero required approvals and no
bypass actors. Auto-merge is off. A human reviews and merges.
The extra-approval rule for unattributed Copilot changes is explicitly off:
GitHub enables it by default, which can require an approval even in solo mode.

Intake accepts admin-triggered issue opens during bootstrap. Its only writes
are label and comment outputs. It cannot apply `ready-for-agent`.
Build-cloud checks both the original actor and the rerun actor against the
repository owner. Only the `ready-for-agent` label can trigger assignment.
The owner can also assign Copilot directly, outside this workflow. That route
skips the approval job, variable gates and credit cap; the owner checks the spec.

Before inference, a read-only approval job checks that linked specs exist on
the default branch. When an issue identifies a `Spec PR`, the check uses the
pull-request API's `merged` field, verifies its base repository and branch, and
confirms that it changes a linked spec. An issue API's `closed` state does not
prove whether a PR merged. Missing or unreadable evidence blocks the build
and appears in the approval job summary. Main-only spec links remain supported
for the existing owner-ready path; this is not automatic coordinator approval.
Identify a spec PR with `Spec PR: https://github.com/OWNER/REPO/pull/NUMBER`
or a Markdown link whose label starts with `Spec PR:`. The `Merged`, `Approved`
and `Proposed` prefixes are also supported. Ordinary prose and unrelated PR
links are not approval declarations.

The owner approved leaving both agent workflows enabled with live safe outputs
and both enablement variables set to `true`. Intake handles admin-opened issues.
Build-cloud still requires the owner to add `ready-for-agent`.
Enabling a workflow without its variable gates leaves its jobs skipped.
`max-ai-credits: 5000` caps each main agent run. The pinned firewall rejects the
negative no-cap value before inference, so this limit must be positive.
`max-daily-ai-credits: -1` keeps the daily cap off.
Threat detection keeps its separate compiler-default credit limit.

## Recompile workflow fixtures

Install the pinned compiler and actionlint, then run:

```sh
gh extension install github/gh-aw --pin v0.93.5
just workflows
```

Review the `.lock.yml` diffs and commit them with their Markdown sources.
Do not edit lock files by hand. See the upstream
[gh-aw docs](https://github.github.com/gh-aw/) for the current authentication
requirements. The compiler pins the generated action references.

## Initial bootstrap settings

After the initial push, authenticate `gh` as the repository owner and run:

```sh
just bootstrap-github
```

This applies labels and the main ruleset, restricts the default Actions token
to read access, disables auto-merge and disables both agent workflows.
It creates a missing ruleset or updates the existing one by name. Re-running
it intentionally turns the agent workflows and enablement variables off again.
Use the continuous-operation commands below to restore the approved operating
state afterward.

`.github/labels.yml` is JSON-compatible YAML so the temporary Python tool needs
no YAML dependency. Use explicit doctor repairs for supported setting drift.
Do not run the bootstrap tool as a repair: it disables live workflows and gates.

## Owner-only M2 acceptance

The M2 live exit check is not yet recorded. Implementation tests use fake remote
state only. They do not authorize a live check, deliberate drift or a repair.
After merge, the owner must approve each live step separately:

1. Approve a read-only `cargo run -- doctor` on `tylerbutler/acme`.
2. Record the command, revision, configured repository, exit status and report.
   Resolve local drift through reviewed edits, not through remote repairs.
3. Approve one controlled change to a managed label. Save its original fields,
   change only that label, and run read-only doctor to record the named failure.
4. Review all reported drift and approve `cargo run -- doctor --fix` explicitly.
   The command repairs all supported drift, not only the test label. It requires
   an authenticated repository administrator and matching repository identity.
5. Record the repair report and final read-only report. Verify that unrelated
   resources remain intact. Record a second fix with no writes if approved.
6. Link the evidence before claiming that the M2 live exit check passed.

Doctor validates a complete plan before any writes. It blocks conflicting
checkout identity, missing admin access, unreadable state and duplicate managed
rulesets. It repairs only managed labels, the named `main` ruleset, documented
repository settings and Actions defaults. It never writes local source files.
Inherited rulesets require action by their owner.

After writes, doctor reads fresh state and runs the same checks. It stops on the
first write failure and reports partial application. An API error can occur
after a change took effect. There is no transaction or rollback. Review the
fresh report before approving a retry; remaining drift or unreadable state
still exits unsuccessfully.

Do not change secrets, workflow enablement, variable gates, staging, budgets,
assignments or issue state for this test. Never invoke `just bootstrap-github`
from doctor or as an acceptance repair.

## Credentials and explicit owner actions

The repository secret `GH_PAT` supplies both authentication paths:

- `engine.env.COPILOT_GITHUB_TOKEN` maps to `secrets.GH_PAT` in both workflows.
- `safe-outputs.assign-to-agent.github-token` references `secrets.GH_PAT`.

No duplicate `COPILOT_GITHUB_TOKEN` or `GH_AW_AGENT_TOKEN` secrets are needed.
Do not put the token in workflow-level environment variables. Do not read or
print its value, reuse the operator's local `gh` credential, or paste it into
an issue. To replace it, use the hidden prompt:

```sh
gh secret set GH_PAT --repo tylerbutler/acme
```

The shared fine-grained PAT must have resource owner `tylerbutler`, repository
access limited to `acme`, and both permission sets:

- Repository permissions: Metadata read; Actions, Contents, Issues and Pull
  requests read and write.
- Account permissions: Copilot Requests read.

Its owner must have Copilot access, and the repository must allow cloud-agent
assignment. Inference and live assignment passed in the M0 test recorded below.
Never add token values to the config or to version control.

The owner approved continuous paid operation of Intake and Build-cloud.
This does not authorize auto-merge or enable any deferred module.

### Continuous operation

Authenticate `gh` as the repository owner and enable the approved stages:

```sh
gh workflow enable intake.lock.yml --repo tylerbutler/acme
gh workflow enable build-cloud.lock.yml --repo tylerbutler/acme
gh variable set ACME_AGENT_WORKFLOWS_ENABLED --repo tylerbutler/acme --body true
gh variable set ACME_CLOUD_ASSIGNMENT_ENABLED --repo tylerbutler/acme --body true
```

Leave these settings on between normal runs. Intake can apply its allowed
labels and one comment, but never `ready-for-agent`. The owner must still add
that label to approve each specified build. All PRs need human review and merge.
Enabling workflows does not replay old issue events.

### Verify the live loop

1. Merge the reviewed live-workflow changes before testing live writes.
2. Choose a small issue with a goal, acceptance scenarios, non-goals and an
   approved spec link.
3. Have the owner remove and reapply `ready-for-agent` on that issue to create
   a new label event.
4. Verify the actual Copilot assignment, one draft PR, green required CI, and a
   human review and merge. Leave the approved workflows and variables enabled.

A staged preview alone does not verify live assignment permissions. Set
`bootstrap.live_loop_verified=true` only after the live loop passes.

### Retry an unfinished build

Closing a builder PR does not remove Copilot from its issue. The assignment
handler skips issues that already have Copilot assigned. A successful workflow
can also report a no-op instead of starting a builder; inspect its outputs.

For a same-issue retry, the owner must first confirm that no builder or open
implementation PR remains active. Close the old PR without merge, remove
Copilot from the issue, then remove and reapply `ready-for-agent`. Reopening
the old PR or rerunning the workflow does not clear the existing assignment.
Approval failures must be resolved before retrying; do not bypass the gate.

When scope needs a split, keep the parent as an unapproved tracker and create
bounded child issues with their own goal, acceptance scenarios, non-goals and
approved spec links. Only the owner may apply `ready-for-agent` to each child
after its prerequisites are satisfied. Parent approval does not transfer.

### Verified M0 loop

The owner-approved live build loop passed on 2026-10-26:

| Gate | Evidence |
| --- | --- |
| Approved issue | [tylerbutler/acme#11](https://github.com/tylerbutler/acme/issues/11), now closed |
| Live assignment | [Build cloud run](https://github.com/tylerbutler/acme/actions/runs/40282626400), all six jobs passed |
| Copilot builder | [Cloud agent run](https://github.com/tylerbutler/acme/actions/runs/40282969821), passed |
| Draft PR | [tylerbutler/acme#13](https://github.com/tylerbutler/acme/pull/13), only `CONTRIBUTING.md` changed |
| Required CI | [CI run](https://github.com/tylerbutler/acme/actions/runs/40283093471), passed on the PR head |
| Human merge | Owner merged [05ac0f0](https://github.com/tylerbutler/acme/commit/05ac0f08291e3f00f717c8e825e2eb7904e90df3) |

This verifies the live assignment, builder, CI and human-merge path.
`modules.build_cloud.token_verified` and `bootstrap.live_loop_verified` are
`true`. Both workflows and their enablement variables remain on.

### Stop new runs or restore preview mode

Set both enablement variables to `false` to stop future activation, and disable
the workflows if needed. This does not cancel a running workflow or a separate
builder that was already assigned.

Both workflows are live. A staged preview first needs a reviewed and merged
change that sets `safe-outputs.staged: true` for the relevant workflow, updates
its `staged` value in `factory/factory.toml` and recompiles the lock file.
Do not enable a live workflow expecting a preview. Staged runs still use AI
credits.

## Deferred safeguards and exceptions

- No monthly or daily budget; each main agent run has a 5000-AI-credit cap.
- Live Intake label/comment writes were not exercised in this build loop.
  [Staged Intake](https://github.com/tylerbutler/acme/actions/runs/40277232574)
  passed separately.
- No release pin: there is no released CLI. `target_version` is not an install pin.
- No automated review. Copilot will run review with a different model from build.
- No enforceable agent-author/path gate yet. CODEOWNERS and risk paths document
  the policy; they do not enforce it. M2 must add the policy check.
- No hard cloud-agent runtime limit yet. The 59-minute scope is an instruction,
  not a platform-enforced timeout.
- No holdout, build-local, release, ops, merge queue or second template repo.

Before adoption, run the future renderer against this config until the fixtures
have zero diff. Then run doctor and sync. Build the first release by hand and
replace the bootstrap marker with a tagged stage0 pin, never HEAD.
