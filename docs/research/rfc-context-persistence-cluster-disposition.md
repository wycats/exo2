# RFC 10154 Context Persistence Cluster Disposition

This checkpoint reviews RFC `10154` against the current persistence model,
sidecar/shadow policy split, and reactive SQLite storage design.

## Finding

RFC `10154` is directionally right that Exo context must be durable and
portable, but its Stage 4 text is too broad for the current system. The current
rule is not "check `docs/agent-context` into version control" as a human
context directory. The current rule is:

- Operational project state lives in the resolved SQLite database.
- Repo policy writes generated SQL dumps under `docs/agent-context/*.sql`.
- Sidecar policy writes generated SQL dumps under the sidecar project
  `agent-context/*.sql`.
- Shadow policy keeps state machine-local and does not write workspace SQL
  projections.
- RFCs, research notes, design notes, and specs remain prose documents.

## Decision Table

| RFC ID | Current Claim | Current Code/State Evidence | Relationship to `10154` | Recommended Disposition | Review Question Before Action |
| ---: | --- | --- | --- | --- | --- |
| `10154` | Stable context persistence rule: check `docs/agent-context` into version control. | `docs/design/agent-context-ownership.md` defines `docs/agent-context/*.sql` as generated repo-policy projection, not a durable human-doc home. `tools/exo/src/context.rs` resolves the active projection directory from project policy. | This is the stale Stage 4 record being reviewed. It preserves the persistence intent, but its storage boundary is now wrong. | Rewrite as the current Stage 4 persistence policy. Keep the durability principle, replace the directory-level rule with policy-specific SQLite and SQL projection rules. | Should the rewrite stay in RFC `10154`, or should it create a new stable replacement and archive `10154` afterward? |
| `10176` | Project state model: epochs, phases, goals, tasks, authority semantics, and secondary entities. | Storage migrations and Exo commands use the SQLite-backed entity model. RFC `10176` now supersedes `0022`, `00177`, `00229`, and `10161`. | Defines what the persisted state is. `10154` should point at this model instead of old context-file prose. | Keep as the state-model anchor for the rewrite. | Does `10154` need to name every state table, or only refer to the project-state model and persistence policy? |
| `10165` | Reactive SQLite storage with virtual tables, shadow tables, traces, row digests, and rowset revisions. | `crates/exosuit-storage/src/vtab/reactive.rs` records membership through `xFilter` and content through `xColumn`; `crates/exosuit-storage/src/revisions.rs` stores row digests and rowset counters; `crates/exosuit-storage/src/schema.rs` registers reactive virtual tables over `*_data` tables. | Explains why SQLite is not just storage but also the planned reactive observation substrate. | Use as implementation context for `10154`, especially to define "shadow tables" separately from Exo shadow policy. | How much of the reactive mechanism belongs in a persistence RFC versus remaining in `10165`? |
| `10178` | Git-friendly sorted SQL text dumps are the portable representation of SQLite state. | `crates/exosuit-storage/src/dump.rs` dumps selected tables and excludes workspace-local phase focus/ownership; `tools/exo/src/context.rs` imports dumps when a DB is missing. | Provides the current replacement for "check in context files" under repo policy. | Treat as the serialization/persistence mechanism that `10154` should reference. | Should `10154` require SQL dumps for repo policy, or defer file layout entirely to `10178`? |
| `10180` | Persistent surfaces are canonical state, tool configuration, or documents; old TOML projections are rejected. | The RFC classifies SQL dumps as secondary filesystem representations while rejecting old TOML projections and `docs/agent-context/current` phase files. | Clarifies that generated SQL dumps are not human-authored context documents. | Use this as the classification vocabulary for the `10154` rewrite. | Should `10154` explicitly say "generated SQL projection" to avoid conflict with `10180`'s anti-projection language? |
| `10184` | Project, workspace, and worktree are separate; state policy selects repo, shadow, or sidecar storage behavior. | `tools/exo/src/project.rs` resolves repo/shadow/sidecar policy, state root, DB path, runtime path, and sidecar projection directory. | Determines where `10154` persistence applies. | Use as the policy/path anchor for the rewrite. | Should `10154` describe repo/shadow/sidecar policy directly, or refer to `10184` and state the persistence invariant only? |
| `10189` | Sidecar Git is a transport for Exo state, not the state model. | Sidecar commands and status surfaces expose sidecar roots, repo status, projection directories, and sync state; sidecar SQL projection is separate from the worktree. | Extends `10154` from repo persistence to personal portable state. | Reference as sidecar portability context, not as the direct replacement for `10154`. | Should `10154` mention sidecar sync barriers, or keep them in sidecar-specific RFCs? |
| `10191` | Sidecar write ownership and stale writer fencing control who may checkpoint shared sidecar state. | Phase ownership/focus state is intentionally not dumped; sidecar writer ownership is future-facing safety policy around checkpointing. | Affects when generated persistence may become portable, but not what the persistence model is. | Include as boundary context only. | Does persistence policy need to state "ownership controls checkpointing" without defining ownership mechanics? |
| `10196` | Worktree-aware sidecar state must distinguish shared project state from branch-local document observations. | RFC markdown remains document files; shared RFC metadata can outlive one workspace's visible branch file tree. | Important for `10154` because RFCs are documents, not SQLite state projections. | Include to prevent `10154` from treating document files as complete shared state. | Should the rewrite mention document overlays now, or leave that to RFC-specific metadata handling? |
| `0071` | Projection-backed resources are read views; operations mutate roots. | The RFC provides vocabulary for projection-backed reads and writable operations. | Useful vocabulary, but not the core persistence rule. | Mention only if the rewrite needs "projection-backed read" terminology. | Is this vocabulary helpful, or does it add unnecessary abstraction to `10154`? |
| `0022` | Old unified project state via TypeScript `ContextService`. | Archived and superseded by `10176`; current state is SQLite-backed Exo state. | Historical predecessor for the same source-of-truth problem. | Complete; no additional action for this checkpoint. | None. |
| `0114` | Advanced phase transition with archived `docs/agent-context/current` artifacts. | Withdrawn in PR `#187`; phase finish no longer creates archive snapshots. | Removes the old archive strategy that `10154` still references. | Use as evidence that archive/current-file persistence language should be removed from `10154`. | None. |
| `0131` | `docs/agent-context/current/implementation-plan.toml` as canonical execution artifact. | Withdrawn; `10180` says implementation-plan TOML is superseded by SQLite tasks and phase read surfaces. | Historical source of the file-based execution-context model. | No action here beyond citing as replaced background. | None. |
| `10161` | TOML Plan Object Model as canonical project state. | Withdrawn; superseded by `10176` in Exo metadata. | Historical predecessor to SQLite project state. | No action here beyond preserving the supersession trail. | None. |
| `00229` | Goal status authority in `plan.toml` with derived signals from implementation-plan TOML. | Withdrawn; `10176` carries the authority model into SQLite-backed goals and tasks. | Historical predecessor for status authority semantics. | No action here beyond preserving the supersession trail. | None. |
| `00177` | Goals and tasks unified work item model. | Withdrawn; superseded by `10176`. | Historical predecessor for the project-state hierarchy. | No action here beyond preserving the supersession trail. | None. |

