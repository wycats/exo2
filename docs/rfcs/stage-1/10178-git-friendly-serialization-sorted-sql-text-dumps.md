<!-- exo:10178 ulid:01kmzxey1yhcqa6qawdxcbvn8t -->

# RFC 10178: Git-Friendly Serialization: Sorted SQL Text Dumps

**Status**: Stage 1 (Proposal)

## Summary

Replace committed TOML files (`plan.toml`, `inbox.toml`, `ideas.toml`) with deterministic sorted SQL text dumps as the git-friendly representation of project steering state.

The SQLite database at `{state_root}/cache/exo.db` remains the canonical runtime store. The text dumps are a policy-controlled projection for version control and portability, not universal workspace-written state. Repo policy writes that projection into the workspace. Sidecar policy writes the same projection into a private portable sidecar.

**Extracted from**: RFC 10165 §7 (Reactive SQLite), which defines the reactive runtime. This RFC covers the git persistence concern separately because it has different dependencies, timeline, and design questions.

**Related RFCs**:

- RFC 10165: Reactive SQLite (runtime storage)
- RFC 10176: Project State Model (data model)
- RFC 10184: Project / Workspace / Worktree unbundling (resolved project database path)
- RFC 10208: Storage Writer Compatibility Fence (writer generation and import preflight)

## Motivation

SQLite is the runtime source of truth. But SQLite databases are binary files; they can't be meaningfully diffed, reviewed, or merged in git. The workspace needs a text-based representation that:

1. **Survives clones** — a fresh `git clone` can reconstruct the SQLite database;
2. **Produces meaningful diffs** — reviewers can see what changed;
3. **Merges cleanly** — concurrent changes to different entities don't conflict;
4. **Is deterministic** — the same database state always produces the same text output.

Earlier TOML files served this role. This RFC replaces that bridge with sorted SQL dumps.

## Design

### Runtime database path

The runtime database is the resolved project database from RFC 10184:

```text
{state_root}/cache/exo.db
```

For repo policy this is usually `<primary-workspace>/.exo/cache/exo.db`. For shadow policy this is `$HOME/.exo/projects/<project-id>/cache/exo.db`. For sidecar policy this is `$HOME/.exo/sidecars/<sidecar-key>/cache/exo.db` or the equivalent local materialization root selected by RFC 10184.

References in older text to checkout-local `.cache/exo.db` should be read as this resolved project database path.

### Format: Sorted SQL INSERT Statements

One INSERT statement per line, sorted by `text_id` (ULID) for deterministic output. The `id` column (rowid) is omitted because it is reassigned on import.

The required `epochs.sql` file is also the portable projection's compatibility
carrier. A fence-aware exporter writes this strict first line before any SQL
statement:

```sql
-- exo:minimum-writer-generation=<generation>
```

The decimal value is the semantic minimum writer generation defined by RFC
10208. It is shared with the canonical database's `PRAGMA user_version`, but it
is not a migration identifier and does not require contiguous schema history.
The generation is a canonical decimal in the shared non-negative signed-32-bit
range `0..=2_147_483_647`. It has no sign or surrounding whitespace and no
leading zero except the value zero itself. A missing header means legacy
generation zero. A line beginning with the Exo writer-generation prefix but not
matching that grammar is malformed projection metadata.

The header is format metadata rather than a database row. It is not sorted with
the INSERT statements, is not imported into `epochs_data`, and does not change
the deterministic ordering of the SQL body.

```sql
INSERT INTO goals_data(text_id, label, status, phase_text_id, slug) VALUES('01hz3kabcd', 'Fix RFC promotion', 'completed', '01kj5nc4zdkxcqba96mnbt1ynz', 'frontmatter-bug');
INSERT INTO goals_data(text_id, label, status, phase_text_id, slug) VALUES('01kh2hrefq', 'Implement validate', 'in-progress', '01kj5nc4zdkxcqba96mnbt1ynz', 'validate-cmd');
```

Diffs show exactly what changed:

```diff
-INSERT INTO goals_data(...) VALUES('01kh2hrefq', 'Implement validate', 'in-progress', ...);
+INSERT INTO goals_data(...) VALUES('01kh2hrefq', 'Implement validate', 'completed', ...);
```

### Foreign Key Resolution

