# Lane-Centered Workbench Source Map

This map explains how the lane-centered design package relates to existing Exo source strata.

It is not an exhaustive RFC inventory. It is a current-reading map for agents so they do not treat the RFC corpus as a flat pile of equally current instructions.

## How to use this map

When implementing lane-centered workbench features:

1. Read the design package first.
2. Read [`docs/vision.md`](../../vision.md) as the current workspace-centered architectural overview.
3. Use this map to find existing decisions that should be preserved.
4. Treat unimplemented, withdrawn, or proposal-stage material as historical unless this map, a current manual page, or an explicit adoption RFC marks the idea as active.
5. Treat Stage 3+ implemented RFCs and current manual/reference docs as authoritative for implemented behavior unless a specific superseding source is named.
6. When sources conflict, surface the conflict instead of silently choosing the most recent or nearest document.

This map narrows the RFC corpus for lane-centered design work; it does not give agents permission to ignore implemented contracts merely because they are not enumerated here.

## Current implementation baseline

The lane design starts from the current workspace-centered implementation:

- project identity, workspace views, sidecar policy, and `exo project move-root` reconciliation are described by RFC 10184;
- task and goal completion use outcome review language and preserve review evidence in project state;
- phase ownership is the implemented mutation boundary for phase-scoped work;
- reactive SQLite write mediation and trace validation are implemented for ordinary Exo state writes;
- MCP exposes the CLI-shaped `exo-run` command language, while RFC 10190 describes the next durable `exo-mcp` proxy boundary.

The first lane implementation should extend these contracts. It should not replace phase ownership, sidecar binding, outcome review, reactive storage, or MCP command semantics as part of the initial proof.

## Preserved foundations

These source strata should be treated as active foundations for the lane model.

### Project / workspace / worktree split

The lane model depends on the distinction between project identity and workspace view.

Preserve:

- one durable project identity;
- many workspace roots / linked worktrees;
- project state shared across worktrees;
- workspace-local focus and observations;
- daemon/runtime rooted in resolved project state, not blindly in the current checkout.
- sidecar-backed workspace relocation through `exo project move-root` when a checkout path changes.

Lane refinement:

- a lane may be focused in one workspace view;
- a lane may be associated with a branch or worktree;
- absence from one worktree is not evidence that project-level state is obsolete;
- portable lane rows should not depend on path-derived local project ids unless import remaps them.

### Storage disposition

Preserve the distinction between canonical state, tool configuration, documents, and policy-controlled projections.

Lane refinement:

- lanes, portable lane relationships, portable signals, and durable status inputs are structured steering state;
- workspace-local focus pins, raw checkout paths, runtime snapshots, dirty-tree observations, and other machine-local facts stay out of repo and sidecar dumps unless a focused migration creates an explicit portable summary;
- structured portable lane state should participate in the same canonical-state and SQL-dump projection policy as comparable Exo steering tables when repo or sidecar policy requires portability/reviewability;
- deterministic SQL dumps remain valid projection artifacts for diff, review, and reconstruction;
- RFCs remain prose documents with indexed metadata;
- mockups and design references are documents, not generated projections of SQLite state.

### Shared perception

Preserve the idea that user feedback, system observations, and plan changes should reach the agent at relevant action boundaries.

Lane refinement:

- perception events become user-facing **signals**;
- signals are scoped to lanes, goals, tasks, PRs, RFCs, validations, or workspace views;
- signals should reuse or bridge the existing inbox/perception channel rather than becoming an invisible fork;
- closure should surface unresolved signals in full.

### Machine channel / semantic operations

Preserve the idea that CLI, MCP, VS Code, LM tools, and future workbench surfaces should share one semantic operations layer.

Lane refinement:

- lane operations should have explicit effects;
- UI adapters should not reimplement lane semantics;
- status explanations should come from the same state the agent reads.

### Sidecar and state sync contracts

Preserve the separation between local durability, checkpointing, remote portability, and writer ownership.

Lane refinement:

