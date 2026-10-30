# acme

A deterministic software-factory CLI, written in Rust. The binary reads
`factory/factory.toml` and renders the supported workflows and guardrails.
It does not call an LLM. An agent skill will conduct the init interview.

**Status: M0 factory loop verified.** The factory is hand-built. `doctor` runs
read-only checks against the local module assets and the GitHub repository named
in the config. It requires an authenticated `gh` CLI. `render` writes the
deterministic inventory to a new or empty directory. Enabled agent modules need
an explicit, validated compiled review directory. `sync` updates only
validated module inventory paths in the current repository. `init` prepares a
local change set from approved config for a supported Rust CLI project.
Automatic dispatch is paused.

## Development

```sh
just build
just check
cargo run -- doctor
cargo run -- doctor --config path/to/factory.toml
cargo run -- doctor --compiled-inputs /tmp/acme-agent-review
cargo run --bin acme -- render --compiled-inputs /tmp/acme-agent-review --output /tmp/acme-render
cargo run --bin acme -- sync --check --compiled-inputs /tmp/acme-agent-review
```

Rust is pinned in `rust-toolchain.toml`. Python 3 is used for the bootstrap
and commit-config scripts. `just check` runs formatting, Clippy, config checks,
and tests. Doctor reports drift and repair guidance. It does not change local
files. Default execution does not change GitHub settings. It does not verify
token scopes, enforce path-author policy, or enforce model separation.

When Intake or Build-cloud is enabled, supply `--compiled-inputs DIR` from a
reviewed `acme compile` result for the same approved config and binary version.
Doctor compares local workflow sources and the approval helper with the source
plan. It validates the compiled review record and compares local locks with the
captured bytes. Missing, stale or unsafe inputs produce unsuccessful checks.
Doctor never invokes the compiler, inference or workflow jobs.

Agent checks also inspect GitHub workflow state, activation variables and
configured secret-name presence. They do not read secret values or verify
token scopes or assignment capability. Reports keep configured model roles and
runtime selection separate from usage evidence and unknown serving models.
Doctor does not fetch usage logs or run paid credential or model verification.
Missing activation variables count as off; failed API requests remain failures.
Missing configured secrets fail checks even for inactive workflows.
Stored credential flags do not replace fresh secret-name checks.

With explicit owner approval, `cargo run -- doctor --fix` repairs supported
managed labels, the named `main` ruleset, repository merge settings and Actions
defaults. `--config PATH` and `--compiled-inputs DIR` also work with `--fix`.
Fix requires authenticated repository-administration access.
All requests use the configured repository.
A conflicting checkout origin or API repository identity blocks the plan.
An existing checkout needs an identifiable `github.com` origin.

Doctor validates the complete plan before writes, then reads fresh state and
runs the same checks. A second fix makes no writes if supported settings match.
A failed write stops further writes. Partial changes and failed verification
remain failures; doctor does not promise rollback. Local drift needs a reviewed
edit or local sync within the ownership rules below. Unrelated labels and
rulesets remain unchanged. Duplicate managed rulesets need manual resolution.

