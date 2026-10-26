# Bootstrap operations

## What is installed

This is the M0 hand-built factory. Files in `.github/`, `AGENTS.md` and the
approved draft in `factory/factory.toml` are future renderer fixtures. There is
no renderer or doctor yet. Keep config and fixtures consistent by hand.

The `main` ruleset requires a PR, the GitHub Actions `CI` check, an up-to-date
branch and resolved review threads. It has zero required approvals and no
bypass actors. Auto-merge is off. A human reviews and merges.
The extra-approval rule for unattributed Copilot changes is explicitly off:
GitHub enables it by default, which can require an approval even in solo mode.

Intake accepts admin-triggered issue opens during bootstrap. Its only writes
are label and comment outputs. It cannot apply `ready-for-agent`.
Build-cloud checks both the original actor and the rerun actor against the
repository owner. Only the `ready-for-agent` label can trigger assignment.

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

## Apply remote settings

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
no YAML dependency. The Rust doctor replaces this tool in M2.

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
assignment. Inference and the staged assignment preview passed. Live assignment
permissions remain unverified. Never add token values to the config or to
version control.

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

The staged previews did not verify live assignment permissions. Keep
`bootstrap.live_loop_verified=false` until the live loop passes.

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
- Staged tests passed; live assignment and the full issue-to-PR loop remain
  unverified.
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
