# Bootstrap operations

## What is installed

This is the M0 hand-built factory. Files in `.github/`, `AGENTS.md` and the
approved draft in `factory/factory.toml` are renderer fixtures. Render produces
the deterministic M2 inventory. Sync can adopt exact matching files with separate
owner approval. Doctor checks supported local assets and remote settings.

The `main` ruleset requires a PR, the GitHub Actions `CI` check and resolved
review threads. Branches do not have to be up to date before merging.
It has zero required approvals and no
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

### Check an issue before build approval

Put a full spec file URL in the issue body. A code-formatted path, a shorthand
PR number, a link in a comment or a link only in the spec PR body does not
satisfy the checker. For example:

```markdown
## Spec and prerequisites
[Spec](https://github.com/tylerbutler/acme/blob/main/docs/specs/0012-intake-spec-classification.md)
Spec PR: https://github.com/tylerbutler/acme/pull/100
```

The spec PR line is optional for an existing default-branch spec. If present,
it must name a merged PR that changes a linked spec. After a spec merges,
update its implementation issue to remove stale draft status and add these
links. PR publication and merge do not repair the issue body automatically.

Run the same validator locally before requesting an owner ready label:

```sh
just check-build-approval tylerbutler/acme 99 main
```

Supply the intended repository, issue number and actual default branch.
The command requires an authenticated `gh` CLI. It reads current issue,
file and PR metadata, prints the result, and exits nonzero on missing,
invalid or unavailable evidence. It does not write Actions output files,
change GitHub state, run inference or assign work. A pass is not approval of
scope or proof that prerequisites are complete. Build-cloud repeats its
check when the owner applies the ready label.

### Continuous operation

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
- The live config still uses bootstrap mode. Its `target_version` is a build
  target, not an enforced install pin.
- No automated review. Copilot will run review with a different model from build.
- No enforceable agent-author/path gate yet. CODEOWNERS and risk paths document
  the policy; they do not enforce it. M2 must add the policy check.
- No hard cloud-agent runtime limit yet. The 59-minute scope is an instruction,
  not a platform-enforced timeout.
- No holdout, build-local, release, ops, merge queue or second template repo.

## Owner-only stage0 adoption

Only the owner may build and tag stage0, approve the non-bootstrap pin, and
adopt the live factory. Agents must not publish releases or change the live
bootstrap flag. A tag alone does not prove adoption.

1. Select a reviewed, human-merged revision. Use the pinned Rust toolchain.
   Run `just check`, then build the binary by hand:

   ```sh
   cargo build --locked --release --bin acme
   ```

   Record the source revision, `Cargo.toml` package version, toolchain and build
   result. Preserve the resulting binary outside the build directory.
2. Verify that the package version is the intended concrete version. The owner
   publishes the corresponding `v<target_version>` tag on that exact revision.
   If the tag already exists, verify its revision; never move it to newer code.
   Changes after stage0 require a new version and tag. There is no release
   automation, self-update or automatic download.
3. Install that exact binary explicitly. In a reviewed config change, set
   `[toolchain.acme]` to `bootstrap = false` and set `target_version` to its
   package version, without `v`. Keep `schema_version = 0` and all unrelated
   settings unchanged. For example, a binary built as `0.1.0` uses the real
   tag `v0.1.0` and pin `"0.1.0"`, not HEAD, `latest` or a range.
4. With the installed binary, render the approved config into a fresh temporary
   directory:

   ```sh
   preview=$(mktemp -d)
   acme render --config factory/factory.toml --output "$preview"
   ```

   Compare every rendered file with its matching checkout path using raw bytes.
   Include hidden `.github` paths. The current inventory is the CI workflow,
   Rust setup action, labels, ruleset and four issue-form files.
   Require zero golden diff for initial adoption. Do not copy unexpected output
   over reviewed fixtures. Resolve incompatible schema or module contracts
   through a reviewed migration before retrying; do not bypass validation.
5. Inspect `acme sync --check`. An initial ownership-record change is expected
   and makes check mode fail. Resolve any conflict manually. Approve and run
   `acme sync`, then run `acme sync --check` again. The second check must succeed
   without output or metadata drift.
6. Approve and run read-only `acme doctor` with authenticated `gh`. Do not pass
   `--fix`. Record the installed binary's version and source revision, tag,
   golden comparison, sync reports and doctor report. Missing credentials or
   unresolved checks mean adoption is not verified.
7. Review the pin and `factory/managed-files.json` diff. Human-merge the adoption
   changes only after the checks pass. Only then claim self-hosting adoption.

### Stage0 adoption evidence

Recorded on 2026-10-28 from source revision
`997a365af1e2b167b2c209f84798006c77860feb`. The package version was `0.1.0`.
The pinned Rust toolchain resolved to `rustc 1.85.1
(4eb161250 2025-03-15)`. `TMPDIR=/private/tmp just check` passed; 26 Python
tests ran and 2 were skipped.

