# Changelog

All notable changes to this project will be documented in this file.

## [unreleased]

### Features

- Add `tracking` and `task` issue-role labels, a tracking form and role guidance
  to the managed issue forms.

### Bug Fixes

- Prepare version 0.4.0 with scoped actionlint compatibility for valid generated
  `concurrency.queue` and `job.workflow_ref` syntax, while preserving unrelated
  lint failures.
- Include stdout and stderr diagnostics when compilation or validation fails.
- Initialize an isolated Git repository for workflow compilation so generated
  failure-issue expiry matches the reviewed locks. Enabled workflow compilation
  now requires Git 2.28 or later.

## [0.3.0] - 2026-10-30

### Features

- Prepare version 0.3.0 with deterministic Intake and Build-cloud source plans,
  explicit workflow compilation, and validated compiled inputs for render and sync.
- Add optional controller and detection model settings, separate from the
  assigned builder model.
- Extend read-only doctor checks to agent workflow files, guards, activation
  settings and credential-name metadata.

### Documentation

- Document the owner-run M5 release and live adoption procedure. Release
  preparation does not change the live factory pin or adopt its agent files.
