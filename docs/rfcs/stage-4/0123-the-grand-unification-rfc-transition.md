<!-- exo:123 ulid:01kg5kp2h5q2r1s65n0tybhw2k -->

# RFC 123: The Grand Unification (RFC Transition)


# RFC 0123: The Grand Unification (RFC Transition)

## Summary

A comprehensive plan to fully adopt the Staged RFC process as the backbone of the Exosuit project. This involves transitioning from a mix of ad-hoc scripts, design docs, and "Session Laws" to a unified workflow managed by a strongly-typed CLI (`exo`) and a rigorous distinction between historical decisions and current authoritative documentation in stabilized RFCs.

## Motivation

- **Incoherent State**: The project currently has "Design Docs", "RFCs", "Axioms", and "Decisions" scattered across different formats and directories.
- **Tooling Fragmentation**: The agent workflow relies on fragile bash scripts that are hard to maintain and test.
- **Missing History**: Many existing features (Daemon, IPC, Core) lack a formal RFC, making it hard to understand _why_ they were built that way.
- **Drift**: Authoritative documentation is fragmented, leading to confusion between "what we planned" and "what is".

## The Plan (Epoch 12)

We will execute this transition in four distinct phases.

### Phase 28: The `exo` CLI (RFC 0009)

We will rename and expand the `rfc-status` tool into a general-purpose agent CLI.

- **Rename**: `tools/rfc-status` -> `tools/exo`.
- **Port Logic**: Move logic from `scripts/agent/*.sh` to Rust subcommands:
  - `exo context restore`: Generates the full project context.
  - `exo phase prepare/start/status/finish`: Manages the granular phase lifecycle, enforcing a mandatory "Steering" pause between phases.
- **Deprecation**: Delete the bash scripts once the CLI is verified.

### Phase 29: The Great Backfill

We will ensure every significant feature has a corresponding RFC.

- **Audit**: Identify features without RFCs (e.g., `exosuit-core`, `exosuit-rtd`, `Sidebar`).
- **Backfill**: Create Stage 3 (Candidate) or Stage 4 (Stable) RFCs for these features.
  - Use the updated RFC template.
  - Fill in all sections (including "Context Updates") _as if_ they went through the process.
- **Goal**: A complete historical record of _decisions_ that includes instructions for the Manual.

### Phase 30: RFC Stabilization Sync

We will establish Stage 3/4 RFCs as the authoritative "Record of Reality" for implemented features.

- **Execute**: Walk through the "Context Updates" section of the backfilled RFCs to align stabilized RFC prose with implemented reality.
- **Consolidate**: Move descriptive content into the relevant Stage 3/4 RFCs instead of a separate manual tree.
- **Distinguish**: Early-stage RFCs remain proposals; Stage 3/4 RFCs are the descriptive record of the current system.
- **Link**: Ensure stabilized RFCs cite the relevant source RFC lineage.

### Phase 31: Legacy Purge

We will remove the scaffolding that is no longer needed.

- **Delete**: `docs/design/` (migrated to RFCs).
- **Delete**: `docs/future/` (migrated to Stage 0 RFCs or Ideas).
- **Cleanup**: Remove legacy prompts that are replaced by `exo` commands.

## Outcome

At the end of this Epoch:

1.  **Single Source of Truth**: Stabilized RFCs describe the system.
2.  **Complete History**: RFCs describe the decisions.
3.  **Unified Tooling**: The `exo` CLI manages the workflow.
4.  **Clean Workspace**: No vestigial directories or scripts.
