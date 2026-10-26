# ADR 0001: deterministic engine, replaceable agents

Status: accepted for bootstrap.

## Decision

Use a Rust binary for deterministic configuration and rendering. It never calls
an LLM. Agent skills draft config and specs; a human approves them before the
binary writes project files.

`factory/factory.toml` is the desired source of truth. During M0 it is a draft
and fixture, not a parsed contract. Hand-built workflows, labels, ruleset and
agent instructions define the output the later renderer must reproduce.
Modules must own their outputs so removing one cannot delete unrelated files.

Use issues and labels as shared state. One system owns each responsibility.
Use Copilot for all stages by owner decision. The later reviewer must use a
different model from the builder. Review is not enabled in M0.

Use L3 and human merge for all paths. Keep schema, modules and the renderer
under human merge even if later profiles allow lower-risk auto-merge.
Solo mode needs zero required approvals because an owner cannot approve a
requested agent PR. Require CI and resolved threads instead.

Keep agent jobs read-only and route writes through staged safe outputs.
Keep workflows disabled until the owner approves credentials and paid testing.
The owner explicitly declined monthly budgets and credit caps at bootstrap;
this is a deviation from the proposed cost-cap default, not a free-run claim.

Holdout scenarios will live in a separate private repository. No holdout repo
or token is provisioned in M0. Release, ops and local orchestration are also off.
The first tagged CLI release will be built by hand; later projects pin a release,
not HEAD. No usable stage0 release exists yet.

## Consequences

The factory can host work on its own CLI before that CLI exists. Adoption needs
an exact renderer golden check, a passing doctor check and zero-diff sync.
The draft schema and manual provisioning tool may change at M1 and M2.
Path restrictions, model separation and cloud runtime limits remain explicit
unimplemented gates; documentation must not present them as enforced.