- user-facing UI should say **State sync**;
- state sync is a health facet, not a single dirty bit;
- runtime drift and sidecar conflict are different kinds of degraded state;
- sidecar SQL projection should carry portable lane state without baking in path-derived project ids.

### Goals as PER cycles

Preserve the goal sizing rule: a goal is one Prepare → Execute → Review sized unit.

Lane refinement:

- goals live under lanes for active work;
- the lane gives goals workspace, branch, PR, RFC, validation, and signal context;
- current-goal UI should show hypothesis, evidence, divergences, and next move rather than a generic checklist.

### Validation lanes

Existing Exohook validation lanes are a separate implemented concept. They should remain validation lanes, and any future workbench-lane command surface must avoid conflating them with active work streams.

Lane refinement:

- product surfaces may say **lane** when the workbench context is clear;
- implementation docs should say **workbench lane** or **work lane** when disambiguation matters;
- Exohook/check grouping should continue to say **validation lane**;
- validation freshness should account for dirty source worktrees, not only commit SHA.

## Reframed by lanes

These source strata contain useful material, but the lane model changes their role.

### Phases as the only active-work axis

Earlier Exo material often treats phases as the primary active unit. Lane-centered workbench keeps phases as useful planning bands, campaigns, or historical structure, but the active work stream becomes the lane.

Do not mechanically rename phase to lane. Translate based on role:

- phase as planning band → keep as phase/campaign;
- workspace-active phase → likely workspace-local lane focus;
- PR-sized active work → workbench lane;
- phase goals → lane goals when attached to an active stream.

### PR-as-phase or PR-as-review-artifact sketches

The lane model preserves the insight that PRs are review artifacts, but rejects making PRs the core product object.

A PR can be attached to a lane, park a lane, reactivate a lane for repair, or provide closure evidence. The lane is where work happens; the PR is where review is public.

### Sidebar workbench concepts

Sidebar concepts remain valuable, especially compact status and shared perception. They should be rebuilt as lane-aware views rather than phase-only views.

The HTML mockups in this package are the current taste reference for density, hierarchy, and terminology.

### Inbox as backlog

Unscoped backlog items should remain ideas or deferred work. Scoped communication that should affect current behavior becomes a signal through the shared perception channel.

## Likely superseded or historical

These patterns should not be revived unless a new design explicitly reopens them.

- Hand-edited TOML/file mirrors of SQLite-canonical steering state.
- Treating deterministic SQL dumps as human-editable source of truth instead of policy-controlled projections.
- Treating chat/session memory as project truth.
- A single global active phase as the only work focus.
- Raw Git or GitHub state as sufficient lane truth without provenance.
- UI surfaces that own or mutate state independently of Exo operations.
- RFC markdown scans that treat one workspace's file tree as complete global truth.

This section does not supersede implemented Stage 3+ behavior by implication. In particular, it does not supersede repo/sidecar SQL dump projections for canonical state. Supersession should name the specific source being superseded.

## Still open

The design package intentionally leaves these as future implementation questions:

- exact lane schema and migration path;
- final CLI/API namespace after checking validation-lane collisions;
- whether lane observations are fully normalized or stored as typed JSON facts;
- how to expose lane focus in VS Code without disrupting existing phase focus;
- how much PR state to mirror locally vs. derive on sync;
- how to represent RFC document observations across worktrees;
- browser workbench architecture;
- cross-lane conflict detection;
- lane close / outcome review operation shape;
- sidecar sync conflict UX.

## Recommended consolidation posture

Do not rewrite RFC history. Keep RFCs as durable history and create a current architecture layer that says what the system believes now.

[RFC 10202: Lane-Centered Workbench Adoption](../../rfcs/stage-2/10202-lane-centered-workbench-adoption.md)
records the adoption decision and implementation-ready first-proof contract.
RFC 10202 is authoritative for that first proof, including its `exo lane`
namespace and its deliberate deferral of attachments, signals, validation
freshness, and derived status. This design package remains the broader product
exploration and source material for later work; where it describes a larger or
older implementation shape, the Stage 2 RFC controls.