## Shadow Means Two Different Things

SQLite shadow tables are storage internals. Tables such as `epochs_data`,
`phases_data`, `goals_data`, and `tasks_data` hold rows behind reactive virtual
tables. Revision tables and `rowset_revisions` support trace validation.

Exo shadow policy is a project policy. It stores a user's private machine-local
project database under `$HOME/.exo/projects/<project_id>` and does not import
from or export to `docs/agent-context/*.sql` by default.

The `10154` rewrite should keep these meanings separate. "Shadow tables" are
inside SQLite. "Shadow policy" is where a project's Exo state lives.

## Reactive SQLite Context

The reactive storage model matters because it explains why SQLite is the
operational state boundary, not just a serialization format.

- `xFilter` records a membership observation for a table rowset.
- `xColumn` records a content observation for the current row.
- Row digests represent row content revisions.
- Rowset counters represent membership revisions.
- Trace validation compares recorded revisions with current revisions.

Current code has reactive virtual tables, trace recording, revision stores, and
defensive-mode shadow table protection. The command write path is still
implemented through the storage layer rather than virtual-table write
interception. That means `10154` should describe the persistence boundary
without depending on virtual-table writes being the only mutation path.

## Persistence Language

The durable wording should be:

> Exo operational state is canonical in the resolved SQLite database. Repo,
> sidecar, and shadow policy determine whether and where Exo emits generated
> SQL portability projections. Human-authored project knowledge lives in RFCs,
> research notes, design notes, specs, and configuration files, not in
> generated `docs/agent-context` directories.

That resolves the apparent tension between:

- `10154`: check in `docs/agent-context`;
- `10178`: generated SQL dumps are git-friendly portability output;
- `10180`: old TOML/file projections of SQLite state are not source of truth;
- `10184`: repo, sidecar, and shadow policy decide where state and projections
  live.

## Recommended Next Action

Rewrite RFC `10154` as the current Stage 4 persistence policy.

The rewrite should preserve the original durability principle but replace the
old directory-level rule with policy-specific persistence language:

- SQLite is the operational source of truth.
- Repo policy commits generated SQL dumps under `docs/agent-context/*.sql`.
- Sidecar policy stores generated SQL dumps in the sidecar projection.
- Shadow policy remains machine-local.
- `docs/agent-context/current` and `docs/agent-context/archive` are legacy
  surfaces, not current persistence strategy.
- RFCs and research/design/spec docs remain human-authored documents.

Do not supersede or archive `10154` before the replacement text exists. The
lowest-risk next PR is a focused rewrite of `10154` using `10176`, `10178`,
`10180`, `10184`, and `10165` as the supporting current-state anchors.