Foreign key columns (`phase_id`, `epoch_id`, `goal_id`) are internal rowids that change on reimport. The dump emits the referenced entity's `text_id` instead.

| Table                | FK Column   | Emitted As       | Resolved By                   |
| -------------------- | ----------- | ---------------- | ----------------------------- |
| `phases_data`        | `epoch_id`  | `epoch_text_id`  | Lookup `epochs_data.text_id`  |
| `goals_data`         | `phase_id`  | `phase_text_id`  | Lookup `phases_data.text_id`  |
| `tasks_data`         | `goal_id`   | `goal_text_id`   | Lookup `goals_data.text_id`   |
| `phase_rfcs_data`    | `phase_id`  | `phase_text_id`  | Lookup `phases_data.text_id`  |
| `task_logs`          | `task_id`   | `task_text_id`   | Lookup `tasks_data.text_id`   |
| `task_verifications` | `task_id`   | `task_text_id`   | Lookup `tasks_data.text_id`   |
| `entity_aliases`     | `entity_id` | `entity_text_id` | Lookup by `entity_type` table |
| `idea_tags`          | `idea_id`   | `idea_text_id`   | Lookup `ideas_data.text_id`   |
| `idea_task_refs`     | `idea_id`   | `idea_text_id`   | Lookup `ideas_data.text_id`   |

The importer processes tables in dependency order and builds a `text_id → rowid` map as it goes.

### File Layout

Under repo policy, per-table `.sql` files are committed to git:

```text
docs/agent-context/
├── epochs.sql
├── phases.sql
├── goals.sql
├── tasks.sql
├── ideas.sql
├── inbox.sql
├── phase_rfcs.sql
├── entity_aliases.sql
├── idea_tags.sql
├── idea_task_refs.sql
├── task_logs.sql
└── task_verifications.sql
```

Shadow policy does not read from or write to these workspace dump files by default. Its state is private machine-local state in the project database.

Sidecar policy uses the same per-table SQL dump format as a private portable projection outside the work repository:

```text
{sidecar_root}/projects/{sidecar_key}/agent-context/
├── epochs.sql
├── phases.sql
├── goals.sql
├── tasks.sql
├── ideas.sql
├── inbox.sql
├── phase_rfcs.sql
├── entity_aliases.sql
├── idea_tags.sql
├── idea_task_refs.sql
├── task_logs.sql
└── task_verifications.sql
```

The sidecar projection is selected by the sidecar binding in RFC 10184. It is not written to `docs/agent-context` in the work repository.

### What Is NOT Serialized

- `id` (rowid) — reassigned on import
- `_rev` tables — recomputed on import
- `rowset_revisions` — recomputed on import
- `__schema_history` — managed by migration system
- `workspace_active_phase_data` — workspace-local runtime pin state from RFC 10184

### Workflow

After mutation:

```text
command → SQLite mutation → regenerate .sql files
```

Regeneration derives every file from one SQLite snapshot and obeys RFC 10208's
never-under-fenced publication order. When the generation rises, the strict
`epochs.sql` header is published before any file requiring the higher
generation. A proven downgrade publishes all downgraded bodies under the old
higher header and lowers the header last. Per-file replacement remains atomic;
this RFC does not claim directory-wide atomicity. An interrupted export may be
stale, partially advanced, or conservatively over-fenced, but it must never
advertise a generation lower than a visible file requires.

Fresh clone under repo policy:

```text
git clone → .sql files present → compatibility preflight → exo init/import → {state_root}/cache/exo.db rebuilt
```

The compatibility preflight reads the first header of `epochs.sql` before Exo
creates a state directory or database. A required generation newer than the
binary supports fails with `storage.writer_incompatible`; malformed generation
metadata fails distinctly. Only after the preflight succeeds does the importer
process the ordinary dependency-ordered table list and rebuild SQLite. Missing
metadata follows the legacy import path.

Shadow policy skips this workspace import/export path by default. A repository can contain `docs/agent-context/*.sql` for team state while an individual user runs the same checkout with private shadow state.

Sidecar policy follows the same mutation/export shape, but the projection directory is the sidecar projection:

```text
command → local sidecar-materialized SQLite mutation → regenerate sidecar agent-context/*.sql
```

Fresh checkout under sidecar policy:

