<!-- exo:10154 ulid:01kmzxbcygfjj21jr4fe6y1e55 -->

# RFC 10154: Context Persistence

## Summary

Exo project context has four durable surfaces:

- **Operational state** lives in SQLite at `{state_root}/cache/exo.db`.
- **Runtime materialization** lives under `{state_root}/runtime/`.
- **Generated projection** is policy-controlled SQL output used for portability,
  review, sync, and reconstruction.
- **Human-authored documents** carry design reasoning, specifications, research,
  manuals, and configuration.

Repo, sidecar, and shadow policy decide who owns those surfaces. Repo policy
stores shared project state with the repository. Sidecar policy stores
user-owned portable state outside the work repository. Shadow policy stores
machine-local private state.

## Motivation

Exo context must survive across sessions, tools, worktrees, and machines. The
persistence policy gives that context a stable operational home and clear
public surfaces around it.

Project context is structured operational state, read and mutated through Exo
commands. Generated projections make selected state portable and reviewable.
Human-authored documents carry durable design reasoning. Keeping those roles
separate lets users, agents, and maintainers understand where state lives, which
files Exo regenerates, and where project prose belongs.

This policy is especially important for public or upstream source repositories.
A user can keep rich Exo state for that repository through a private sidecar
while the public worktree carries source, documentation, and configuration.

## Detailed Design

### Stable Persistence Roles

Every Exo project has a resolved project state root. The state root determines
where operational state and runtime materialization live:

```text
{state_root}/cache/exo.db
{state_root}/runtime/
```

`cache/exo.db` stores structured project state: epochs, phases, goals, tasks,
RFC metadata, ideas, inbox items, completion evidence, and related steering
data. Exo commands, daemon operations, and machine-channel operations read and
mutate this state through the SQLite-backed command surface.

`runtime/` stores runtime materialization for the resolved project state root,
including daemon socket and PID material. Runtime materialization is part of the
operational environment for the current machine.

Generated SQL projections serialize selected operational state into
deterministic text. They provide reviewability, personal or team sync, and
database reconstruction for policies that expose a projection.

Human-authored documents carry project meaning. RFCs, specifications, design
notes, research notes, manuals, and configuration describe intent, behavior,
constraints, and decisions.

### Policy-Specific Persistence

Project policy determines who owns the project state root and where generated
SQL projections are written.

| Policy | Operational State | Generated SQL Projection | Intended Use |
| --- | --- | --- | --- |
| `repo` | Repository-owned project state root. | `docs/agent-context/*.sql` in the work repository. | Team-owned Exo state that should be versioned with the repository. |
| `sidecar` | Resolved sidecar-backed project state root. | Sidecar project `agent-context/*.sql`. | Private portable personal state for a source/documentation repository whose Exo state lives outside its public tree. |
| `shadow` | Machine-local project state root. | None by default. | Private state for one machine or one local user environment. |

Repo policy makes the repository the owner of Exo operational state. The
repository can commit the generated SQL projection under `docs/agent-context`.

Sidecar policy makes a user-named sidecar project the owner of Exo operational
state for a work repository. The work repository carries source, documentation,
and configuration. The sidecar project carries the user's Exo state and
projection.

Shadow policy makes one machine the owner of Exo operational state. It gives a
checkout private Exo state and leaves workspace projection unset by default.

### Sidecar Identity And Continuity

Sidecar policy is the primary persistence model for a public or upstream
repository with user-owned Exo state. It separates **portable sidecar identity**
from **local checkout identity**.

The selected sidecar key is the portable identity. It names the user's sidecar
project across machines and checkouts. The local project id is the binding
between the current git checkout and that sidecar project.

A sidecar-backed project resolves these paths from the sidecar binding:

```text
{sidecar_root}/projects/{sidecar_key}/sidecar.toml
{sidecar_root}/projects/{sidecar_key}/cache/exo.db
{sidecar_root}/projects/{sidecar_key}/runtime/
{sidecar_root}/projects/{sidecar_key}/agent-context/*.sql
```

The sidecar manifest records the sidecar key and the local project id bound to
that sidecar project:

```toml
[sidecar]
key = "<sidecar-key>"
project_id = "<local-project-id>"
```

The sidecar database and runtime directory are the live operational state for
that binding. The sidecar `agent-context/*.sql` files are the generated
portability projection for personal sync, review, and reconstruction.

Sidecar repository commands manage that portability boundary. `exo sidecar repo
status` reports the sidecar git state. `exo sidecar repo commit --message
<msg>` flushes the current SQL projection and commits sidecar-owned files. `exo
sidecar repo push` and `exo sidecar repo sync` move that sidecar projection
through the configured sidecar remote.

