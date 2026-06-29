# Migration-aware upgrade guidance inventory

## Purpose

This inventory captures the downstream `locald` upgrade-order feedback and turns it into actionable requirements for the `Migration-Aware Upgrade Guidance` phase.

## Source feedback

Primary report: `docs/bug-reports/locald-upgrade-order.md`.

The report came from dogfooding sidecar support in the downstream `locald` repository. The user-facing failure was not that migration code was absent; the failure was that the migration path was blocked before the migration could run.

## Affected downstream setup

- Repository: `locald`
- Branch: `pr/exosuit-bootstrap`
- Exo source: current `exo2`
- Installation path: `cargo install --path tools/exo --locked`
- Sidecar bootstrap:
  - `exo sidecar bootstrap --key locald --root /home/dev/src/locald-sidecar`
- Sidecar state commit:
  - `exo sidecar repo commit --message "Bootstrap Exosuit sidecar state"`

## Observed failure sequence

After sidecar bootstrap succeeded and `exo sidecar status` reported `Sidecar linked: locald`:

1. `exo status` failed.
2. `exo update` failed.
3. Both failures surfaced as `Failed to reconcile RFC metadata from disk into SQLite`.
4. `exo rfc status` still worked.
5. `exo sidecar status` still worked.
6. The repo contained many legacy RFC markdown files without Exosuit anchor comments.

The important dogfood signal is that the commands an agent naturally tries first, `exo status` and `exo update`, were the commands that failed. The fallback commands that still worked (`exo rfc status`, `exo sidecar status`) were not obvious from the failure output.

## Root cause recorded by the report

The failing ordering was:

1. `UpdateCommand::execute_mut()` loaded the full `AgentContext`.
2. Full context load called `AgentContext::load_from_sqlite()`.
3. SQLite load reconciled RFC metadata from disk via `reconcile_rfcs_once_with_project(root, project)`.
4. RFC disk parsing rejected legacy RFC files without `<!-- exo:... ulid:... -->` anchors.
5. The RFC metadata migration that should add anchors could not run because upgrade plugins executed after full context load.

Relevant code paths named in the report:

- `tools/exo/src/context.rs`
  - `AgentContext::load_from_sqlite()`
  - `reconcile_rfcs_once_with_project(root, project)`
- `tools/exo/src/rfc.rs`
  - `parse_disk_rfc()`
  - `RFC file missing anchor comment`
- `tools/exo/src/command/update.rs`
  - `UpdateCommand::execute_mut()`
- `tools/exo/src/upgrade/plugins/migrate_rfc_metadata.rs`
  - `MigrateRfcMetadataPlugin`

## Current resolution baseline

The immediate bug is recorded as resolved by PR #100:

- Commit: `eeb1b65d`
- Title: `fix: let update migrate legacy RFC anchors before load (#100)`
- Regression file: `tools/exo/tests/update_migrates_legacy_rfc_metadata.rs`

The current phase should not reopen that bug as unfixed. The task is to turn the feedback into migration-aware guidance so similar upgrade blockers are easy to diagnose and recover from.

## Guidance requirements extracted from the feedback

### 1. Pre-load migration blockers must be recognizable

If a command cannot load full context because a known migration must run first, the user-facing error should classify the situation as an upgrade blocker, not as a generic internal failure.

Minimum fields to surface:

- failing subsystem (`rfc metadata reconciliation`)
- concrete cause (`legacy RFC file missing anchor comment`)
- affected file path when available
- command that can recover (`exo update` when applicable)
- fallback orientation command if update itself cannot run

### 2. `exo update` must remain the front door for migration recovery

Agents should be able to try `exo update` without first knowing which migration plugin exists. If `exo update` needs a reduced-context path, it should say so in code and tests.

### 3. Sidecar bootstrap should steer post-bootstrap validation

After sidecar bootstrap, guidance should explicitly tell agents what to run next and how to interpret failures:

- `exo status`
- `exo update` if status reports upgrade-needed or migration-blocked state
- `exo sidecar status` for sidecar binding health

### 4. Partial command availability should be treated as a diagnostic signal

The `locald` report showed that `exo rfc status` and `exo sidecar status` still worked while full context commands failed. Guidance should use that pattern:

- full context failure + rfc status works → likely full-load reconciliation/order problem
- sidecar status works → sidecar binding is intact; focus on state upgrade/reconciliation

### 5. Regression coverage must preserve the downstream shape

