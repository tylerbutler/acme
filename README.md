# acme

A deterministic software-factory CLI, written in Rust. The binary reads
`factory/factory.toml` and renders the supported M2 workflows and guardrails.
It does not call an LLM. An agent skill will conduct the init interview.

**Status: M0 factory loop verified.** The factory is hand-built. `doctor` runs
read-only checks against the local module assets and the GitHub repository named
in the config. It requires an authenticated `gh` CLI. `render` writes the
deterministic M2 inventory to a new or empty directory. `sync` updates only
validated M2 inventory paths in the current repository. `init` prepares a local
change set from approved config for a supported Rust CLI project.
Automatic dispatch is paused.

## Development

```sh
just build
just check
cargo run -- doctor
cargo run -- doctor --config path/to/factory.toml
cargo run --bin acme -- render --output /tmp/acme-render
cargo run --bin acme -- sync --check
```

Rust is pinned in `rust-toolchain.toml`. Python 3 is used for the bootstrap
and commit-config scripts. `just check` runs formatting, Clippy, config checks,
and tests. Doctor reports drift and repair guidance. It does not change local
files. Default execution does not change GitHub settings. It does not verify
token scopes, enforce path-author policy, or enforce model separation.

With explicit owner approval, `cargo run -- doctor --fix` repairs supported
managed labels, the named `main` ruleset, repository merge settings and Actions
defaults. `--config PATH` also works with `--fix`. Fix requires authenticated
repository-administration access. All requests use the configured repository.
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

### Local init

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

`acme sync [--check] [--config PATH]` uses the same config and render plan as
`render`. Check mode lists path-sorted creates, updates and removals, followed
by any ownership-record change. It exits unsuccessfully on output or metadata
drift and does not write files or create directories.

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

```sh
acme render --output /tmp/acme-render
acme render --config path/to/factory.toml --output /tmp/acme-preview
```

Render reads `factory/factory.toml` by default and validates the complete plan
before writing. The destination must be absent or empty. Render rejects
symlinked destinations, symlinked ancestors and parent (`..`) components.
An I/O failure can leave partial output;
inspect it and use a fresh destination to retry.

The binary embeds its templates and does not need the source checkout or `gh`.
It writes only the CI workflow, Rust setup action, labels, issue forms and
ruleset fixture, with LF line endings. It does not execute configured commands,
call GitHub, compile gh-aw, render Intake or Build-cloud, or write ownership
records. The golden tests compare every output byte with the reviewed M2 files.
The bundled M2 assets remain fixed doctor fixtures; the two `.tmpl` assets
supply the config-dependent render text.

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
