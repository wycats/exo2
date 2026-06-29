# locald upgrade-order bug: `exo update` blocked by RFC reconciliation

## Summary

The `locald` repo hit an upgrade-order bug while dogfooding current `exo2` sidecar support. After sidecar bootstrap, `exo status` and `exo update` failed before the RFC metadata migration could add anchor comments to legacy RFC files.

This report is captured from downstream agent feedback. The core issue was that `exo update` loaded the full `AgentContext` before applying upgrade plugins, and full context load reconciled RFC metadata from disk into SQLite. RFC reconciliation required `<!-- exo:... ulid:... -->` anchor comments, so legacy unanchored RFCs made the update path fail before the migration designed to add those anchors could run.

This bug was repaired by PR #100 (`eeb1b65d`, `fix: let update migrate legacy RFC anchors before load`). Keep this report as the captured downstream feedback and as regression context for the follow-up upgrade-guidance tasks.

## Environment

- Repo: `locald`
- Branch: `pr/exosuit-bootstrap`
- Exo source: current `exo2` source checkout
- Install command:
  - `cargo install --path tools/exo --locked`
- Sidecar bootstrap command:
  - `exo sidecar bootstrap --key locald --root /home/dev/src/locald-sidecar`
- Sidecar state commit command:
  - `exo sidecar repo commit --message "Bootstrap Exosuit sidecar state"`

## Observed behavior

After sidecar bootstrap succeeded and `exo sidecar status` reported `Sidecar linked: locald`:

```sh
cd /home/dev/src/locald
exo status
# Error: Failed to reconcile RFC metadata from disk into SQLite

exo update
# Error: Failed to reconcile RFC metadata from disk into SQLite

exo rfc status
# rfc status: done

exo sidecar status
# Sidecar linked: locald
```

Additional observations:

- `git diff --check` passed.
- `exo rfc status` worked.
- The repo had many legacy RFC files without `<!-- exo:... ulid:... -->` anchors; the downstream agent counted about 135 unanchored RFC markdown files.

## Root cause

The failing order was:

1. `UpdateCommand::execute_mut()` called `AgentContext::load(ctx.root.to_path_buf())`.
2. `AgentContext::load_from_sqlite()` called RFC reconciliation via `reconcile_rfcs_once_with_project(root, project)`.
3. `parse_disk_rfc()` rejected legacy RFC files without anchor comments.
4. `MigrateRfcMetadataPlugin`, which was intended to add/migrate anchors, could not run because upgrade plugin execution happened after full context load.

Relevant code paths from the report:

- `tools/exo/src/context.rs`
  - `AgentContext::load_from_sqlite()` called `crate::rfc::reconcile_rfcs_once_with_project(root, project)`.
  - The error was wrapped as `Failed to reconcile RFC metadata from disk into SQLite`.
- `tools/exo/src/rfc.rs`
  - `parse_disk_rfc()` bailed when `!has_anchor(&content)` with `RFC file missing anchor comment: ...`.
- `tools/exo/src/command/update.rs`
  - `UpdateCommand::execute_mut()` loaded full context before `UpgradeRegistry::apply_all()`.
- `tools/exo/src/upgrade/plugins/migrate_rfc_metadata.rs`
  - `MigrateRfcMetadataPlugin` was the migration that should add anchors and migrate metadata.

## Required behavior

For a repo with legacy unanchored RFC markdown files:

- `exo update` succeeds and applies the RFC metadata migration.
- After `exo update`, `exo status` succeeds.
- `exo sidecar bootstrap` followed by `exo status` works without manually editing RFC files.
- If reconciliation still fails, the failure names the RFC file that caused it.
- Agents are not required to perform manual bulk rewrites before the migration can run.

## Resolution status

Resolved in current `main` by PR #100:

- Commit: `eeb1b65d`
- Title: `fix: let update migrate legacy RFC anchors before load (#100)`
- Key regression file: `tools/exo/tests/update_migrates_legacy_rfc_metadata.rs`

The task that follows this report should use the captured feedback to verify or extend regression coverage around unanchored RFC update behavior.
