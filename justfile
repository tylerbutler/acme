#!/usr/bin/env just --justfile

alias b := build
alias t := test
alias l := lint
alias f := format
alias c := check

export RUST_BACKTRACE := "1"

default:
    @just --list

build:
    cargo build --locked

test:
    cargo test --locked --all-features
    python3 -m unittest discover -s tests -p 'test_*.py'

lint:
    cargo clippy --locked --all-targets --all-features -- -D warnings

format:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

generate-configs:
    python3 scripts/generate-cliff-configs.py
    python3 scripts/generate-commitlint-config.py

check-configs:
    python3 scripts/check-configs-sync.py

generate-schema:
    cargo run --locked --bin generate-schema

check-schema:
    cargo run --quiet --locked --bin generate-schema -- --check

check: fmt-check lint check-configs check-schema test

ci: check

# Requires github/gh-aw pinned to v0.93.5.
workflows:
    test "$(gh aw version)" = "gh aw version v0.93.5"
    gh aw compile --no-check-update --actionlint

# Requires local pinned compiler and runtime paths; does not run a workflow.
prove-assignment:
    ACME_ASSIGNMENT_PROOF=1 python3 -m unittest discover -s tests -p test_assignment_handoff.py -v

# Applies the hand-built GitHub settings; never enables an agent.
bootstrap-github:
    python3 scripts/bootstrap-github.py