The owner authorized retargeting the existing `v0.1.0` tag. Its previous
commit was `0342c130b30658eb91d4aaacba8a77a87345e70b`; it now points to
`997a365af1e2b167b2c209f84798006c77860feb` through annotated tag object
`22e77755b4018ac2e85efbf06e5971c8dbd11831`. This is an exception to step 2
above. Existing clones may still retain the previous target.

`cargo build --locked --release --bin acme` passed. A copy of the binary was
preserved outside the build directory; its SHA-256 is
`87c2c902492e6525ae1f32d89a6e01c5e2695ee692877f0af0c7f79818c8c0cb`.
That binary rendered all 8 inventory files, each matching the checkout
byte-for-byte. The first `sync --check` requested only
`factory/managed-files.json`; sync wrote that record, and the next check was
clean.

Read-only `doctor --config factory/factory.toml` reported 56 PASS, 0 FAIL and
3 DEFERRED: path-author enforcement, model separation and token-scope
verification. These safeguards remain unimplemented or unenforced. No
`doctor --fix` command was run. Adoption remains pending human review and merge.

For an upgrade, approve a new concrete config pin and explicitly install the
matching tagged binary. Render, review the output diff, sync, check again and
run read-only doctor. Unchanged outputs may require only an ownership-record
version change. The next sync must be a no-op. Do not edit recorded hashes to
override modified owned files.

## Owner-only M5 release and adoption

