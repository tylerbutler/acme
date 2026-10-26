# Contributing

Read `AGENTS.md` and the applicable spec before making a change. Keep each PR
small and limited to one goal. Run `just check` before opening the PR.

Use Conventional Commits. `commit-types.json` defines the supported types.
After changing it, run `just generate-configs`. Do not edit generated
`cliff.toml` or `.commitlintrc.json` directly.

The factory files are hand-built golden fixtures during bootstrap. Change
`factory/factory.toml` and the matching fixtures together. No renderer exists yet.
Do not enable an agent workflow or create a secret as part of an unrelated PR.

Only the repository owner may add `ready-for-agent`. That label is approval
to build a specified change, not approval to merge it.
