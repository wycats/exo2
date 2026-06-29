# SQLite Migration Inventory

> Companion to [RFC 10165](../rfcs/stage-0/10165-reactive-sqlite-virtual-table-integration-with-revision-algebra.md).
> Generated from codebase recon — maps every touchpoint that changes in the TOML→SQLite migration.

---

## 1. Serde Structs → SQL Tables

These are the core data structures in `tools/exo/src/context.rs` that currently derive `Serialize`/`Deserialize` for TOML round-tripping. Each becomes a SQL table.

| Struct      | File:Line      | Fields                                                                                                      | SQL Table          | Notes                                                          |
| ----------- | -------------- | ----------------------------------------------------------------------------------------------------------- | ------------------ | -------------------------------------------------------------- |
| `ExoState`  | context.rs:891 | `meta`, `epochs[]`                                                                                          | — (root container) | Becomes the DB itself; `meta` → `_meta` table                  |
| `Epoch`     | context.rs:689 | `id`, `ulid`, `slug`, `title`, `status`, `review_status`, `aliases[]`, `phases[]`                           | `epochs`           | Has `EpochInput` (line 710) for custom deser                   |
| `Phase`     | context.rs:463 | `id`, `ulid`, `slug`, `title`, `status`, `kind`, `aliases[]`, `goals[]`, `rfcs[]`, `walkthroughs[]`         | `phases`           | FK → `epochs.id`. Has `PhaseInput` (line 555) for custom deser |
| `Goal`      | context.rs:275 | `id`, `ulid`, `slug`, `title`, `status`, `tdd_status`, `completion_log`, `rfc`, `target_stage`, `aliases[]` | `goals`            | FK → `phases.id`                                               |
| `PhaseRfc`  | context.rs:402 | `id`, `target_stage`                                                                                        | `phase_rfcs`       | FK → `phases.id`, references RFC number                        |
| `Idea`      | idea.rs:10     | `id`, `title`, `status`, `source`, `tags[]`, `notes`                                                        | `ideas`            | Currently in `ideas.toml`                                      |
| `InboxItem` | inbox.rs:39    | `id`, `title`, `status`, `priority`, `source`, `tags[]`, `notes`, `surfaced_as`                             | `inbox_items`      | Currently in `inbox.toml`                                      |
| `Meta`      | context.rs:35  | `schema_version`, `exo_version`                                                                             | `_meta`            | Single-row config table                                        |

**Additional structs in implementation-plan.toml** (managed by `implementation.rs`):

- Implementation plan header (`phase.id`, `phase.title`)
- Goal execution entries (goal ID + tasks)
- Task entries (`id`, `title`, `status`, `tdd_status`, `log`)

**Structs in other files**:

- `Decision` in decisions.toml
- `Feedback` in feedback.toml
- `Axiom` in axioms.\*.toml
- `Walkthrough` entries in plan.toml phases

---

## 2. Hand-Coded Query Methods → SQL Queries

Every `find_*`/`list_*`/`get_*` method on `ExoState` is a hand-coded traversal of nested Vec structures. Each becomes a SQL query.