Regression fixtures should model the downstream conditions:

- git-backed repo
- sidecar or sidecar-like project policy when relevant
- legacy RFC markdown without anchor comments
- missing or stale SQLite state where the migration is supposed to repair metadata
- `exo update` succeeds before full `exo status` is expected to succeed

## Signals to carry into the next task

The next task, `migration-upgrade-audit-feedback::map-update-ordering`, should verify the current command ordering against this inventory:

1. Which commands load full `AgentContext` before dispatch?
2. Which commands have reduced-context or project-only paths?
3. Where does `exo update` bypass full RFC reconciliation today?
4. Which error paths still collapse known migration blockers into generic failures?
5. Which steering messages already recommend the right recovery commands?

## Non-goals for this phase

- Do not build GitHub profile sidecar discovery here.
- Do not bulk-edit downstream `locald` RFC files.
- Do not redesign RFC metadata anchors.
- Do not treat direct SQL dumps as a human read interface.

## Inventory conclusion

The `locald` failure is a migration-order and guidance problem: adoption of an existing repo exposed a state where the repair command was blocked by the thing it needed to repair. PR #100 fixed the concrete RFC-anchor ordering bug. This phase should make that class of failure discoverable, actionable, and regression-covered so agents can recover migrated projects without manual archaeology.

## Current update-ordering map

### CLI bootstrap and dispatch order

Current CLI startup in `tools/exo/src/main.rs` classifies several commands as direct/reduced-context before normal dispatch:

1. `project resolve`
2. any `sidecar ...` command
3. `update`

Those commands build an `AgentContext` manually from `Project::resolve(&cwd).ok()` and an empty `ExoState` instead of calling `load_context_or_exit()`. Every other direct CLI command calls `load_context_or_exit()`, which calls `AgentContext::load(cwd)` and therefore full SQLite load plus RFC reconciliation.

Daemon mode follows the same high-level rule because `is_update_command(&args)` contributes to `is_direct`; `exo update` is forced into direct local execution rather than daemon dispatch. Normal machine-channel requests are handled by `tools/exo/src/api/handler.rs`, which builds a command and invokes it through `invoke_command_box_json()` without preloading full `AgentContext`; individual command implementations decide whether to call `AgentContext::load()`.

### `exo update` current ordering

`tools/exo/src/command/update.rs` now avoids the historical pre-migration full load:

1. `UpdateCommand::execute_mut()` calls `load_update_context(ctx.root.to_path_buf(), ctx.project)`.
2. `load_update_context()` resolves project policy, calls `ensure_update_database()`, and returns an `AgentContext` with empty `ExoState`.
3. `ensure_update_database()` validates that the workspace has an Exosuit marker (`exosuit.toml`, SQLite database, or SQL projection files).
4. If SQL projections exist but the DB does not, `ensure_update_database()` imports SQL dumps before upgrades run.
5. `apply_upgrades()` runs `UpgradeRegistry::apply_all(context)`.
6. Only after upgrades run does `reload_after_upgrade()` call full `AgentContext::load()`.

This is the key ordering invariant from PR #100: migrations that repair load blockers can run before full context/RFC reconciliation.

### Full context load ordering

`AgentContext::load()` still means full context load:

1. Resolve project identity and state root.
2. Import SQL dumps if needed.
3. Open SQLite.
4. Run `reconcile_rfcs_once_with_project(root, project)`.
5. Load state from SQLite.

Any command path that calls `AgentContext::load()` before applying relevant migrations can still reproduce the class of failure seen in `locald`.

### Reduced-context paths available today

These paths avoid full `AgentContext::load()` at the CLI front door:

- `exo project resolve`
- `exo sidecar ...`
- `exo update`
- `exo init`
- daemon bootstrap commands (`exo daemon run`, `exo daemon ensure`)

Additional partial availability from the `locald` report:

- `exo rfc status` worked even while full context commands failed.
- `exo sidecar status` worked and confirmed sidecar binding health.

### Remaining ordering risks

The broad grep for `AgentContext::load(` shows many command implementations still perform full context load internally. That is correct for normal operations, but it matters for migration guidance:

- `status`, `phase`, `goal`, `task`, `plan`, and many write commands can fail if full context load is blocked.
- Machine-channel operation handling does not preload full context globally, but command implementations can still fail by calling `AgentContext::load()`.
- Error rendering in `load_context_or_exit()` still collapses full-load failures into a generic `Failed to load agent context: ...` envelope or fatal error. It does not classify known migration blockers yet.

