<!-- exo:10154 ulid:01kmzxbcygfjj21jr4fe6y1e55 -->

# RFC 10154: Context Persistence

## Summary

Exo treats project context as durable operational state in SQLite at the
resolved project state root. Repo, sidecar, and shadow policy determine the
state root, the projection boundary, and the portability surface that carries
that state across tools, machines, and repositories.

Under repo policy, `docs/agent-context` carries generated SQL projections.
Durable project prose lives in RFCs, specifications, design notes, research
notes, manuals, and configuration.

## Motivation

Exo context must survive across sessions, tools, worktrees, and machines. The
persistence policy gives that context one operational home and clear public
surfaces around it.

Project context is structured operational state, read and mutated through Exo
commands. Generated projections make selected state portable and reviewable.
Human-authored documents carry durable design reasoning. Keeping those roles
separate lets users, agents, and maintainers understand where state lives, which
files Exo regenerates, and where project prose belongs.

## Detailed Design

### Persistence Contract

The canonical runtime store for Exo project state is the resolved SQLite
database:

```text
{state_root}/cache/exo.db
```

This database stores structured project state such as epochs, phases, goals,
tasks, RFC metadata, ideas, inbox items, completion evidence, and related
steering data. Exo commands and machine-channel operations read and mutate this
state through the SQLite-backed command surface.

Generated SQL files are portability projections of that database. They let
selected operational state be reviewed, diffed, synced, or reconstructed when
the selected policy calls for that behavior. SQLite remains the operational
source of truth for Exo commands and services.

Human-authored project knowledge lives in documents: RFCs, specifications,
design notes, research notes, manuals, and tool configuration. Documents carry
design reasoning, usage guidance, and project intent.

### Policy-Specific Persistence

Project policy determines where the state root and generated projection live.

| Policy | Operational State | Generated SQL Projection | Intended Use |
| --- | --- | --- | --- |
| `repo` | Repository-owned project state root. | `docs/agent-context/*.sql` in the work repository. | Team-owned Exo state that should be versioned with the repository. |
| `sidecar` | Resolved sidecar-backed project state root. | Sidecar project `agent-context/*.sql`. | Private portable personal state for a source/documentation repository whose Exo state lives outside its public tree. |
| `shadow` | Machine-local project state root. | None by default. | Private state for one machine or one local user environment. |

Repo policy is the only policy that writes generated SQL projections into the
work repository. Under repo policy, `docs/agent-context/*.sql` may be committed
because the repository owns that Exo state.

Sidecar policy writes the same kind of generated SQL projection into the
sidecar project, outside the work repository. The sidecar key and sidecar root
control that portable personal state. The work repository remains free of
sidecar-owned generated context files.

Shadow policy keeps Exo state local to the machine and leaves workspace SQL
projections unset by default.

### Generated Projection Rules

Generated SQL projections are infrastructure produced by Exo. Exo commands
regenerate them from operational state.

The projection format and table order belong to RFC 10178. The classification
of canonical state, tool configuration, and documents belongs to RFC 10180. This
RFC defines the persistence policy that connects those mechanisms to the
project's selected state policy.

### Human-Authored Documents

The current durable prose homes are:

- `docs/rfcs/` for RFC records;
- `docs/specs/` for specifications;
- `docs/design/` for design notes;
- `docs/research/` for research checkpoints and reconstruction notes; and
- ordinary repository configuration files for deterministic tool behavior.

Durable prose belongs in those document locations. Under repo policy,
`docs/agent-context` carries generated SQL output. Under sidecar policy, the
analogous `agent-context` directory belongs to the sidecar projection. Under
shadow policy, machine-local state remains local by default.

### Historical Context Surfaces

Readers may encounter older RFCs and migration notes that name mutable
phase-context files, archive snapshots, or TOML/Markdown state files. Examples
include `docs/agent-context/current`, `docs/agent-context/archive`, `plan.toml`,
`implementation-plan.toml`, `task-list.toml`, `ideas.toml`, and `inbox.toml`.

Those names now map to the current persistence roles. Runtime state lives in
SQLite. Policy-controlled SQL projections carry portable generated state.
Documents carry durable prose. Phase completion, task views, ideas, inbox
items, and completion evidence flow through the Exo state model and command
surface.

### Shadow Terminology

This RFC uses **shadow policy** to mean private machine-local Exo project state.
That is a project persistence policy.

RFC 10165 uses **SQLite shadow tables** to mean storage tables such as
`*_data` and `*_rev` that back reactive SQLite virtual tables. Those are storage
internals. They are distinct from Exo shadow policy.

## Relationship To Other RFCs

This RFC is the stable persistence policy. Related RFCs carry the supporting
models and mechanisms:

- RFC 10176 defines the project-state model that is persisted.
- RFC 10184 defines project identity, state roots, workspace roots, sidecar
  binding, and policy-specific path resolution.
- RFC 10178 defines deterministic SQL dump serialization and import.
- RFC 10180 defines the boundary between canonical state, tool configuration,
  generated projections, and documents.
- RFC 10165 defines reactive SQLite storage, virtual tables, shadow tables, row
  revisions, rowset revisions, and trace validation.

The remaining RFC 10165 shadow-boundary status cleanup belongs to RFC 10165.
This persistence policy continues to use SQLite-backed operational state and
policy-controlled generated projection placement.

## Design Guidance

### Repository Projection As Generated SQL

Repo policy keeps team-owned Exo state reviewable in Git through generated SQL
under `docs/agent-context/*.sql`. That keeps the repository's state projection
portable while preserving a clear distinction between generated operational
data and human-authored documents.

### Local-Only State

Shadow policy gives a project private machine-local Exo state. It fits local
experiments, private work, and environments where the repository should carry
only source and documentation.

### Structured State Over TOML Or Markdown Projections

SQLite provides the operational state model for structured project data.
Deterministic SQL dumps provide the portability bridge for policies that need a
filesystem projection. RFC 10180 carries the classification rule that separates
canonical state, generated projection, configuration, and documents.

### Use Sidecar Policy For Every Project

Sidecar policy is the right fit for private portable personal state, especially
when a public or team repository carries source and docs while Exo state stays
portable in a sidecar project. Repo policy fits repositories that intentionally
own shared Exo state. Shadow policy fits machine-local private state.

## Open Design Work

- The exact exported table set and import ordering are owned by RFC 10178 and
  the Exo storage implementation.
- Sidecar checkpointing, writer ownership, and sync failure recovery continue
  in sidecar-specific RFCs and command behavior.
- Future lane-centered work can introduce additional scoped state views while
  keeping the same persistence roles: SQLite for operational state,
  policy-controlled generated projections for portability, and documents for
  human-authored design prose.
