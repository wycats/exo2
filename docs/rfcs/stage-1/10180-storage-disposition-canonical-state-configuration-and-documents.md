<!-- exo:10180 ulid:01kmzxey17ce7zy7bcze7eeyz7 -->

# RFC 10180: Storage Disposition: Canonical State, Configuration, and Documents

## Summary

Every persistent surface in the Exosuit workspace falls into exactly one of three categories: **canonical state**, **tool configuration**, or **documents**. This RFC defines the classification rule, applies it to existing surfaces, and explicitly rejects the "file projection" pattern that previous migration work introduced.

RFC 10184 adds one refinement: canonical state lives in the resolved project database at `{state_root}/cache/exo.db`, not necessarily in a checkout-local `.cache/exo.db`.

## The Classification Rule

### 1. Canonical State → SQLite

Data is canonical state if it is **structured** and **used for project steering**.

- **Structured** means: the data has a schema; tools and steering logic operate on its structured relationships; the value is primarily in those relationships, not in prose content.
- **Used for project steering** means: the data guides decisions, tracks progress, or captures live tensions that are resolved via natural language in context.

Canonical state lives in SQLite. It is accessed through operations (`exo` commands, machine channel). It is never read from or written to TOML files by production code.

The SQLite database path is resolved from the current project:

```text
{state_root}/cache/exo.db
```

For default state this is usually `<primary-workspace>/.exo/cache/exo.db`. For shadow state this is `$HOME/.exo/projects/<project-id>/cache/exo.db`.

### 2. Tool Configuration → Files

Data is configuration if it **deterministically configures tool behavior**.

- Configuration nails down rules: what commands to run, where binaries are, what hooks to execute.
- Configuration tells tools what to do mechanically. It does not capture tensions or require natural-language resolution.

Configuration lives in files (`exosuit.toml`, `.config/exo/*`, `hooks.toml`).

### 3. Documents → Files

Data is a document if it is **primarily unstructured prose** whose value is in the narrative content.

- Documents may have structured metadata, but the reason they exist is the prose argument, not the metadata fields.
- Documents are human-authored and human-read. Their primary audience is people, not tools.

Documents live as files (Markdown, plain text).

### The Boundary Between Structured State and Documents

Some surfaces contain both structured fields and prose content. The test is: **what is the primary reason this thing exists?**

- **Axioms** have markdown content, but they exist because of their structured role in the steering system: id, scope, tags, principle text matched against context. The markdown is payload inside a structured container. → **Canonical state.**
- **RFCs** have metadata, but they exist because of the prose argument. The metadata is metadata on an unstructured document. → **Documents.**
- **Ideas** have a title and description, but they exist to be triaged, tagged, and tracked as structured backlog items. → **Canonical state.**

### The Boundary Between State and Configuration

- **Configuration** is deterministic and fully nails down rules. It tells tools what to do mechanically.
- **Steering data** captures live tensions that need to be resolved via natural language in the context in which the tensions arise. It tells the project how to navigate ambiguity.

`exosuit.toml` says "run this command." That is configuration.
An axiom says "prefer simplicity over configurability." That is steering data.

## The Anti-Pattern: File Projections of SQLite State

**There are no file projections of SQLite state.**

Previous migration work introduced a pattern where SQLite-canonical data was "projected" back into TOML files for human readability or backward compatibility. This pattern is wrong and is explicitly rejected.

- SQL dumps exist for **git-friendliness** (version control diffing), not for human reading or tool consumption.
- TOML files that were previously canonical (`plan.toml`, `implementation-plan.toml`, `ideas.toml`, `inbox.toml`) are not maintained as projections. They are deleted.
- `docs/agent-context/current/` phase context files and `docs/agent-context/archive/` phase snapshots are deleted legacy surfaces, not project memory.
- No new TOML projections of SQLite state should be created.
- Any existing code, design documents, or migration plans that describe TOML projections of SQLite state as a strategy are superseded by this RFC.

### Why RFCs Are Not a Counterexample

RFCs live on disk because they are **documents** (Rule 3), not because they are projections of SQLite state. The directory structure (`stage-0/`, `stage-1/`, etc.) organizes them by stage for human navigation. This is filesystem organization of documents, not a projection of canonical state.

RFC metadata in SQLite is an index over document files. The RFC files remain documents.

## Per-Surface Disposition

### Canonical State (SQLite)

| Surface | Canonical location | Status | Notes |
|---|---|---|---|
| Epochs | `{state_root}/cache/exo.db` | ✅ Migrated | SQL dump: `docs/agent-context/epochs.sql` |
| Phases | `{state_root}/cache/exo.db` | ✅ Migrated | |
| Goals | `{state_root}/cache/exo.db` | ✅ Migrated | |
| Tasks | `{state_root}/cache/exo.db` | ✅ Migrated | |
| Ideas | `{state_root}/cache/exo.db` | ✅ Migrated | SQL dump: `docs/agent-context/ideas.sql` |
| Inbox | `{state_root}/cache/exo.db` | ✅ Migrated | SQL dump: `docs/agent-context/inbox.sql` |
| Axioms | `docs/agent-context/axioms.*.toml` | ❌ Not yet migrated | Must move to SQLite |
| Task logs | `{state_root}/cache/exo.db` | ✅ Migrated | |
| Task verifications | `{state_root}/cache/exo.db` | ✅ Migrated | |
| Workspace active phase pins | `{state_root}/cache/exo.db` | ✅ Migrated | Project-local runtime state; not dumped to git |

