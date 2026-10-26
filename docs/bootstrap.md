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
are `false`. Safe outputs are also staged. These are separate controls:
staging prevents safe-output writes, but does not prevent paid inference.
Both credit fields are explicitly `-1` in the workflow sources: omitting them
would allow gh-aw's built-in credit caps to apply.

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
assignment. Secret presence is confirmed; permissions and authentication have
not been tested. Never add token values to the config or to version control.

The owner must separately approve any paid test. To dry-run:

1. Verify credentials and choose a small, non-sensitive issue.
2. Enable the relevant workflow in GitHub, set
   `ACME_AGENT_WORKFLOWS_ENABLED=true`, and keep `safe-outputs.staged: true`.
3. Open an issue as the owner and inspect intake's staged output.
4. For assignment preview, also set `ACME_CLOUD_ASSIGNMENT_ENABLED=true` and
   have the owner add `ready-for-agent` to a fully specified issue.
5. Restore both variables to `false` and disable both workflows.

A staged assignment does not create an agent PR. After the dry run, a live
end-to-end test needs another explicit owner approval, a reviewed PR that
removes staging for the required outputs, and the corresponding config update.
Verify a draft agent PR, green required CI, and a human merge. Until then,
`bootstrap.live_loop_verified` remains `false`.

## Deferred safeguards and exceptions

- No monthly budget or per-run credit cap, by explicit owner decision.
- No paid run or live-loop verification during this bootstrap.
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