When a checkout moves and its path-derived project id changes, `exo project
move-root --key <sidecar-key> --to <checkout-root>` reconciles the binding. It
preserves the sidecar key, sidecar project directory, database, runtime state,
and generated projection while retargeting local policy, the sidecar manifest
`project_id`, workspace active-phase rows, and workspace phase-ownership ids to
the new checkout identity.

### Projection Contract

Generated SQL projections are infrastructure produced by Exo from SQLite-backed
operational state. Exo commands regenerate the projection after mutations.

Projection paths are selected by policy:

```text
repo    → docs/agent-context/*.sql
sidecar → {sidecar_root}/projects/{sidecar_key}/agent-context/*.sql
shadow  → no workspace projection by default
```

When repo and sidecar projections both exist, the selected policy determines
which projection Exo uses. Exo treats the projection as generated operational
state for the selected policy, and policy owners stay distinct.

RFC 10178 defines deterministic SQL dump format, table order, import behavior,
and round-trip requirements. RFC 10180 defines the classification boundary
between canonical state, generated projection, tool configuration, and
documents.

### Document Boundary

Durable prose belongs in document locations:

- `docs/rfcs/` for RFC records;
- `docs/specs/` for specifications;
- `docs/design/` for design notes;
- `docs/research/` for research checkpoints and reconstruction notes; and
- ordinary repository configuration files for deterministic tool behavior.

Under repo policy, `docs/agent-context` is generated SQL output. Under sidecar
policy, `agent-context` belongs to the sidecar projection. Under shadow policy,
machine-local state remains local by default.

### Shadow Terminology

This RFC uses **shadow policy** to mean private machine-local Exo project
state. Shadow policy is a project persistence policy.

RFC 10165 uses **SQLite shadow tables** to mean storage tables such as
`*_data` and `*_rev` that back reactive SQLite virtual tables. SQLite shadow
tables are storage internals.

## Relationship To Other RFCs

This RFC is the stable persistence policy. Related RFCs carry the supporting
models and mechanisms:

- RFC 10176 defines the project-state model persisted in SQLite.
- RFC 10184 defines project identity, workspace identity, state roots, sidecar
  binding, and `project move-root` reconciliation.
- RFC 10178 defines deterministic SQL dump serialization and import.
- RFC 10180 defines the boundary between canonical state, tool configuration,
  generated projections, and documents.
- RFC 10165 defines reactive SQLite storage, virtual tables, SQLite shadow
  tables, row revisions, rowset revisions, and trace validation.

The remaining RFC 10165 shadow-boundary status cleanup belongs to RFC 10165.
This persistence policy continues to use SQLite-backed operational state and
policy-controlled generated projection placement.

## Historical Compatibility

Readers may encounter older RFCs and migration notes that name mutable
phase-context files, archive snapshots, or TOML/Markdown state files. Examples
include `docs/agent-context/current`, `docs/agent-context/archive`, `plan.toml`,
`implementation-plan.toml`, `task-list.toml`, `ideas.toml`, and `inbox.toml`.

Those names now map to the current persistence roles. Runtime state lives in
SQLite. Policy-controlled SQL projections carry portable generated state.
Documents carry durable prose. Phase completion, task views, ideas, inbox
items, and completion evidence flow through the Exo state model and command
surface.

## Design Guidance

### Repo Policy

Repo policy keeps team-owned Exo state reviewable in Git through generated SQL
under `docs/agent-context/*.sql`. That keeps the repository's state projection
portable while preserving a clear distinction between generated operational
data and human-authored documents.

### Sidecar Policy

Sidecar policy keeps Exo state available for a work repository while keeping
the repository tree focused on public source, documentation, and configuration.
The sidecar key is the user-facing handle for that state. The sidecar manifest
records the local project binding, and the sidecar git repository carries the
generated SQL projection through personal sync.

The supported continuity path for a moved checkout is `exo project move-root`.
The command reconciles path-derived local identity with the existing portable
sidecar identity and preserves the sidecar state root.

### Shadow Policy

Shadow policy gives a project private machine-local Exo state. It fits local
experiments, private work, and environments where the repository should carry
only source and documentation.

### Structured State

SQLite provides the operational state model for structured project data.
Deterministic SQL dumps provide the portability bridge for policies that need a
filesystem projection. RFC 10180 carries the classification rule that separates
canonical state, generated projection, configuration, and documents.

## Open Design Work

- The exact exported table set and import ordering are owned by RFC 10178 and
  the Exo storage implementation.
- Sidecar writer ownership, checkpoint safety, and sync recovery continue in
  the sidecar command surface and sidecar-specific RFCs.
- Future lane-centered work can introduce additional scoped state views while
  keeping the same persistence roles: SQLite for operational state,
  policy-controlled generated projections for portability, and documents for
  human-authored design prose.
