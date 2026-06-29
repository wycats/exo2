<!-- exo:10064 ulid:01kmzxefdtd9y2kg61phgm4wn6 -->


# RFC 10064: Surgical Context

## Summary

This RFC proposes a fundamental shift in how the Exosuit Agent interacts with the codebase. We move from **Text-Based Editing** (fragile, blind, regex-heavy) to **Surgical Mutation** (structural, typed, reactive).

We introduce **`exosuit-surgeon`**, a system where the Agent operates on a **Live Reactive Graph** of the project using **Resumable Intents** (TypeScript). This is supported by a new data structure layer, **`exosuit-reactivity-collections`**, which implements "Indexing" as **Reactive Collections** with **Dependency Tracking**.

## Motivation

### The Problem: "Blind Surgery"

Currently, the Agent acts like a surgeon operating in the dark.

1.  **Blindness**: It reads a file (`cat`), gets a snapshot, and loses connection to the live state.
2.  **Fragility**: It edits by guessing line numbers or using regex. If the user types one character while the Agent is thinking, the edit applies to the wrong place (Race Condition).
3.  **Staleness**: The Agent's "Index" (e.g., list of functions) is a static list that is immediately out of date.

### The Solution: "The Living Patient"

We treat the codebase not as a collection of dead files, but as a **Living Graph**.

1.  **Visibility**: The Agent sees the AST (Abstract Syntax Tree) and Data Structure (TOML Tree).
2.  **Precision**: The Agent sends _intent_ ("Rename function X"), not _patches_ ("Replace line 10").
3.  **Reactivity**: The Agent's view of the world updates instantly via fine-grained signals.
4.  **Safety**: All operations are simulated in a "Ghost Layer" (Forked Revision) before being committed.

## Architecture

### 1. The Patient: Reactive Collections

This layer is defined in **RFC 10003: Reactive Collections**. It provides the "Cellular Data Structures" (Sequence, Map) that the Surgeon operates on.

**The "Split Brain" Architecture**:
We distinguish between the **Structural Backbone** (Reactive) and the **Semantic Overlay** (Computed).

#### The Reactive Backbone (Level 0)

The "Truth" of the system is **Reactive Collections** (see RFC 10003).

- **Abstraction**: `ReactiveSequence<T>` and `ReactiveMap<K, V>`.
- **Stability**: We maintain "Stable IDs" for regions (e.g., functions) via a lightweight map.

#### The Semantic Overlay (Level 1 & 2)

- **Level 1 (AST)**: Derived from the Text via incremental parsing. Modeled as `Computed<AST>`.
- **Level 2 (Graph)**: Cross-file references. These are **Reactive Queries**.
  - **No Symlinks**: We do not maintain manual links.
  - **Dependency Tracking**: A `Scope` captures dependencies during execution. `scope.find_references(symbol)` is a computed signal. If the symbol moves, the signal updates.

### 2. The Scalpel: Resumable Intents (TS)

The Agent does not write to disk directly. It dispatches a **Surgical Intent**.

- **Language**: TypeScript (running in Node.js Extension Host).
- **Execution**: Runs inside a `Transaction` scope.
- **API**: High-level "Smart Queries" (e.g., `ctx.query(selector).rename(newName)`). This pushes the iteration logic down to the Rust engine (via bulk APIs) to avoid the JS/Rust bridge overhead.

#### The Simulation Layer (Ghost Mode)

1.  **Fork**: The System creates a **Forked Revision** of the affected Cells. This leverages the existing `exosuit-reactivity` engine (no new "Overlay" struct needed).
2.  **Dry Run**: The Agent's script runs against this Fork.
3.  **Preview**: The User sees "Ghost Text" (the proposed state).
4.  **Commit**: If approved, the Fork is merged into the Live Graph.

#### Optimistic Concurrency

To handle race conditions (User types while Agent thinks):

- **Replay Intent**: The Surgeon reads `Chunk(Rev 1)`. It submits `Update(Chunk, Expected: Rev 1)`.
- **Conflict**: If the Live Chunk is at `Rev 2`, the update fails.
- **Retry**: The Agent is notified of the conflict and simply **Re-runs the Intent Function** against the new state.

### 3. The Monitor: Tangible Interface

The User must see what the Agent is doing.

- **Ghost Mode**: The Agent's "Cursor" is visible in the editor.
- **Intent Visualization**: Before the "Scalpel" cuts, the UI highlights the AST nodes that will be affected.
- **Sparse Replication**: The UI does not subscribe to the entire graph. It uses a **Viewport Protocol**.

## Operational Safety & Constraints

1.  **No Binary Bloat**: The "Index" is ephemeral. We do not pollute the repo with `.exosuit.db` files.
2.  **Supervisor Pattern**: Intents run with a strict timeout (`AbortController`). If they hang, they are killed.
3.  **Interface Boundaries**: The Agent sees a high-level `Collection` API.
4.  **Polyglot Core**: The Engine is pure Rust. The TS layer is just a client. This ensures the system can be used in standalone Rust environments.
5.  **Leak Verification**: The implementation MUST include a `verify_leaks()` test suite that asserts all "Ghost Nodes" (Forked Revisions) are dropped when the Transaction Scope ends.

## Detailed Design: `exosuit-surgeon`

The Surgeon is the "Active" component that dispatches intents.

### Core Primitive: Transient Computations

To support complex queries without polluting the global namespace, the Engine uses **Transient Computations** (Trace Flattening). These are computed values that exist only for the duration of a Query Scope and are not registered as persistent "Roots". The Core Spec proves that these are isomorphic to inlined functions.

## Workflow Example: "Add a Task"

1.  **User Intent**: "Add a task to fix the login bug."
2.  **Agent Action**:
    - Agent queries the `Plan` (Reactive Collection).
    - Agent identifies the insertion point.
    - Agent generates a **Surgical Intent**: `plan.addTask(...)`.
3.  **Simulation**:
    - Host runs script in **Ghost Mode** (Forked Revision).
    - User sees the new task appear in "Pending" state (Ghost Text).
4.  **Execution**:
    - User approves (or Auto-Commit).
    - Host merges the Fork into the **Live Graph**.
    - **Reactivity**: The `Plan` collection updates. The `Dashboard` UI updates _instantly_.
    - **Persistence**: The Host serializes the new state to `plan.toml` on disk.

## Axiom Alignment

- **Axiom 1 (Context is King)**: The Graph _is_ the Context.
- **Axiom 11 (Agent-First Tooling)**: The API is typed and structural, perfect for LLMs.
- **Axiom 13 (Reactive Glitch Freedom)**: The UI and Agent share the same Reactive State.
