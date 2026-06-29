# Lane-Centered Workbench Implementation Brief

This note is the agent-facing bridge from the design package to incremental implementation.

It is not a second design. The design source is `proposal.md`, with `mockups.html` as the primary visual and interaction reference.

## Invariants to preserve

1. A workbench lane is an observable execution stream, not a synonym for branch, PR, worktree, phase, validation lane, or chat.
2. Project state, workspace view, and observations are separate layers.
3. Lifecycle and health facets are separate.
4. `Why this status?` is a product explanation, not a debug dump.
5. Agents should not infer lane truth from chat, branch names, raw GitHub state, or commit SHA alone when the workspace is dirty.
6. UI adapters render canonical state; they do not own it.
7. The HTML mockups are a primary design artifact and should influence hierarchy, density, and terminology.
8. First implementation should prove one narrow loop before building the full workbench UI.

## First useful slice

The first implementation slice should make workbench lanes real in state and tools without attempting the whole workbench UI.

Implement conceptually:

- a minimal lane entity in canonical project state with a stable portable `text_id`;
- portable lane rows that do not bake in path-derived local project ids;
- policy-controlled SQL dump/projection participation for portable lane facts, with workspace-local focus and raw observations excluded or summarized explicitly;
- a disambiguated command namespace for create/list/show/focus that fits the current one-namespace/one-operation router shape;
- a workspace-keyed focus relation;
- a lane checkout/attachment relation separate from focus;
- lane-scoped signals through the existing shared perception inbox/channel rather than a separate invisible store, including any required inbox enum/source migration;
- validation freshness that accounts for commit, dirty source state, and content/fileset fingerprint when available;
- park if the storage/lifecycle shape makes it straightforward;
- machine-channel exposure for those operations;
- a compact VS Code/sidebar rendering if the command surface is stable enough;
- one basic `Why this status?` explanation for focused-lane status.

The final command spelling should be chosen after reconciling with existing Exo uses of “lane,” especially Exohook validation lanes. Product UI can say **lane** in workbench contexts, but implementation should use a router-compatible namespace/operation pair such as `exo workbench lane-create`, `exo workbench lane-focus`, and related `workbench lane-*` operations unless the router is deliberately extended first.

The first slice may attach a lane to the current workspace and branch, but it should not require a separate worktree or PR.

## Explicitly out of scope for the first slice

- full browser workbench UI;
- full RFC axis;
- PR review repair workflow;
- sidecar sync automation;
- cross-lane conflict detection;
- lane close / outcome review;
- automatic worktree creation;
- migration of existing phases into lanes;
- replacing current phase/goal/task behavior;
- replacing or renaming Exohook validation lanes.

## Recon checklist before editing

Before implementation, do a read-only recon pass:

- Find existing command namespace patterns for goal/task/phase.
- Find any existing Exohook validation-lane command, docs, templates, or schemas.
- Find storage migration conventions for canonical state.
- Find SQL dump/projection conventions for repo and sidecar policy.
- Find the existing inbox/perception event schema and closure guards.
- Find machine-channel exposure and effect annotation patterns.
- Find VS Code tree/sidebar rendering patterns.
- Identify how focused workspace state is currently represented.
- Identify how validation handles dirty worktrees and uncommitted changes.
- Identify tests that should be extended rather than duplicated.

Report the smallest coherent implementation path before editing.

## PER discipline for the first implementation lane

Run the work as a bounded Prepare → Execute → Review cycle.

Prepare should state:

- the minimal schema hypothesis;
- which existing commands are the closest pattern;
- whether `exo lane` is safe or whether a disambiguated, router-compatible namespace is required;
- whether lane state needs a new SQL dump/projection and how path-derived ids, workspace-local focus, and raw observations are excluded, summarized, or remapped;
- how lane signals reuse the shared perception channel and what inbox schema migration is required for lane scopes;
- how validation freshness is computed for dirty workspaces;
- where machine-channel exposure is likely generated or declared;
- what could make the first slice too large;
- what can be safely deferred.

Execute should make the smallest durable change that proves lane identity and workspace-local focus.

Review should verify:

- a lane can be created and focused in the current workspace;
- a later command can read the focused lane without chat history;
- focusing a lane from one workspace does not overwrite another workspace's focus;
- portable lane state is not tied to one checkout's path-derived project id;
- lane-scoped signals remain visible through existing inbox/perception surfaces and guards;
- validation freshness treats dirty source changes as stale unless covered by the run;
- command effects are correctly exposed;
- existing phase/goal/task behavior remains intact;
- existing validation-lane behavior remains intact;
- the implementation preserves the project/workspace/observation distinction.

## The first proof

The first slice is successful when this is true:

> An agent can create, focus, and resume a lane from canonical project state without relying on chat history.

If a proposed step does not help prove that sentence, it probably belongs to a later lane.