### Deleted (Former State Surfaces)

| Surface | Former Location | Disposition |
|---|---|---|
| `plan.toml` | `docs/agent-context/plan.toml` | Superseded by SQLite epochs/phases/goals |
| `implementation-plan.toml` | `docs/agent-context/current/implementation-plan.toml` | Superseded by SQLite tasks + `phase.read-details` |
| `task-list.toml` | `docs/agent-context/current/task-list.toml` | Deprecated projection, delete |
| `walkthrough.toml` / `walkthrough.md` phase files | `docs/agent-context/current/` | Superseded by task logs and derived views |
| Phase context directory | `docs/agent-context/current/` | Deleted legacy state/document mix |
| Phase archive snapshots | `docs/agent-context/archive/` | Deleted legacy phase-finish snapshots |
| `ideas.toml` | `docs/agent-context/ideas.toml` | Superseded by SQLite ideas |
| `inbox.toml` | `docs/agent-context/inbox.toml` | Superseded by SQLite inbox |
| `decisions.toml` | `docs/agent-context/decisions.toml` | Deprecated, superseded by RFCs |
| `feedback.toml` | `docs/agent-context/feedback.toml` | Already deleted |
| `modes.toml` | `docs/agent-context/modes.toml` | Already deleted |
| `prompts.toml` | `docs/agent-context/prompts.toml` | Already deleted |

### Tool Configuration (Files)

| Surface | Location | Notes |
|---|---|---|
| `exosuit.toml` | Workspace root | Task definitions, binary paths, build config |
| `hooks.toml` | `.config/exo/hooks.toml` | Exohook configuration |
| Project state policy | `$XDG_CONFIG_HOME/exo/projects.toml` or `$HOME/.config/exo/projects.toml` | User-local shadow/default policy by project id |
| Rust toolchain | `rust-toolchain.toml` | Standard Rust config |
| Clippy | `clippy.toml` | Standard Rust config |
| Rustfmt | `rustfmt.toml` | Standard Rust config |
| TypeScript | `tsconfig.*.json` | Standard TS config |
| Package manifests | `package.json`, `Cargo.toml` | Standard package config |

### Documents (Files)

| Surface | Location | Notes |
|---|---|---|
| RFCs | `docs/rfcs/stage-*/` | Human-authored prose; directory structure reflects stage |
| Manual | `docs/manual/` | Human-authored documentation |
| Vision | `docs/vision.md`, `docs/vision-exo-everywhere.md` | Human-authored narrative |
| Specs/algebras | `docs/specs/` | Formal specifications |

### Secondary Views (Synced Filesystem Representations)

| Surface | Canonical Source | Filesystem View | Sync Mechanism |
|---|---|---|---|
| RFC stage directories | RFC document lifecycle metadata | `docs/rfcs/stage-N/` | `exo rfc promote` + validation |
| SQL dumps | `{state_root}/cache/exo.db` | `docs/agent-context/*.sql` | `exo` CLI / machine-channel mutations |

## Implications for Extension UI Surfaces

### Surfaces That Must Be Rebuilt on Canonical State

The following extension UI surfaces were built on TOML-as-source-of-truth and have been or should be removed:

- Rich Editor / Studio views for `plan.toml`, `implementation-plan.toml`, `task-list.toml`, `ideas.toml`, `inbox.toml`.
- Implementation-plan-based phase details rendering, replaced by `phase.read-details`.
- TOML-based root materialization for plan, inbox, ideas, replaced by daemon-backed root registration.

### Surfaces That Remain Valid

- RFC rendering in the rich editor — RFCs are documents, so file-based rendering is correct.
- Sidebar tree views — valid when backed by canonical operations.

### Surfaces That Are Transitional

- Axiom rendering — currently file-based TOML, but axioms are canonical state and should eventually be rendered from SQLite.

## Superseded Documents

This RFC supersedes:

- RFC 0131 (Implementation Plan as Canonical Execution Artifact) — `implementation-plan.toml` is no longer canonical.
- Any migration plan or design document that describes TOML projections of SQLite state as a supported pattern.

## Related RFCs

- RFC 10184 defines project/workspace identity and the resolved project database path.
- RFC 10178 defines git-friendly SQL dumps.
- RFC 0097 defines daemon lifecycle over project runtime paths.
- RFC 0125 defines machine-channel protocol.

## Success Criteria

1. Production code reads and writes structured steering state through SQLite operations.
2. No TOML projections of SQLite state are recreated.
3. SQL dumps remain git-friendly exports, not user-facing source of truth.
4. Documents remain files because their value is prose.
5. Path references use the resolved project database path `{state_root}/cache/exo.db`.
