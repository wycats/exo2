<!-- exo:10191 ulid:01ktd80t692d4f9j6gvhbmcqa6 -->

# RFC 10191: Sidecar Write Ownership and Stale Writer Fencing

**Status**: Idea
**Feature**: sidecar

## Summary

Sidecar write ownership defines when Exo is allowed to mutate and checkpoint a shared sidecar checkout.

The immediate rule is intentionally conservative: until Exo has a stronger collaboration model, only one active writer may auto-persist a shared sidecar checkout at a time. Other machines may read, import, and explicitly sync, but they must not silently checkpoint or push state when ownership is unclear.

This RFC turns the PR #130 owned-subtree hotfix into a broader operating contract. The hotfix prevents a stale writer from deleting another project subtree. This RFC defines how Exo should prevent stale writers from being active writers in the first place.

## Problem

A shared sidecar repository can be touched by multiple machines, daemons, MCP servers, and VS Code windows. Even when Git staging is scoped to `projects/<key>`, Exo still needs to know which runtime is allowed to checkpoint and push the sidecar state for a project.

Without an ownership contract, users and agents have to reason about details that should be Exo's responsibility:

- whether an old daemon is still running with stale sidecar behavior,
- whether this machine is the current writer for a sidecar key,
- whether auto-persist is safe or should be disabled,
- whether a sidecar checkout is being reused by another active workspace,
- how to hand ownership from one machine to another without raw Git inspection.

The result is uncertainty: users can import state only after mentally proving that no background process will stomp it.

## Design

A sidecar checkout has one active writer per sidecar key. The active writer is the only runtime allowed to perform automatic checkpointing: flushing SQLite state to SQL projection, committing sidecar state, and pushing opportunistically.

Read commands and explicit inspection commands may run without ownership. Mutations may update local SQLite state, but if checkpoint ownership is missing or stale, Exo must surface pending portability debt instead of silently auto-persisting.

Ownership should be represented as Exo-managed runtime state, not as a human-edited Git file. The marker should identify:

- sidecar key,
- sidecar root,
- workspace/project identity,
- machine identity,
- owning process/runtime identity,
- Exo binary identity,
- acquisition time and refresh time.

Exo acquires ownership before enabling auto-persist. If the marker points at a live compatible runtime for the same project, the current runtime may proceed. If it points at a dead runtime, Exo may reclaim it. If it points at a live incompatible runtime, Exo must disable auto-persist and report that another writer owns the sidecar.

Stale daemon and MCP detection is part of the ownership check. If Exo can prove that a scoped runtime for the current workspace is stale because the on-disk binary changed or the runtime resolves a different database, it should restart or replace that runtime once before reporting a user-facing problem.

Manual sidecar barriers such as `sidecar repo sync`, dogfood verification, and safe-to-move-machine checks may ask for ownership or explicit user approval. They should not pretend that auto-persist is healthy when ownership is blocked.

PR #130 remains the minimum data-loss guard. Even an owner may stage and commit only the current project's owned subtree plus explicitly owned sidecar metadata. Ownership is an additional precondition for automatic writes, not a replacement for scoped staging.

## User-Facing Behavior

When ownership is healthy, Exo should say that sidecar auto-persist is enabled for the current workspace.

When ownership is absent, Exo should say that sidecar state is durable locally but not yet portable, and should offer an Exo command to acquire or repair ownership.

When another live writer owns the sidecar key, Exo should say that another machine or runtime is currently the sidecar writer. It should not expose raw PID files, lock files, or Git internals as the primary message.

When a stale writer is detected and safely reclaimed, Exo should report that it refreshed sidecar write ownership.

## Implementation Direction

The first implementation should add ownership checks around automatic checkpoint and auto-push paths, not around ordinary reads.

The repair command should be explicit and observable. It may reclaim dead owners automatically when the evidence is local and strong, but live-owner conflicts must be reported as Exo state.

VS Code should surface ownership health in the sidecar status pane. If a sidebar action would require ownership, it should queue or recommend an agent-visible Exo action rather than silently taking ownership from another runtime.

## Drawbacks

Single-writer ownership is conservative. It can delay portability when two machines are legitimately being used in parallel.

Ownership markers can become another source of confusion if they are too low-level or if Exo reports them as raw files instead of product state.

## Alternatives

One alternative is to rely only on scoped staging and semantic merge. That prevents the worst foreign-subtree deletion, but it does not prevent stale writers from creating confusing commits or sync debt.

Another alternative is to make every sidecar write go through a hosted coordination service. That is out of scope for the current sidecar Git model.

## Unresolved Questions

- What exact marker format should represent sidecar write ownership?
- What machine identity is stable enough for ownership without becoming surprising across migrations?
- Which commands may reclaim dead ownership automatically, and which require explicit approval?
- Should ownership be per sidecar key, per sidecar root, or per workspace binding?

## Future Possibilities

This RFC is the safety baseline for epoch-owned collaboration. Once Exo can reliably fence stale writers, it can define narrower ownership units that allow multiple machines or agents to collaborate concurrently without a single global writer.