| Method                        | File:Line       | Current Logic                                             | SQL Replacement                                                                                                    |
| ----------------------------- | --------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `find_active_phase()`         | context.rs:128  | Nested loop: epochs→phases, match `status = "active"`     | `SELECT * FROM phases WHERE status IN ('active','in-progress') LIMIT 1`                                            |
| `find_active_phase_id()`      | context.rs:147  | Delegates to above, extracts `.id`                        | Same query, select `id` only                                                                                       |
| `find_next_pending_phase()`   | context.rs:155  | Scan phases after anchor, find first `status = "pending"` | `SELECT * FROM phases WHERE status='pending' AND rowid > (SELECT rowid FROM phases WHERE id=?) LIMIT 1`            |
| `find_phase_by_id()`          | context.rs:184  | Nested loop + `matches_id()` (ULID/slug/alias)            | `SELECT * FROM phases WHERE id=? OR ulid=? OR slug=? OR id IN (SELECT phase_id FROM aliases WHERE alias=?)`        |
| `find_epoch_by_id()`          | context.rs:196  | Linear scan + `matches_id()`                              | Same pattern on `epochs` table                                                                                     |
| `find_goal_in_active_phase()` | context.rs:204  | `find_active_phase()` then scan goals                     | `SELECT g.* FROM goals g JOIN phases p ON g.phase_id=p.id WHERE p.status='active' AND (g.id=? OR g.ulid=? OR ...)` |
| `find_goal_by_id()`           | context.rs:224  | Triple-nested loop: epochs→phases→goals                   | `SELECT * FROM goals WHERE id=? OR ulid=? OR slug=? OR ...`                                                        |
| `find_unreviewed_epochs()`    | context.rs:238  | Filter epochs by `needs_review()`                         | `SELECT * FROM epochs WHERE status='completed' AND review_status IS NULL`                                          |
| `list_ideas()`                | idea.rs:71      | Read ideas.toml, parse, return Vec                        | `SELECT * FROM ideas`                                                                                              |
| `list_tasks()`                | task.rs:623     | Read impl-plan.toml, parse, filter by goal                | `SELECT * FROM tasks WHERE goal_id=?`                                                                              |
| `list_execution_tasks()`      | task.rs:370     | Read impl-plan.toml, parse all tasks                      | `SELECT * FROM tasks WHERE phase_id=?`                                                                             |
| `get_pending_items()`         | inbox.rs:421    | Read inbox.toml, filter by status                         | `SELECT * FROM inbox_items WHERE status='pending'`                                                                 |
| `get_surfaced_intents()`      | inbox.rs:468    | Read inbox.toml, filter by surfaced_as                    | `SELECT * FROM inbox_items WHERE surfaced_as IS NOT NULL`                                                          |
| `list_criteria()`             | criteria.rs:197 | Read impl-plan.toml, extract criteria                     | `SELECT * FROM criteria WHERE goal_id=?`                                                                           |
| `list_axioms()`               | axiom.rs:168    | Read axioms.\*.toml files                                 | `SELECT * FROM axioms`                                                                                             |

**Big win**: `UlidResolvable::matches_id()` (ulid_util.rs:194) currently does 5-way matching (canonical ref, ULID, slug, primary ID, aliases) in Rust. This becomes a single SQL `WHERE` clause with `OR`/`UNION`, eliminating the trait entirely.

---

## 3. Cross-File Joins → SQL JOINs

These are the most painful parts of the current architecture — places where multiple TOML files must be loaded and manually correlated.

| Join                            | Files Involved                                       | Current Code                                                                                                                                                               | SQL Replacement                                                                                                                                                   |
| ------------------------------- | ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Goal status derivation**      | plan.toml + implementation-plan.toml + RFC files     | `DeriveContext` (derived.rs:14) loads RFC stages from filesystem, `derive_from_implementation_plan()` (derived.rs:39) reads impl-plan.toml, matches phase ID, counts tasks | `SELECT g.*, COUNT(t.id) as task_count, COUNT(CASE WHEN t.status='done' THEN 1 END) as done_count FROM goals g LEFT JOIN tasks t ON t.goal_id=g.id GROUP BY g.id` |
| **Phase status derivation**     | plan.toml + implementation-plan.toml                 | `derived_status()` on Phase/Epoch (context.rs)                                                                                                                             | `SELECT p.*, (SELECT COUNT(*) FROM goals WHERE phase_id=p.id AND status='completed') as completed_goals FROM phases p`                                            |
| **RFC pipeline tracking**       | plan.toml (PhaseRfc) + docs/rfcs/ filesystem         | `DeriveContext.rfc_stage()` (derived.rs:33)                                                                                                                                | `SELECT pr.*, r.stage FROM phase_rfcs pr JOIN rfcs r ON pr.rfc_id=r.number`                                                                                       |
| **VS Code impl-plan↔plan join** | plan.toml + implementation-plan.toml                 | `ImplementationPlanExecution.ts:426` — `smol-toml` parse of both files, manual goal ID matching, orphan filtering                                                          | Eliminated — single SQL query via CLI JSON                                                                                                                        |
| **Steering recommendations**    | plan.toml + impl-plan.toml + inbox.toml + ideas.toml | steering.rs reads multiple files                                                                                                                                           | Single query across joined tables                                                                                                                                 |

**This is the biggest cleanup win.** The `DeriveContext` pattern (load N files, correlate by ID, derive status) is exactly what a relational DB does natively.

---

## 4. Write Paths (edit_cli_managed_file calls)