Fix does not provision secrets or change workflow enablement, variable gates,
staging, budgets, assignments or issue state. It never runs the bootstrap script.
See the [owner-only M2 acceptance procedure](docs/bootstrap.md#owner-only-m2-acceptance).
Implementation and fake-state tests do not establish the live M2 exit check.

### Offline event-gate proof

```sh
ACME_GH_AW_ACTIONS=/absolute/path/to/gh-aw-actions just prove-event-gates
```

Supply a local copy of `github/gh-aw-actions` at
`de9d416cac8ccea1a2d582cb07c4b7db96946792` (gh-aw `v0.93.5`).
The proof verifies hashes of its JavaScript and JSON runtime files and requires
Node.js. It does not download tools, use credentials, run inference or make
remote writes. It reads the reviewed compiled Intake and Build-cloud locks,
executes their membership and safe-output scripts with fake GitHub state,
and runs the approval helper with a fake read-only `gh api`.

Fixtures check event types, actor/label/variable gates, job dependencies,
permissions, spec evidence, output limits, target binding and assignment replay.
The guard evaluator supports only the expression subset used by these locks;
unknown expressions, API calls and runtime files fail the proof. Runtime files
under `/tmp/gh-aw` are isolated for each fixture. Ordinary `just check` runs
the fixture checks but skips runtime execution when `ACME_GH_AW_ACTIONS` is
absent. A skip is not proof; the explicit command fails if the runtime is absent.

**Current blocker:** the explicit proof fails
`test_missing_issue_scope_blocks_assignment`. A proposed assignment with valid
spec evidence but no goal, acceptance scenarios or non-goals reaches the fake
assignment API. These scope checks are controller instructions, not deterministic
guards. The test does not change live workflows or replace this failure with
an expected pass.

This is not a full Actions runner. Detection outcomes are fixture inputs.
It does not prove controller compliance, real token scopes, concurrent-run
exclusion, a builder's draft-PR behavior or the live human-merge boundary.
Those boundaries still need separate evidence and owner-approved acceptance.

### Local init

Init is included in the `0.2.0` release candidate, not the adopted `v0.1.0`
release. The owner must publish and explicitly install the matching release
before the template can use it. See the
[M4 release handoff](docs/bootstrap.md#m4-release-preparation-and-template-handoff).

Review the complete config before you run init. An explicit invocation supplies
the approved config; the CLI does not infer approval from the brief.

```sh
acme init --config /path/to/approved-factory.toml
```

Without `--config`, init reads `factory/factory.toml`. With a separate approved
file, it copies the exact config bytes to `factory/factory.toml`. An existing
config must match; init never replaces a different config.

Init requires the CLI/Rust preset, L3 autonomy, a non-bootstrap acme pin that
matches the running binary, and supported local project inputs. Agent stages,
workflow/assignment flags and automatic dispatch must remain off. Init uses M1
validation and the M3 render plan and ownership checks. It rejects missing
prerequisites, unsupported dependencies, conflicting outputs, symlinks and
non-regular targets before writes. It preserves the brief, authored instructions,
specs and project source. An enabled Spec stage must use `implementation = "human"`.

The initial preset supports a single-package Cargo project with local Rust
source targets, not a workspace. Custom `.cargo` config overrides are unsupported.
CI commands are limited to `cargo fmt [--all] -- --check` and
`cargo check`, `build`, `test` or `clippy`. The latter commands accept
`--locked`, `--all-targets`, `--all-features` and `--no-default-features`;
Clippy also accepts a final `-- -D warnings`. A locked command requires a
nonempty, parseable `Cargo.lock`. Init checks these inputs without running
commands or resolving dependencies. Init does not support custom scripts or
shell commands.

The result contains supported generated files and
`factory/managed-files.json`. An existing ownership record blocks init; review
the project and use `acme sync --check` followed by an approved `acme sync`
instead. Resolve conflicts by hand. Like sync, init writes staged files and
writes the record last. An I/O error can leave partial changes. Do not run it
with concurrent checkout writers.

After init, review the complete diff, run the configured local checks, and
explicitly create a branch, commit and draft PR. For example:

```sh
git switch -c factory-init
git status --short
git diff
git add -- factory/factory.toml factory/managed-files.json
# Also stage the generated paths listed by init.
git diff --cached
git commit -m "feat: initialize factory"
gh pr create --draft
```

Review untracked files too. Do not stage unrelated authored files.

Init does not execute configured CI commands, call an LLM, read secret values,
contact GitHub, provision settings, activate agent workflows, create a branch or
PR, or start paid work. Repository administration needs separate owner approval.

### Local sync

`acme sync [--check] [--config PATH] [--compiled-inputs DIR]` uses the same config
and validated output bytes as `render`. Check mode lists path-sorted creates,
updates and removals, followed by any ownership-record change. It exits
unsuccessfully on output or metadata drift and does not write files or create
directories.

Sync keeps deterministic bookkeeping at `factory/managed-files.json`, with
format version 1, the generating acme version, and path-sorted module, path
and lowercase SHA256 entries. A first sync adopts existing files only if their
bytes match rendered output. Owned files must match their recorded hash before
update or removal. Missing owned outputs are recreated; missing obsolete
outputs are dropped from the record. Disabled modules lose only their recorded,
unmodified outputs. Unowned files and parent directories remain.

Sync validates the complete plan, record and affected paths before writes.
It rejects unknown record fields, owners and paths, unsupported record formats,
duplicates, invalid hashes, symlink components and non-regular file targets.
It stages each replacement in a new sibling file, then renames it. It writes the
record last. An I/O error stops sync and reports partial application; there is
no rollback. Resolve partial changes by hand before retrying. Do not run sync
with concurrent checkout writers: these checks are not a race-proof filesystem
sandbox. Sync is local; it does not run doctor, provision settings or compile
agent workflows.

Outside `[toolchain.acme]` bootstrap mode, render and sync require a concrete
`target_version` that exactly matches the running binary. Use the version without
the release tag's `v` prefix. Moving references and ranges are not pins.
Validation fails before writes, including in sync check mode. Bootstrap mode
treats the version as a build target only. The CLI does not download binaries
or check remote tags.

The live factory has not been adopted by this implementation. Live adoption
requires the separate owner-approved release and pin procedure in the
[owner-only adoption guide](docs/bootstrap.md#owner-only-stage0-adoption).

### Local rendering

The first two examples need a config with agent modules disabled. Enabled agent
modules require the compiled input shown in the third example.

```sh
acme render --output /tmp/acme-render
acme render --config path/to/factory.toml --output /tmp/acme-preview
acme render --compiled-inputs /tmp/acme-agent-review --output /tmp/acme-full-preview
```

Render reads `factory/factory.toml` by default and validates the complete plan
before writing. The destination must be absent or empty. Render rejects
symlinked destinations, symlinked ancestors and parent (`..`) components.
An I/O failure can leave partial output;
inspect it and use a fresh destination to retry.

The binary embeds its templates and does not need `gh` to render or sync.
The M2 inventory contains the CI workflow, Rust setup action, labels, issue forms
and ruleset fixture, with LF line endings. Enabled agent modules also add their
validated workflow sources, compiled locks and required approval helper.
Agent planning checks authored prerequisites in the current repository.
Render does not execute configured commands, call GitHub, compile gh-aw or write
ownership records. Golden tests compare output bytes with reviewed fixtures.
The bundled M2 assets remain fixed doctor fixtures; the two `.tmpl` assets
supply the config-dependent render text.

### Agent source planning

The library API `acme::render::build_agent_source_plan(&config, repository_root)`
plans bundled Intake and Build-cloud Markdown and the approval helper. It checks
the acme and gh-aw pins, dependencies and authored prerequisites without writing
files or running tools. `AGENTS.md` must be a nonempty UTF-8 regular file.
Build-cloud also needs a valid authored `commit-types.json`; its helper requires
`python3` and `gh` on the workflow runner.

The plan declares exact source, helper and future lock ownership, but contains no
compiler-output bytes. The separate compilation command supplies compiler-output
bytes for review. Full render and sync consume these bytes after validation;
source planning alone does not compile, synchronize or activate workflows.

Both `engines.intake` and `engines.build_cloud` accept optional `controller_model`
and `detection_model` fields. The existing `engines.build_cloud.model` selects only
the assigned builder. Overrides are literal identifiers: 1–128 ASCII letters,
digits, dots, underscores or hyphens, starting with a letter or digit. Expressions
are rejected; there is no model allowlist or availability check. Omission preserves
the reviewed source bytes. A controller-only override adds the trusted detection
selection expression so the controller cannot become the detection default.
Source tests establish rendering behavior, not serving-model identity.

### Agent workflow compilation

```sh
acme compile --output /tmp/acme-agent-review
acme compile --config path/to/factory.toml --output /tmp/acme-agent-review
```

Compilation reads `factory/factory.toml` by default. It uses the same source
planning, dependency checks, authored prerequisites and version checks described
above. The output must be a new or empty directory. Compilation does not change
checkout files or `factory/managed-files.json`. It does not install tools, fetch
imports, run configured commands, run workflow jobs or require credentials.
Install gh-aw `v0.93.5`, native `actionlint` version `1.7.12` or later, and
Git `2.28` or later. Acme finds `gh-aw` on `PATH` or in the standard gh extension
directory. It does not invoke `gh aw` to discover or install a missing extension.

The review directory contains enabled agent-module Markdown, compiled locks,
the required approval helper, and `compiled-workflows.json`. Disabled modules
have no output. The record is written last, after successful compilation and
validation. A failed operation does not publish a completed record. Use a fresh
output directory to retry if an I/O failure leaves partial files.
Do not compile or validate with concurrent filesystem writers.

`compiled-workflows.json` uses format version `1`. It records:

- `acme_version` and `compiler_version`;
- `config_sha256`, the SHA-256 hash of the typed config serialized as
  two-space-indented JSON with one trailing newline;
- `assets`, a sorted map of bundled source, template and manifest hashes,
  including M2 capability providers;
- `compiler_options`, `validator_options` and `action_cache_sha256`;
- `outputs`, sorted by relative path, with each output's `module`, `path` and
  `sha256`.

The JSON record has no timestamps, absolute host paths or credential values.
Config formatting does not affect its hash. Changes to approved config values,
bundled assets, compiler options or output bytes invalidate the record.
Compilation uses an isolated workspace and a cleared process environment, with
only fixed settings and the local tool search path restored. Ambient model,
compiler, Git and credential settings do not select generated workflow behavior.
The temporary directory must be outside Git checkouts and the review directory.
Acme fixes the compiler options to `--no-check-update --strict --action-mode action
--action-tag v0.93.5` and supplies bundled action pins. It runs local actionlint
separately with `-shellcheck= -pyflakes=`. It does not use gh-aw's `--validate`
or scanner-installation paths.

Acme adds two scoped compatibility patterns for actionlint's missing support
for generated `concurrency.queue` and `job.workflow_ref`. The pinned
[gh-aw compiler also handles these known gaps](https://github.com/github/gh-aw/blob/v0.93.5/pkg/cli/lint_command.go#L19-L37).
Other unknown concurrency keys, job properties and `workflow_ref` properties in
other contexts still fail validation. The record binds these validator options;
they are not caller-configurable. Failed tools report both stdout and stderr.

For enabled workflows, compilation initializes an empty private Git repository
with templates disabled and a fixed initial branch. Git metadata stays outside
the review directory. This lets gh-aw reconcile failure-issue expiry exactly as
it does in the reviewed checkout. No commits, remotes or hooks are configured,
and ambient Git configuration is not inherited.

The reusable library API
`acme::render::validate_compiled_inputs(&config, repository_root, review_directory)`
recomputes expected sources and validates the record, versions, exact inventory,
paths and file hashes before returning output bytes. It does not run the compiler
or write files. It rejects missing, stale, edited, extra or incompatible inputs,
symlinks and non-regular files. Plain hashes detect mismatches; they do not
authenticate a compiler or an untrusted review directory.

### Compiled inputs for render and sync

When Intake or Build-cloud is enabled, render, sync and sync check mode require
`--compiled-inputs DIR`. Supply the review directory made for the same approved
config and bundled assets:

```sh
acme compile --output /tmp/acme-agent-review
acme render --compiled-inputs /tmp/acme-agent-review --output /tmp/acme-render
acme sync --check --compiled-inputs /tmp/acme-agent-review
# After review, in the intended checkout:
acme sync --compiled-inputs /tmp/acme-agent-review
acme sync --check --compiled-inputs /tmp/acme-agent-review
```

These consumers validate the complete review directory before output writes,
then use its captured bytes. They never invoke the compiler or actionlint.
The compilation record is review metadata, not a managed checkout output.
Missing, stale, edited, extra or incompatible inputs fail before writes.
The full source/lock/helper inventory uses the same adoption, conflict and
ownership checks as M2 outputs.

Library callers can use `render_to_directory_with_compiled_inputs` and
`sync_repository_with_compiled_inputs` in `acme::render`. Both take an explicit
repository root and an optional review-directory path. The original APIs reject
enabled agent modules when no compiled input is supplied.

After a config change, compile only the remaining enabled workflows into a fresh
review directory. Sync removes a disabled module's recorded, unmodified outputs
without compiling that disabled workflow. If no agent modules remain enabled,
render and sync need no compiled input or compiler and use the M2 path.
Removal preserves authored instructions, commit configuration, briefs, specs
and unrelated helpers.

No live adoption is performed by this implementation. Compilation, render and
sync do not authorize publication, activation, assignment or paid tests.

Run `just prove-compilation` with the required local tools to check pinned-compiler
repeatability and model-role separation, including detection fallback. This proof
compiles in temporary directories without running workflow jobs. Normal
`just check` compiles this test but does not run its tool-dependent proof.

### Agent comment style

The APM source rule is
[`.apm/instructions/code-comments.instructions.md`](.apm/instructions/code-comments.instructions.md).
It applies ASD-STE100 guidance to new and changed code comments, including
documentation comments and docstrings. It does not require changes to existing
comments or claim verified compliance with the complete standard.

After you edit the source rule, use [APM](https://github.com/microsoft/apm) to
validate it and generate the Copilot rule:

```sh
apm compile --target copilot --local-only --validate
apm install --target copilot --only apm
```

Commit the source rule, the generated file in `.github/instructions/`, and
`apm.lock.yaml`. The project manifest is `apm.yml`. Copilot reads the committed
rule without an APM install step. Keep the hand-written `AGENTS.md`; it remains
the shared instruction source.

## The first factory

| File | Purpose |
| --- | --- |
| `factory/brief.md` | Project goals and constraints |
| `factory/factory.toml` | Human-approved config read and validated by doctor and render |
| `AGENTS.md` | Agent instructions and risk boundaries |
| `.github/workflows/ci.yml` | Required `CI` check: fmt, Clippy and tests |
| `.github/workflows/intake.md` | Admin-opened issue intake |
| `.github/workflows/build-cloud.md` | Owner-authorized Copilot assignment |
| `.github/labels.yml` | Issue state labels |
| `.github/rulesets/main.json` | Main ruleset: PR, zero approvals, required CI |
| `docs/specs/0001-bootstrap.md` | M0 scope, gates and acceptance criteria |
| `docs/adr/0001-factory-boundaries.md` | Architecture and bootstrap exceptions |

The owner approved leaving Intake and Build-cloud enabled with live safe
outputs and both variable gates set to `true`. Intake triages admin-opened
issues. Build-cloud still requires the owner to add `ready-for-agent` to a
specified issue. The owner can also assign Copilot directly, which skips
Build-cloud and its gates. **These events can use paid AI credits.** Each main agent run
has a 5000-AI-credit cap; monthly and daily budgets are off.
Both authentication paths use `GH_PAT`. Inference and live assignment passed. See
[the bootstrap guide](docs/bootstrap.md) before enabling anything.

The [M0 live-loop record](docs/bootstrap.md#verified-m0-loop) links the Copilot
draft PR, passing CI and human merge. Holdout, local builds, automated review,
release and ops are deferred.

## Template basis

The Rust scaffold, lints, release profiles, licenses, toolchain, formatting,
composite setup action, dependency updates and commit-config tools come from
[`tylerbutler/rust-template`](https://github.com/tylerbutler/rust-template) at
`240a7d187109bd3b99fd5e7217763b71d4f84e42`.
The source is not marked as a GitHub template, so this repository uses a selected
file snapshot with new Git history. Unneeded template release, coverage and
multi-platform workflows are not installed.

## License

MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