### Steering already in place

Known helpful steering today:

- `exo update` has a workspace guard that tells users to run `exo init` when no Exosuit marker exists.
- `exo update` is direct/reduced-context, preserving it as the recovery front door for load-blocking migrations.
- `exo sidecar status` remains a reduced-context diagnostic for sidecar binding health.

### Guidance gap to design next

The design task should focus on classifying full-load failures and choosing recovery copy. The decision table needs at least these cases:

| Observed condition | Meaning | Guidance |
| --- | --- | --- |
| full context load fails with RFC reconciliation / missing anchor | known migration blocker | run `exo update`; if update fails, show offending RFC path and fallback diagnostics |
| `exo update` succeeds | migration applied | rerun `exo status` |
| `exo update` says no workspace marker | not initialized | run `exo init` or sidecar bootstrap first |
| `exo sidecar status` succeeds while status fails | sidecar binding intact | focus on state upgrade/reconciliation |
| SQL projection exists but DB missing | fresh clone/import path | `exo update` imports SQL dumps before upgrades |

The implementation tasks should avoid adding another broad pre-command full load. Recovery guidance must be available at the same boundary where context load fails.

## Upgrade guidance decision table

The guidance surface has two distinct entry points:

1. **Pre-dispatch/full-load failure**: `load_context_or_exit()` or an equivalent command-level `AgentContext::load()` call fails before normal command execution can produce status/map steering.
2. **Post-load upgrade gate**: full context loaded successfully and `UpgradeRegistry::check_all()` reports critical upgrades through existing status/map steering.

The decision table below separates those paths so implementation does not introduce another broad full-context load before the recovery command can run.

| Case | Signals | Classification | User-facing guidance | Machine/structured guidance | Regression expectation |
| --- | --- | --- | --- | --- | --- |
| Workspace is not initialized | no `exosuit.toml`; no SQLite DB; no SQL projection files | `not_initialized` | This directory is not an Exosuit workspace. Run `exo init` for inline state or sidecar bootstrap for sidecar state. | next call: `exo init`; repair call: `exo sidecar bootstrap` when project policy is sidecar-relevant | `exo update` refuses the directory and does not create a DB |
| Full load fails in RFC reconciliation because an RFC lacks an anchor | error chain contains `Failed to reconcile RFC metadata from disk into SQLite` and `RFC file missing anchor comment` | `migration_blocked:rfc_metadata_anchor` | Legacy RFC metadata must be migrated before this command can load context. Run `exo update`, then rerun the original command. Include the offending RFC path when the error chain contains it. | next call: `exo update`; confidence `1.0`; include subsystem `rfc metadata reconciliation`, cause `missing anchor comment`, affected path when available | unanchored RFC fixture: `AgentContext::load()` fails, `exo update` succeeds, then `exo status` succeeds |
| Full load fails in RFC reconciliation for a non-anchor parse error | error chain contains `Failed to reconcile RFC metadata from disk into SQLite` but not the missing-anchor signature | `rfc_reconciliation_failed` | RFC reconciliation failed. Show the concrete parse error and file path. Run `exo update` only when the message matches a known migration blocker; otherwise inspect the named RFC file. | next call: `exo rfc status`; repair call: `exo update` only if classifier marks the error migration-recoverable | fixture with malformed RFC produces file-specific guidance, not anchor-migration copy |
| Full load fails opening SQLite | error chain contains `Failed to open SQLite database` | `state_store_unavailable` | The SQLite state store could not be opened. Show the DB path and underlying SQLite error. Do not present this as an RFC migration. | next call: `exo sidecar status` for sidecar projects; otherwise `exo update` only if SQL projection import is possible | locked/corrupt DB fixture produces state-store guidance |
| SQL projections exist but DB is missing | `docs/agent-context/*.sql` exists and DB path does not exist | `sql_projection_import_needed` | State exists as SQL projections. Run `exo update`; it imports SQL before applying upgrades. | next call: `exo update`; include DB path and SQL projection directory | fresh-clone projection fixture: `exo update` imports SQL and then status succeeds |
| Full context loads and critical upgrades are reported | `UpgradeRegistry::check_all()` returns critical upgrades | `upgrade_required` | Critical upgrade required. Run `exo update` before continuing. Preserve existing status/map copy. | existing `upgrade_required_steering()` with next call `exo update` | status/map JSON returns upgrade steering without executing writes |
| Full context loads and warning upgrades are reported | `UpgradeRegistry::check_all()` returns warning upgrades only | `upgrade_recommended` | Continue normal read-only orientation, but surface `exo update` as recommended maintenance. Writes follow existing upgrade-gate policy. | status/map repair action or reminder, not a hard block | warning upgrade fixture preserves normal status visibility |
| `exo update` succeeds | update summary reports applied or skipped upgrades without error | `migration_recovered` | Upgrade finished. Rerun the original command, normally `exo status`. | next call: original command when known; otherwise `exo status` | recovery fixture verifies status succeeds after update |
| `exo update` fails with workspace guard | update output says no Exosuit marker exists | `not_initialized` | This directory is not initialized. Run `exo init` or sidecar bootstrap; do not edit RFC files manually. | next call: `exo init`; no DB side effects | non-workspace update test asserts no DB creation |
| `exo update` fails while applying RFC metadata migration | error chain names `migrate-rfc-metadata-v1` or verification reports `missing anchor comment` | `migration_apply_failed:rfc_metadata` | The RFC metadata migration started but did not complete. Show plugin id and all named RFC files. Rerun `exo update` after fixing the listed files or file a bug if the files are valid legacy RFCs. | next call: `exo update`; diagnostic call: `exo rfc status`; include plugin id | fixture with intentionally unparseable RFC reports plugin/file-specific failure |
| `exo sidecar status` succeeds while full status fails | sidecar status reports linked/healthy; status fails during load | `sidecar_ok_state_blocked` | Sidecar binding is healthy. Do not rerun bootstrap. Run `exo update` and focus on state/RFC reconciliation. | repair call: `exo update`; diagnostic context: sidecar linked | sidecar fixture preserves sidecar health while update repairs state |
| `exo sidecar status` fails | sidecar status reports missing/unlinked/broken binding | `sidecar_binding_failed` | Fix sidecar binding before state migration guidance. Use sidecar repair/bootstrap commands from sidecar output. | next call from sidecar command output, not migration classifier | sidecar failure does not get mislabeled as RFC migration |
| `exo rfc status` works while full status fails | RFC command can inspect files without full context; full status fails | `partial_rfc_diagnostic_available` | RFC inspection is still available, but recovery remains `exo update` for known anchor blockers. | diagnostic call: `exo rfc status`; next call: `exo update` for anchor blockers | locald-shaped fixture records this as orientation evidence |

