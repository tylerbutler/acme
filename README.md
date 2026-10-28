# acme

A deterministic software-factory CLI, written in Rust. The binary reads
`factory/factory.toml` and renders the supported M2 workflows and guardrails.
It does not call an LLM. An agent skill will conduct the init interview.

**Status: M0 factory loop verified.** The factory is hand-built. `doctor` runs
read-only checks against the local module assets and the GitHub repository named
in the config. It requires an authenticated `gh` CLI. `render` writes the
deterministic M2 inventory to a new or empty directory. `init` and `sync`
are not implemented and fail explicitly. Automatic dispatch is paused.

## Development

```sh
just build
just check
cargo run -- doctor
cargo run -- doctor --config path/to/factory.toml
cargo run --bin acme -- render --output /tmp/acme-render
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
edit until M3. Unrelated labels and rulesets remain unchanged. Duplicate managed
rulesets need manual resolution.

Fix does not provision secrets or change workflow enablement, variable gates,
staging, budgets, assignments or issue state. It never runs the bootstrap script.
See the [owner-only M2 acceptance procedure](docs/bootstrap.md#owner-only-m2-acceptance).
Implementation and fake-state tests do not establish the live M2 exit check.

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