```text
git clone → exo sidecar link --key <key> --root <sidecar-root> → sidecar .sql files present → compatibility preflight → local {state_root}/cache/exo.db rebuilt
```

If repo dumps and sidecar dumps both exist, the selected policy decides which projection is used. The importer does not merge them implicitly.

Merge behavior:

Since each line is one entity sorted by ULID, compatible changes to different
logical rows must be preserved automatically by the repo or sidecar projection
merge path. Edits to the same logical row require semantic comparison by table,
stable logical row id such as `text_id`, and field. Compatible field-level
changes may be merged; incompatible same-field edits become Exo conflicts.

The writer generation is typed merge metadata for both repo-policy and sidecar
projections. Sidecar merge is Exo-orchestrated: it preflights the base and every
input before mutating the sidecar checkout, SQLite cache, or output projection.
Unsupported or malformed metadata stops that merge before mutation.

Repo-policy files currently use Git's per-file merge machinery. That driver
cannot preflight the entire projection before Git mutates the worktree; the
stronger guarantee requires a future Exo-owned repository merge coordinator.
Until then, Exo rejects semantic projection reads, hydration, and export while
Git reports an unsettled merge operation or unresolved index conflicts. The
`epochs.sql` driver renders the maximum generation from base, ours, and theirs,
and Exo preflights the completed projection after Git settles and before any
semantic use. Git's transient unsettled worktree is quarantined merge input,
not a published semantic projection under the never-under-fenced contract.

For either policy, a completed compatible result uses at least the maximum
generation from the base and all inputs, raises it further if the resolved rows
require that, and always renders the strict `epochs.sql` header even when every
input was a headerless legacy projection. Ordinary merge or conflict resolution
cannot lower the generation; only RFC 10208's proven downgrade path may do so.

### Round-Trip Fidelity

The critical invariant: `SQLite → SQL dump → SQLite → identical state`. This must be tested as part of the migration.

Compatibility participates in that invariant. The export header and database
generation describe the same snapshot after a complete export. A failed export
may retain a conservatively higher header while it is regenerated, but never a
lower one than its visible files require. Import does not serialize or
reconstruct `__schema_history`; after compatibility succeeds, the migration
system applies every missing migration known to the receiving binary.

## Resolved Questions

### SQL vs. NDJSON

RFC 10177 proposed `.state/*.ndjson`. This RFC uses sorted SQL. **Decision: SQL.** Rationale:

- Native SQLite import (`sqlite3 < file.sql`) — no custom parser needed
- Consistent with `sqlite-diffable` ecosystem
- The research doc, RFC 10165 §7, and the vtable sketch converge on sorted SQL
- RFC 10184 carries forward the Local XDG taxonomy but does not require NDJSON as the dump format

## Open Questions

### Merge Driver

The projection merge path must automate compatible logical-row changes for both
repo-policy and sidecar-policy projections. The implementation may combine
gitattributes, a semantic SQL merge driver, and post-merge regeneration from
SQLite, but the user-facing contract is Exo-level reconciliation rather than
manual SQL conflict editing.

### Regeneration trigger

Options:

1. Pre-commit hook
2. On every daemon mutation
3. Explicit export command

## Implementation Notes

The serializer and importer live in `crates/exosuit-storage`. The call site for writing dumps is in `tools/exo/src/context.rs` / machine-channel mutation handling, and it resolves the projection directory from the active persistence policy.

Entity tables sort by `text_id`. Junction tables without `text_id` sort by their composite natural key.

RFC 10208 owns generation parsing, stable failures, and release sequencing. This
RFC owns only the projection carrier and the rule that preflight precedes every
filesystem or database creation side effect.

## Implementation Plan

1. Build the sorted-SQL serializer.
2. Build the SQL importer.
3. Add round-trip tests.
4. Replace TOML write-through call sites.
5. Remove obsolete TOML cache files.
6. Emit and preflight the RFC 10208 writer-generation header in `epochs.sql`.

## Success Criteria

1. SQL dumps deterministically represent the committed steering state.
2. A fresh clone can rebuild `{state_root}/cache/exo.db` from dumps.
3. Rowids are not serialized.
4. Workspace-active phase pins are not serialized.
5. No TOML projection is recreated.
6. A fence-aware older importer rejects a newer projection before creating a
   target directory or database, while legacy projections still import and
   migrate invisibly.