Track this work in [#96](https://github.com/tylerbutler/acme/issues/96), under
the [M5 contract](specs/0006-m5-agent-modules-contract.md). The implementation
prerequisites merged in
[#105](https://github.com/tylerbutler/acme/pull/105),
[#106](https://github.com/tylerbutler/acme/pull/106) and
[#107](https://github.com/tylerbutler/acme/pull/107).

The `0.3.0` candidate includes the M5 implementation. The existing `v0.2.0`
tag identifies older code; never move it to this implementation. The live
factory stays pinned to `0.1.0` until the owner approves a separate adoption
change. Preparing this release does not publish it, install a binary or adopt
workflow files.

Only the owner may publish the release. Publication, installation and live
adoption require explicit owner action or approval. Keep the current workflow
enablement, variable gates, staging, model choices, secret references and
budgets unchanged. Do not use bootstrap mode or a fictitious pin. Do not run
`doctor --fix`, provisioning, workflow jobs or paid acceptance tests.

### Build and publish the merged release

1. Select the human-merged release-preparation revision on `main`. Confirm that
   its package version is `0.3.0` and the checkout is clean. Use the pinned Rust
   toolchain, then record these commands and their results:

   ```sh
   git rev-parse HEAD
   rustc --version
   just check
   cargo build --locked --release --bin acme
   shasum -a 256 target/release/acme
   ```

   On macOS, use `TMPDIR=/private/tmp just check` if the default temporary path
   has symlink components. Record the environment override with the results.
   Preserve the binary outside `target/`, together with its revision, package
   version, toolchain and SHA-256. Do not rebuild it from an unmerged adoption
   branch and call it the released binary.
2. The owner publishes a new immutable `v0.3.0` tag on that exact merged
   revision. If it already exists, verify its target; do not move it. Record
   the tag and release evidence in #96. Changes to released code require a new
   package version and tag.
3. Explicitly install the preserved binary at the intended executable path.
   Retain the previous binary until adoption is complete. Compare the installed
   binary's SHA-256 with the recorded build hash, and use that explicit path
   for every command below. The examples call it `acme_binary`:

   ```sh
   acme_binary="$HOME/.local/bin/acme"
   shasum -a 256 "$acme_binary"
   ```

   The path is an example, not an installation command. The CLI has no
   self-update or automatic download step.

### Prepare and verify the adoption change

1. On a separate adoption branch, review a change to only
   `toolchain.acme.target_version` in `factory/factory.toml`, from `"0.1.0"`
   to `"0.3.0"`. Keep `bootstrap = false` and all other config values unchanged.
   The package version, real release tag and installed binary must match.
2. Install gh-aw `v0.93.5` and native actionlint `1.7.12` or later if missing.
   Tool installation is an explicit prerequisite, not part of compilation.
   With the approved config, compile and render into separate new directories:

   ```sh
   review=$(mktemp -d)
   preview=$(mktemp -d)
   "$acme_binary" compile --config factory/factory.toml --output "$review"
   "$acme_binary" render --config factory/factory.toml \
     --compiled-inputs "$review" --output "$preview"
   ```

   Use canonical directory paths without symlink components. On macOS,
   set `TMPDIR=/private/tmp` before these commands if needed. Keep temporary
   compilation outside Git checkouts. Compilation does not run inference or
   workflow jobs.
3. Compare every rendered file with its matching checkout path using raw bytes.
   Require all 13 files to match: the 8 M2 files listed in stage0 adoption,
   both workflow Markdown files, both compiled locks and
   `scripts/check-build-approval.py`. Include hidden `.github` paths.
   Review the compilation record and preserve the comparison evidence.
   `compiled-workflows.json` is review metadata, not a managed checkout file.
   Stop on any difference; do not copy output over live files or edit locks
   or recorded hashes to force adoption.
4. Inspect the complete sync plan:

   ```sh
   "$acme_binary" sync --check --config factory/factory.toml \
     --compiled-inputs "$review"
   ```

   Before adoption, an ownership-record change is expected and makes check
   mode fail. It must not request workflow content changes. Resolve conflicts
   through review before proceeding. After explicit owner approval, run:

   ```sh
   "$acme_binary" sync --config factory/factory.toml --compiled-inputs "$review"
   "$acme_binary" sync --config factory/factory.toml --compiled-inputs "$review"
   "$acme_binary" sync --check --config factory/factory.toml \
     --compiled-inputs "$review"
   "$acme_binary" doctor --config factory/factory.toml \
     --compiled-inputs "$review"
   ```

   The first sync adopts exact outputs and writes ownership metadata. The
   second sync must be a no-op, and the final check must pass. Doctor is
   read-only and requires authenticated `gh`. Record all results, including
   failures, unavailable API evidence and deferred checks. Do not claim that
   credential-name checks verify token scopes or assignment capability.
5. Review the config pin and `factory/managed-files.json` diff. Expect the
   generating version to become `0.3.0`, with the existing M2 entries preserved
   and five exact agent-module ownership entries added. No workflow, helper or
   other rendered content may change for this adoption. Link the draft adoption
   PR, release revision/tag, installed binary hash, compilation record,
   comparison, sync reports and fresh doctor report in #96. Preserve the review
   directory outside the checkout until review is complete.

Only the owner reviews and merges the adoption PR. Keep #96 open until its
release and adoption evidence is complete and the adoption change is
human-merged. Staged/live event acceptance is separate work with separate
owner approval.

## M4 release preparation and template handoff

The `0.2.0` candidate adds deterministic local `acme init` for an approved
CLI/Rust config. It does not publish a release or complete the
[M4 sample acceptance](specs/0005-m4-init-template-contract.md#acceptance-scenarios).
The adopted `v0.1.0` binary has no init command.

The live factory remains pinned to `0.1.0`, with its existing ownership record.
Keep that installed binary for live factory operations. The `0.2.0` candidate
must reject the old pin before render, sync or init writes. Do not change
bootstrap mode or the live pin to bypass this check.

1. After human merge, the owner selects the exact reviewed revision with
   package version `0.2.0` and uses the pinned toolchain:

   ```sh
   just check
   cargo build --locked --release --bin acme
   git rev-parse HEAD
   rustc --version
   sha256sum target/release/acme
   ```

   Record the revision, package version, toolchain, binary hash and check
   results. Preserve this binary outside the build directory. These results
   verify a candidate; they do not prove publication or live adoption.
2. Verify rendering with a temporary copy of the complete factory config.
   Change only that copy's `toolchain.acme.target_version` to `"0.2.0"`.
   Use the candidate's explicit path and `render --config PATH --output DIR`
   with a fresh output directory. Compare every generated byte with the
   reviewed M2 inventory, including hidden paths. Do not run sync in the live
   checkout or change its ownership record.
3. The owner publishes a new immutable `v0.2.0` tag on that exact merged
   revision. Never move `v0.1.0` or reuse it for the init implementation.
   Explicitly install the matching binary for new projects. Upgrading the
   live factory still requires the separate reviewed pin, render, sync and
   read-only doctor procedure above.
4. Only after publication, prepare a separate change in
   [`tylerbutler/acme-template`](https://github.com/tylerbutler/acme-template).
   Pin the actual release and verify the bundled schema against that revision.
   Copy the generated schema unchanged if an update is needed. Update the
   README, agent map, interview guidance and scenario checks for the released
   init command.
5. In that template change, keep all modules off by default. Permit the
   interview to propose supported CI only when the owner explicitly requests
   it. Keep agent stages, workflow/assignment flags and automatic dispatch off.
   The interview stays read-only and requires complete config review before a
   human invokes init. Publishing the generated CI workflow needs separate
   approval; it does not authorize agent workflows or paid work.

Keep [#86](https://github.com/tylerbutler/acme/issues/86) open. Creating the sample
repository and running its brief-to-human-merge test require separate owner
approval. Record elapsed time, waits and failures; a successful local render
or init is not the milestone exit check.
