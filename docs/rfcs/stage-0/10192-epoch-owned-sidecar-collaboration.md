<!-- exo:10192 ulid:01ktd81psjhxb9n1722rjqpkhs -->

# RFC 10192: Epoch-Owned Sidecar Collaboration

**Status**: Idea
**Feature**: sidecar

## Summary

Epoch-owned sidecar collaboration is the future model for safe concurrent Exo work across machines and agents.

The goal is to move beyond a single global sidecar writer by making ownership explicit at a collaboration boundary: an epoch or workstream. Machines and agents should be able to work on different owned epochs without stomping each other, while Exo reports conflicts as Exo planning conflicts instead of raw Git or SQL failures.

This RFC is design-only until sidecar write ownership and stale writer fencing are stable.

## Problem

Single-writer ownership is safe, but it is not the end state. Exosuit's planning model already has epochs, phases, goals, tasks, inbox items, and RFCs. Users will naturally want multiple agents or machines to make progress on different workstreams at the same time.

Sidecar SQL projection can preserve non-overlapping rows, but row-level merge alone does not answer collaboration questions:

- who is responsible for an epoch,
- whether another machine may move a goal into a phase,
- whether two agents are editing the same planning surface,
- how VS Code should show pending collaboration requests,
- when a semantic conflict needs human or agent review.

Without an ownership model tied to Exo concepts, concurrency remains accidental.

## Design

The ownership unit is an Exo workstream. The default workstream boundary is an epoch. A future implementation may allow narrower boundaries, but the first design should treat an epoch as the smallest independently owned collaboration unit.

An epoch owner may mutate the epoch's phases, goals, tasks, inbox links, and RFC links when those rows belong to the owned epoch. A machine or agent that does not own the epoch may read it and may queue an agent-visible request, but should not silently apply a conflicting structural mutation.

Ownership must be visible as Exo state. The sidebar and MCP surfaces should be able to show which epochs are locally owned, remotely owned, available, or in conflict. The user should not need to inspect sidecar Git history to understand who is working where.

Compatible concurrent writes are writes to different owned workstreams, or writes that Exo can prove are commutative within the same workstream. Incompatible writes are semantic conflicts: two actors changed the same logical row or made structural changes that cannot both be true.

When a conflict occurs, Exo should report the Exo entity: epoch, phase, goal, task, inbox item, RFC, title, status, and proposed action. Raw SQL row ids and Git conflict hunks are diagnostic detail, not the normal product surface.

## Agent and Sidebar Behavior

Agents should perceive ownership state before mutating. If an agent is asked to move a goal in an epoch owned elsewhere, it should create or surface a request rather than applying the mutation directly.

VS Code sidebar actions that reorganize or edit non-owned workstreams should follow the existing agent-visible recommendation pattern: queue a structured request for the agent instead of silently mutating sidecar state.

The sidebar should make ownership understandable without turning it into Git UI. Suggested states are: local, available, remote, pending review, and conflict.

## Relationship to Single-Writer Ownership

Single-writer ownership fences stale runtimes and makes automatic checkpointing safe. Epoch-owned collaboration narrows the unit of ownership after that safety baseline exists.

Until single-writer ownership is implemented and dogfooded, this RFC must not be used to justify multiple automatic sidecar writers.

## Implementation Direction

The first implementation slice should be observational: expose workstream ownership state in Exo status, MCP JSON, and the sidebar without changing mutation policy.

The second slice should route non-owned workstream mutations into structured inbox requests.

The third slice should allow explicit ownership acquisition for an available epoch and make sidecar sync preserve compatible epoch-owned changes.

Only after those slices are stable should Exo enable concurrent automatic checkpointing by epoch.

## Drawbacks

Epoch ownership may feel heavier than free-form collaboration, especially for small personal projects.

Some Exo entities cross epoch boundaries. Moving a goal between epochs, changing global RFC metadata, or changing shared inbox state may require explicit handoff or review.

## Alternatives

One alternative is row-level semantic merge only. That can preserve data, but it does not give users or agents a clear collaboration model.

Another alternative is branch-per-machine sidecar state. That makes Git isolation explicit but pushes too much raw Git workflow into the product.

Another alternative is to keep single-writer ownership permanently. That is safe, but it leaves Exosuit short of its multi-agent collaboration goals.

## Unresolved Questions

- Is an epoch always the right first ownership unit, or should phases be ownable earlier?
- Which global entities are outside epoch ownership?
- How should Exo represent ownership expiration, renewal, and handoff?
- What sidebar interactions should queue requests versus ask the user to acquire ownership?
- What dogfood scenario proves epoch-owned collaboration is safe enough to implement?

## Future Possibilities

Epoch ownership can evolve into richer collaboration lanes: agent-owned goals, review-owned inbox queues, temporary edit leases, and live collaboration indicators across Exo surfaces.
