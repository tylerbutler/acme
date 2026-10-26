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
are staged label and comment outputs. It cannot apply `ready-for-agent`.
Build-cloud checks both the original actor and the rerun actor against the
repository owner. Only the `ready-for-agent` label can trigger assignment.

Both agent workflows are disabled in GitHub, and both enablement variables
are `false`. Intake safe outputs remain staged. Build-cloud assignment is live
when its workflow and both variable gates are enabled and the owner adds
`ready-for-agent`. Enabling the workflow alone leaves its jobs skipped.
Staging prevents safe-output writes, but does not prevent paid inference.
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

The owner must separately approve each paid test. Both workflows and both
enablement variables must remain off between tests.

### Staged tests

For an intake preview, enable only Intake and set
`ACME_AGENT_WORKFLOWS_ENABLED=true`. Keep
`ACME_CLOUD_ASSIGNMENT_ENABLED=false`, open an issue as the owner and inspect
the staged output. Then restore both variables to `false` and disable both
workflows.

Build-cloud is no longer staged. Another assignment preview first needs a
reviewed and merged change that sets `safe-outputs.staged: true`, updates
`modules.build_cloud.staged` and recompiles the lock file. Do not enable the
current build-cloud workflow expecting a preview.

### One live assignment

1. Obtain explicit owner approval for one paid live assignment to a small issue
   with a goal, acceptance scenarios, non-goals and an approved spec link.
2. Keep Intake disabled and enable only Build cloud.
3. Set both `ACME_AGENT_WORKFLOWS_ENABLED` and
   `ACME_CLOUD_ASSIGNMENT_ENABLED` to `true`.
4. Have the owner remove and reapply `ready-for-agent` on that issue to create
   a new label event.
5. After the assignment run, even if it fails, restore both variables to `false`
   and disable both workflows. The separate builder can continue after cleanup.
6. Verify the actual Copilot assignment, one draft PR, green required CI, and a
   human review and merge.

The staged previews did not verify live assignment permissions. Keep
`bootstrap.live_loop_verified=false` until the live loop passes.

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
