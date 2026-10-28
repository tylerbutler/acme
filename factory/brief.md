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
budgets at this stage. The owner approved leaving Intake and Build-cloud enabled
with live safe outputs and both variable gates on. Qualifying events can use
paid inference. Only the owner can approve a build, with the ready label or
by direct Copilot assignment; human merge is still required.

Do not implement the CLI or a second template repository in M0. Defer holdout,
build-local, automated review, release and ops. Build schema, doctor, then the
renderer. The renderer must reproduce the hand-built factory with zero diff.

## Current priority

Schema and doctor exist. The next work is M3: render, sync and version pins.
The renderer is the product; finish it before other milestones.

Automatic dispatch (specs 0010 and 0011) is paused. It saves one owner label
per task but adds a large trust and maintenance surface. The owner-applied
`ready-for-agent` label and direct owner assignment stay the build approvals. Revisit automatic
dispatch only after M5, and only if manual labeling is a real bottleneck.
Start with the simplest exclusion that works, such as an Actions concurrency
group, before a signed journal.
