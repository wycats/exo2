# Project-State RFC Cluster Disposition

This checkpoint maps the project-state RFC cluster before the next focused RFC
disposition. The cluster centers on the shift from old file/service-oriented
context prose to the current SQLite-backed Exo state model, plus the adjacent
phase and persistence rules that now depend on that model.

Post-checkpoint update: PR [#187](https://github.com/wycats/exo2/pull/187)
processed the phase-archive side of this cluster. RFC `0114` was withdrawn
with the broader current/archive-file cleanup, and phase finish no longer
creates `docs/agent-context/archive` snapshots.

## Cluster Table

| RFC ID | Current Claim | Current Code/State Evidence | Relationship to the Cluster | Recommended Disposition | Review Question Before Action |
| ---: | --- | --- | --- | --- | --- |
| `0022` | Stable unified project state through a TypeScript `ContextService` in `packages/exosuit-core`. | `crates/exosuit-storage/migrations/V001__core_tables.sql` defines the epoch/phase/goal/task hierarchy based on RFC `10176`; `tools/exo/src/context.rs` is the current Exo state access layer; `ContextService` references now appear in documentation and planning history, not as current TypeScript or Rust project-state implementation code. | This is the clearest stale stable law in the cluster: it names the right problem, but its implementation model has been replaced. | Make `0022` the next focused disposition target. Treat `10176` as the current replacement anchor while preserving `0022` as historical context. | Should the next RFC action record `0022` as superseded by `10176`, or archive `0022` with a replacement note because `10176` is still Stage 3? |
| `10176` | Candidate project-state model covering epochs, phases, goals, tasks, authority semantics, and secondary entities. | `crates/exosuit-storage/migrations/V001__core_tables.sql` explicitly cites RFC `10176`; later migrations add sort keys, workspace active phase, phase ownership, and RFC/idea/inbox state; Exo commands read and mutate this SQLite-backed model. | This is the current anchor for the cluster. It describes the durable state shape that replaced the `0022` service model. | Keep as the reference point for project-state reconstruction; review later for stabilization after adjacent stable records are cleaned up. | Does `10176` need a narrow update for workspace focus and ownership before it becomes the canonical stable project-state record? |
| `10154` | Stable persistence rule: check `docs/agent-context` into version control. | `docs/design/agent-context-ownership.md` distinguishes operational SQLite state, repo-policy SQL projections, sidecar SQL projections, shadow state, RFC prose, and research/design/spec prose; `docs/agent-context/` is now a generated projection location, not a human-doc home. | Persistence is part of the same state cluster because the storage model determines what is source state, generated projection, sidecar state, or local shadow state. | Rewrite around repo projection, sidecar projection, and shadow-state policy after the `0022` disposition is decided. | Should the replacement remain a Stage 4 persistence RFC, or should it become a design note plus a smaller stable RFC about projection ownership? |
| `0114` | Withdrawn advanced phase transition record centered on close/pivot flow and archived `current/` artifacts. | PR `#187` moved RFC `0114` to `docs/rfcs/withdrawn/`, removed the committed `docs/agent-context/archive` corpus, and changed phase finish so it no longer creates phase archive snapshots. | This is the completed phase-archive disposition inside the project-state cluster. | Complete; use PR `#187` as the withdrawal pattern for archive/current-file-dependent records. | No further `0114` action in this cluster. |

## Current Evidence

- `0022` is the next focused target because it is a Stage 4 record whose
  central implementation claim no longer exists in current code.
- `10176` is the best current anchor because storage migrations and Exo state
  code implement its SQLite-backed entity model.
- `10154` remains an adjacent cluster member because persistence rules now
  depend on the same state boundary: operational SQLite state, generated
  projections, sidecar state, shadow state, and workspace-specific
  focus/ownership.
- `0114` has been handled by withdrawal in PR #187.

## Next Disposition Frame

The next RFC action should decide how to record the `0022` replacement:

| Option | Meaning | Tradeoff |
| --- | --- | --- |
| Supersede `0022` by `10176` | Records a direct replacement path from stale stable law to the current project-state model. | Clear for Exo read surfaces, but the target is still Stage 3. |
| Archive `0022` with a replacement note | Preserves `0022` as implemented-history prose while avoiding a stable-to-candidate supersession. | More conservative, but agents must read the research note or archive rationale to find `10176`. |

Either choice should preserve the useful historical point from `0022`: Exo needs
one authoritative project-state boundary. The current answer to that problem is
the SQLite-backed Exo state model, not the old TypeScript `ContextService`.