### Classifier rules

Implementation should use deterministic error-chain matching, not broad substring guesses in final copy:

1. Walk the full `anyhow` error chain.
2. Match the outer subsystem wrapper first (`Failed to reconcile RFC metadata from disk into SQLite`, `Failed to open SQLite database`, `Failed to load state from SQLite database`).
3. Match known recoverable causes second (`RFC file missing anchor comment`).
4. Extract the affected path from the deepest error string when present.
5. Emit generic subsystem guidance only when the cause is unknown.

Known migration-blocked guidance must name `exo update` because `update` is already a reduced-context direct path. Unknown reconciliation failures must not claim that `exo update` repairs them.

### Copy requirements for the next task

Every surfaced guidance message must include:

- what failed (`rfc metadata reconciliation`, `SQLite open`, `state load`, or `upgrade gate`)
- why it failed in user terms
- the next command to run
- the original command to retry when recovery succeeds
- a diagnostic command when recovery does not apply (`exo rfc status` or `exo sidecar status`)

The primary migration-blocked copy should be:

> Exosuit needs to migrate legacy RFC metadata before this command can load workspace context. Run `exo update`, then rerun the original command.

If an RFC path is available, append:

> First failing RFC: `<path>`

The non-workspace copy should remain separate:

> This directory is not an Exosuit workspace. Run `exo init` to initialize it, or run sidecar bootstrap if this project should store Exosuit state in a sidecar.

## User-facing steering copy spec

### Shape of surfaced guidance

Migration guidance should have the same conceptual shape in human CLI output, JSON error envelopes, and machine-channel steering:

- **Headline**: one sentence naming the classified problem.
- **Cause**: one sentence explaining the user-level reason.
- **Next**: the command to run now.
- **Retry**: the original command to rerun after recovery.
- **Diagnostics**: one optional command for orientation when recovery does not apply.

The JSON/machine shape should use `ErrorCode::PreconditionFailed` when the classifier finds a known recoverable pre-load migration blocker. Unknown failures should keep `ErrorCode::Internal` or the existing subsystem-specific error code until classified.