`edit_cli_managed_file` (utils.rs:54) is the universal write pattern: read file → parse TOML → mutate in memory → serialize → write. Each call becomes a SQL `INSERT`/`UPDATE`/`DELETE`.

### By file (50+ total calls):

| Module                | File                     | Calls  | Operations                                                                                                                                   |
| --------------------- | ------------------------ | ------ | -------------------------------------------------------------------------------------------------------------------------------------------- |
| **implementation.rs** | implementation-plan.toml | **13** | Create plan, add/remove/update goals, set phase header, sync goals, update goal status, set TDD status, add/remove criteria, update criteria |
| **task.rs**           | implementation-plan.toml | **7**  | Add task, remove task, update task, complete task, reorder task, start task, batch update                                                    |
| **inbox.rs**          | inbox.toml               | **4**  | Add item, update item, remove item, surface intent                                                                                           |
| **criteria.rs**       | implementation-plan.toml | **4**  | Add criterion, remove criterion, update criterion, complete criterion                                                                        |
| **walkthrough.rs**    | plan.toml                | **4**  | Add walkthrough, remove walkthrough, update walkthrough, complete walkthrough                                                                |
| **idea.rs**           | ideas.toml               | **3**  | Add idea, update idea, remove idea                                                                                                           |
| **feedback.rs**       | feedback.toml            | **3**  | Add feedback, update feedback, remove feedback                                                                                               |
| **strike.rs**         | plan.toml                | **2**  | Start strike, finish strike                                                                                                                  |
| **tdd.rs**            | implementation-plan.toml | **2**  | Start TDD cycle, update TDD status                                                                                                           |
| **plan.rs**           | plan.toml                | **2+** | Update plan structure, migrate IDs                                                                                                           |
| **phase.rs**          | plan.toml                | **1**  | Finish phase (status update)                                                                                                                 |
| **context.rs**        | plan.toml                | **1**  | Save state                                                                                                                                   |
| **axiom.rs**          | axioms.\*.toml           | **1**  | Add axiom                                                                                                                                    |
| **rfc.rs**            | —                        | **1**  | RFC metadata updates                                                                                                                         |

**Pattern**: Every call follows read→parse→mutate→serialize→write. With SQLite, each becomes a prepared statement. The `edit_cli_managed_file` function itself is eliminated.

---

## 5. VS Code Extension Impact

The extension has its own TOML parsing layer that duplicates logic from the Rust CLI.

### Direct TOML consumers:

| File                                  | What it does                                                                                     | Migration                                                                  |
| ------------------------------------- | ------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------- |
| `ImplementationPlanExecution.ts`      | Parses impl-plan.toml + plan.toml with `smol-toml`, does manual JOIN by goal ID, filters orphans | **Eliminate** — call `exo` CLI for JSON, or query SQLite directly via WASM |
| `PlanService.ts`                      | Loads plan.toml, parses TOML                                                                     | Same — consume CLI JSON or SQLite                                          |
| `mappers/PlanMapper.ts`               | Maps plan.toml TOML → TypeScript types                                                           | Replaced by SQL result mapping                                             |
| `mappers/ImplementationPlanMapper.ts` | Maps impl-plan.toml TOML → TypeScript types                                                      | Same                                                                       |
| `mappers/IdeaMapper.ts`               | Maps ideas.toml → TypeScript types                                                               | Same                                                                       |
| `mappers/AxiomMapper.ts`              | Maps axioms TOML → TypeScript types                                                              | Same                                                                       |
| `mappers/DecisionMapper.ts`           | Maps decisions.toml → TypeScript types                                                           | Same                                                                       |

### File watchers:

