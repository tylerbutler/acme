# Project brief

Acme is a software factory for GitHub repositories. Its next job is to help
the owner of acme itself decide whether a PR meets its approved issue without
having to reconstruct the builder's work. The existing Rust CLI configures
the factory; the next iteration need not change the binary.

The binary reads approved TOML and performs deterministic work. It never calls
an LLM. An agent skill conducts the interview and drafts config. Durable assets
are specs, tests, scenarios, skills and the agent map; agent engines are replaceable.

The first project is `tylerbutler/acme`, a public CLI repository. Bootstrap the
minimum loop by hand: issue forms, labels, CI, ruleset, staged intake and
owner-authorized cloud assignment. Use Copilot for all agent stages. The later
review stage must use a different model from the builder.

Use L3 autonomy. A human approves the config, adds `ready-for-agent`, reviews
changes and merges. The schema, modules and renderer always need human merge.
Secrets and private holdout scenarios must never be changed or exposed by agents.

Use a 5000-AI-credit cap per main agent run. Do not configure monthly or daily
budgets at this stage. The owner approved leaving Intake and Build-cloud enabled
with live safe outputs and both variable gates on. Qualifying events can use
paid inference. Only the owner can approve a build, with the ready label or
by direct Copilot assignment; human merge is still required.

Do not implement the CLI or a second template repository in M0. Defer holdout,
build-local, automated review, release and ops. Build schema, doctor, then the
renderer. The renderer must reproduce the hand-built factory with zero diff.

## Current priority

Start with a [manual PR review pilot](../docs/specs/0017-pr-review-handoff.md).
The builder uses the approved issue's acceptance scenarios to plan checks,
then shows the owner what those checks established and what remains unknown.
Use ordinary approved work and the existing acceptance record to find out
whether this saves owner review time without hiding missed criteria.

Maintain the installed CLI and workflows. Pause
[preset expansion](../docs/specs/0013-builtin-factory-presets.md); keep
automatic dispatch and M6–M8 deferred. The pilot is human review, not the
automated Review stage. If a template paragraph and builder instruction
are enough, keep them without adding software.