Machine-channel `Steering` should set:

- `next_call.kind = Call` for `exo update` recovery.
- `priority = Blocking` for known pre-load migration blockers.
- `confidence = 1.0` only for deterministic known blockers.
- `context_note` to the short headline plus retry instruction.

The rich decision-table fields should live in `error.details`, not only in prose:

```json
{
  "classification": "migration_blocked:rfc_metadata_anchor",
  "subsystem": "rfc metadata reconciliation",
  "cause": "legacy RFC file missing anchor comment",
  "affected_path": "docs/rfcs/stage-1/0001-legacy-rfc.md",
  "next_command": "exo update",
  "retry_command": "exo status",
  "diagnostic_command": "exo rfc status"
}
```

### Primary copy blocks

#### Known RFC metadata migration blocker

Headline:

> Workspace context is blocked by a legacy RFC metadata migration.

Cause:

> Exosuit found an RFC file that still needs the required anchor comment before full workspace context can load.

Next:

> Run `exo update` to migrate RFC metadata, then rerun `<original command>`.

Diagnostic:

> If update fails, run `exo rfc status` to inspect RFC metadata state.

Path addendum when available:

> First failing RFC: `<path>`

Human CLI rendering:

```text
Workspace context is blocked by a legacy RFC metadata migration.

Exosuit found an RFC file that still needs the required anchor comment before full workspace context can load.

[Next]
- exo update

[Then]
- rerun <original command>

[Diagnostic]
- exo rfc status
```

#### Unknown RFC reconciliation failure

Headline:

> Workspace context failed during RFC metadata reconciliation.

Cause:

> Exosuit could not reconcile RFC files into SQLite. This is not classified as a known migration blocker.

Next:

> Inspect the named RFC error, then rerun `<original command>`.

Diagnostic:

> Run `exo rfc status` for RFC-specific diagnostics.

Do not say `exo update` fixes this case unless the known migration-blocker classifier matched.

#### SQLite state store unavailable

Headline:

> Workspace context could not open the SQLite state store.

Cause:

> Exosuit could not open the state database at `<db path>`.

Next:

> Resolve the SQLite error, then rerun `<original command>`.

Diagnostic:

> For sidecar workspaces, run `exo sidecar status` to confirm the state root binding.

This copy must not mention RFC anchors or RFC metadata migration.

#### SQL projection import needed

Headline:

> Workspace state needs to be imported from SQL projections.

Cause:

> SQL projection files exist but the SQLite database is missing.

Next:

> Run `exo update` to import SQL projections and apply migrations, then rerun `<original command>`.

Diagnostic:

> If update fails, include the SQL projection directory and database path in the error details.

#### Not initialized

Headline:

> This directory is not an Exosuit workspace.

Cause:

> Exosuit found no workspace marker, state database, or SQL projections.

Next:

> Run `exo init` to initialize inline state, or run sidecar bootstrap if this project should use sidecar state.

This is not an upgrade failure and should not recommend editing RFC files.

#### Post-load critical upgrade gate

Headline:

> Critical Exosuit upgrade required.

Cause:

> One or more critical migrations must run before work can proceed.

Next:

> Run `exo update`.

This should preserve the existing `upgrade_required_steering()` behavior and only tighten copy where needed.

#### Sidecar healthy, state blocked

Headline:

> Sidecar binding is healthy; workspace state is blocked.

Cause:

> `exo sidecar status` succeeded, so the failure is in state loading or migration, not sidecar binding.

Next:

> Run `exo update`, then rerun `<original command>`.

Diagnostic:

> Keep `exo sidecar status` as a secondary diagnostic, not a bootstrap instruction.

### Copy constraints

- Do not tell users to bulk-edit managed RFC files manually for known migration blockers.
- Do not suggest `exo init` when an Exosuit marker or sidecar state is already present.
- Do not call SQL dumps a source of truth; say “SQL projections”.
- Do not conflate sidecar binding failures with state migration failures.
- Do not hide the original underlying error; append it after the guidance or include it in structured details.
- Do not use hedging. Classified blockers are blockers; unknown failures are unknown failures.

### Implementation handoff

The next implementation task should add a small classifier at the context-load boundary and route `load_context_or_exit()` failures through it. The classifier should return a structured guidance object that can render both human CLI text and JSON/machine `Steering`. Command-level `AgentContext::load()` failures can adopt the same helper after the front-door path is covered.
