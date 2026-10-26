# acme

A deterministic software-factory CLI, written in Rust. The binary will read
`factory/factory.toml` and render the workflows and guardrails a project needs.
It will not call an LLM. An agent skill will conduct the init interview.

**Status: M0 factory loop verified.** The factory is hand-built. `doctor` runs
read-only checks against the local module assets and the GitHub repository named
in the config. It requires an authenticated `gh` CLI. `init`, `render` and `sync`
are not implemented and fail explicitly.

## Development

```sh
just build
just check
cargo run -- doctor
cargo run -- doctor --config path/to/factory.toml
```

Rust is pinned in `rust-toolchain.toml`. Python 3 is used for the bootstrap
and commit-config scripts. `just check` runs formatting, Clippy, config checks,
and tests. Doctor reports drift and repair guidance but does not change local
files or GitHub settings. It does not verify token scopes, enforce path-author
policy, or enforce model separation. `doctor --fix` is not implemented.

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
| `factory/factory.toml` | Human-approved config read and validated by doctor |
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
specified issue. **These events can use paid AI credits.** Each main agent run
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