The extension watches `*.toml` files in `docs/agent-context/` for changes. With SQLite, this becomes watching a single `.db` file (or using the CLI's notification mechanism).

### Agent tools:

`agent/` directory tools reference TOML file paths for context injection. These paths change to either CLI commands or SQLite queries.

---

## 6. Merge Driver

`merge_driver.rs` implements a semantic TOML merge driver (`exo merge-driver toml`) registered via `.gitattributes`:

```
docs/agent-context/ideas.toml merge=exo-toml
docs/agent-context/plan.toml merge=exo-toml
docs/agent-context/current/*.toml merge=exo-toml
```

**Migration**: If the git-friendly format is sorted SQL statements, the merge driver needs to understand SQL line semantics instead of TOML structure. Alternatively, if using a single `.db` file with a text export, the merge driver operates on the text export.

Related files:

- `merge_driver.rs` (468 lines) — the merge logic
- `templates.rs:232` — `.gitattributes` template
- `templates.rs:572` — `configure_merge_driver()`
- `upgrade/plugins/ensure_merge_driver.rs` — upgrade plugin

---

## 7. Test Impact

### Test files creating TOML fixtures:

| Test File                          | What it tests                            |
| ---------------------------------- | ---------------------------------------- |
| `tests/plan_toml_parse.rs`         | Plan parsing, epoch/phase/goal structure |
| `tests/goal_abandon.rs`            | Goal status transitions                  |
| `tests/goal_complete.rs`           | Goal completion flow                     |
| `tests/phase_status_derived.rs`    | Cross-file status derivation             |
| `tests/strike_test.rs`             | Surgical strike operations               |
| `tests/schema_meta.rs`             | Schema version metadata                  |
| `tests/impl_plan_goals_parsing.rs` | Implementation plan parsing              |
| `tests/plan_migrate_ids.rs`        | ULID migration                           |
| `tests/tdd_*.rs`                   | TDD cycle operations                     |
| `tests/task_*.rs`                  | Task CRUD operations                     |

All of these create TOML strings as fixtures and assert on parsed structures. They would need SQL fixture equivalents (either `.sql` files or in-memory SQLite setup).

### VS Code extension tests:

- `test/suite/ImplementationPlanExecution.test.ts` — tests the TOML→tree mapping
- `test/suite/mappers/ImplementationPlanMapper.test.ts` — tests TOML→TS mapping

---

## 8. What Gets Cleaner

### Eliminated entirely:

- **`edit_cli_managed_file`** pattern (50+ call sites) → prepared SQL statements
- **`UlidResolvable` trait** + 3 implementations (Goal, Phase, Epoch) → SQL `WHERE` with `OR`
- **`DeriveContext`** cross-file join machinery → SQL `JOIN`
- **Custom deserializers** (`PhaseInput`, `EpochInput`) → SQL schema handles this
- **TOML merge driver** (468 lines) → SQL-aware merge or eliminated if using CLI-mediated writes
- **VS Code TOML parsing layer** (6 mapper files + `smol-toml` dependency) → consume CLI JSON

### Dramatically simplified:

- **`implementation.rs`** (13 edit calls) → 13 SQL statements
- **`task.rs`** (7 edit calls) → 7 SQL statements
- **`context.rs`** query methods (8 hand-coded traversals) → 8 SQL queries
- **`derived.rs`** (382 lines of cross-file derivation) → SQL JOINs + GROUP BY

### New capabilities unlocked:

- **Ad-hoc queries**: "show me all goals with TDD status across all epochs" — trivial SQL, currently impossible without custom code
- **Referential integrity**: FK constraints prevent orphan goals, dangling phase refs
- **Atomic multi-entity updates**: Currently requires careful ordering of TOML writes
- **Indexing**: Phase/goal lookups go from O(n) scan to O(1) index lookup

---

## Summary: Migration Scope

| Category                      | Items         | Complexity                                            |
| ----------------------------- | ------------- | ----------------------------------------------------- |
| SQL tables to create          | ~10           | Low — direct struct→table mapping                     |
| Query methods to replace      | ~15           | Low — mechanical SQL translation                      |
| Write paths to replace        | ~50           | Medium — each is simple but there are many            |
| Cross-file joins to eliminate | ~5            | **High value** — biggest cleanup win                  |
| VS Code mappers to eliminate  | ~6            | Medium — need new data flow (CLI JSON or WASM SQLite) |
| Merge driver to rewrite       | 1 (468 lines) | Medium — depends on git format decision               |
| Test fixtures to migrate      | ~15 files     | Medium — mechanical but tedious                       |
| Serde structs to retire       | ~8            | Low — replaced by SQL row types                       |

**Estimated net code delta**: Significant reduction. The 50+ `edit_cli_managed_file` calls, `UlidResolvable` trait, `DeriveContext`, and VS Code TOML mappers represent ~2000+ lines that collapse into ~200 lines of SQL + a thin query layer.
