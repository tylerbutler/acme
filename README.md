# acme

A deterministic software-factory CLI, written in Rust. The binary will read
`factory/factory.toml` and render the workflows and guardrails a project needs.
It will not call an LLM. An agent skill will conduct the init interview.

**Status: M0 bootstrap.** The factory is hand-built. `init`, `doctor`, `render`
and `sync` do not exist yet. The binary exits with an explicit error.

## Development

```sh
just build
just check
```

Rust is pinned in `rust-toolchain.toml`. Python 3 is used for the bootstrap
and commit-config scripts. `just check` runs formatting, Clippy, config checks,
and tests.

## The first factory

| File | Purpose |
| --- | --- |
| `factory/brief.md` | Project goals and constraints |
| `factory/factory.toml` | Human-approved draft; no CLI reads it yet |
| `AGENTS.md` | Agent instructions and risk boundaries |
| `.github/workflows/ci.yml` | Required `CI` check: fmt, Clippy and tests |
| `.github/workflows/intake.md` | Staged issue intake |
| `.github/workflows/build-cloud.md` | Owner-authorized Copilot assignment |
| `.github/labels.yml` | Issue state labels |
| `.github/rulesets/main.json` | Main ruleset: PR, zero approvals, required CI |
| `docs/specs/0001-bootstrap.md` | M0 scope, gates and acceptance criteria |
| `docs/adr/0001-factory-boundaries.md` | Architecture and bootstrap exceptions |

Both agent workflows are disabled in GitHub and gated by repository variables.
Their safe outputs are staged. **Staged mode still uses AI credits if a workflow
runs.** No monthly budget or AI-credit cap is configured, by owner decision.
`GH_AW_AGENT_TOKEN` is not configured. See
[the bootstrap guide](docs/bootstrap.md) before enabling anything.

M0's live issue-to-agent-PR acceptance check is deliberately not run. Holdout,
local builds, automated review, release and ops are deferred.

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
