# Project brief

Build `acme`, a single Rust binary that configures a software factory for a
GitHub repository. Its users are maintainers who want explicit specs, repeatable
workflows and human-controlled automation.

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
budgets at this stage. Keep agent workflows
disabled and staged, leave assignment off while its PAT is absent, and do not
start a paid agent run during bootstrap.

Do not implement the CLI or a second template repository in M0. Defer holdout,
build-local, automated review, release and ops. Build schema, doctor, then the
renderer. The renderer must reproduce the hand-built factory with zero diff.
