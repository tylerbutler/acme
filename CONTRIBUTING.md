# Contributing

Read `AGENTS.md` and the applicable spec before making a change. Keep each PR
small and limited to one goal. Run `just check` before opening the PR.

## Local development prerequisites

Install these tools before you start:

- Git.
- Rust via rustup. `rust-toolchain.toml` pins Rust `1.85` and the `rustfmt`,
  `clippy`, and `llvm-tools-preview` components. rustup uses this toolchain
  when you run Rust commands in the repository.
- Python 3.
- just, to run the recipes in `justfile`.

Clone the repository and run the local checks:

```sh
git clone https://github.com/tylerbutler/acme.git
cd acme
just check
```

`just check` runs formatting checks, Clippy, config consistency checks, and
Rust and Python tests. These ordinary local checks do not require a PAT,
repository secrets, or Copilot access.

GitHub CLI with the gh-aw extension pinned to `v0.93.5` and actionlint are
needed only to recompile agent workflows with `just workflows`, not for
`just check`. See [Recompile workflow fixtures](docs/bootstrap.md#recompile-workflow-fixtures)
for the existing setup commands.

## Making changes

Use Conventional Commits. `commit-types.json` defines the supported types.
After changing it, run `just generate-configs`. Do not edit generated
`cliff.toml` or `.commitlintrc.json` directly.

The factory files are hand-built golden fixtures during bootstrap. Change
`factory/factory.toml` and the matching fixtures together. No renderer exists yet.
Do not enable an agent workflow or create a secret as part of an unrelated PR.

Only the repository owner may add `ready-for-agent`. That label is approval
to build a specified change, not approval to merge it.
