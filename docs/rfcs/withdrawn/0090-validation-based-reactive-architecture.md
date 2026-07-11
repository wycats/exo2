<!-- exo:90 ulid:01kg5kp2fe2cvkw7g6xpszekfw -->

# RFC 90: Validation-Based Reactive Architecture

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 0090: Validation-Based Reactive Architecture

## The Problem

Synchronizing state between the Exosuit Core (Rust/WASM) and the VS Code Extension Host (TypeScript) is currently fragile.

- **Fragility**: Ad-hoc message passing leads to "glitches" (UI diverging from source of truth).
- **Chattiness**: Traditional "Push" reactivity (sending values over the wire) is expensive and hard to verify.
- **Verification**: It is difficult to prove that a specific sequence of events results in a correct state.

## The Solution: Validation-Based Reactivity

We propose a **Validation-Based Reactivity** model. This inverts the typical data flow:

1.  **Server**: "Something changed." (Push Notification)
2.  **Client**: "Here is what I am displaying (Digests). Is it valid?" (Pull Validation)
3.  **Server**: "Root A is valid. Root B is invalid."
4.  **Client**: "Send me the new state for Root B." (Fetch)

This separates **Notification** (cheap, frequent) from **Reconciliation** (precise, atomic).

## Core Primitives

### 1. Cells, Addressing, and Revisions

All mutable state is modeled as a **Cell**.

#### Revisions & The Reincarnation Fix

To prevent identity collisions across server restarts (The Reincarnation Bug), memory revisions are **Epoch-Scoped**.

- **Disk Revision**: `Hash(Content)` (inherently stable).
- **Memory Revision**: `(ServerEpochID, MonotonicCounter)`.
  - `ServerEpochID`: A UUID generated at server boot.
  - `Counter`: A standard incrementing integer.
  - **Constraint**: A client holding a revision from Epoch A implies total invalidation if the current server is in Epoch B.

#### Cell Addressing

A Cell is not limited to a whole file. We support **Sub-Structural Addressing**. A Cell ID is formally a tuple:

- **Source ID**: The root identity (e.g., filepath).
- **Pointer**: A structural path into the content (e.g., JSON Pointer `#/debug/enabled` or Rust Item Path `::main::App`).

#### Granularity & Stability

- **Coarse Cells**: Represents the whole file. `Revision = Hash(FileContent)`.
- **Fine Cells**: Represents a fragment. `Revision = Hash(FragmentContent)`.
  - **Performance Note**: Fine-grained extraction can be $O(N)$. Implementations must ensure structural extraction is backed by an index or Memoized Tree to avoid the "Granularity Trap" (where validation costs dominate execution).
- The system allows mixing granularity via **Conservative Approximation**. It is valid to use the Coarse Revision for a Fine Cell (false positive invalidation), but invalid to use a Fine Revision for a Coarse Cell.

### 2. The "Trace" Model (No Graph Required)

We do not maintain a persistent dependency graph in memory. Instead, we use **Traces**.

- When a computation runs, it produces a **Trace**: a list of `(Cell ID, Revision ID)` pairs accessed during execution.
- This Trace is the _only_ thing needed to validate the result later.
- **The Invariant**: "Graphy-ness" exists in the code (loops, recursion), but the Trace is always a directed list/tree.

### 3. Validation is Pure (No Execution)

To ensure performance, the **Validation Phase** never runs user code.

- **Input**: A list of `(Cell, Revision)` from the Trace.
- **Logic**: Check if `CurrentRevision(Cell) == RecordedRevision` (Strict Equality).
- **Output**: `True/False`.
- **Intermediate Cutoff**: We explicitly **do not** support intermediate cutoff during validation. If an input changes, the computation is marked invalid. Optimizations (deduplication) happen during the _Fetch_ phase, not the _Check_ phase.

### 4. Frame Consistency (Atomic Snapshots)

The Client must never display a "torn" state (e.g., File List from $T_1$ and Editor Content from $T_2$).

- **Mechanism**: The Client requests validation for _all_ active UI roots in a single batch.
- **Guarantee**: The Server processes this batch against a single, atomic snapshot of the Revision State.

## The Algebra

We define the system using a **Revision Algebra**:

- **Revisions ($\mathcal{R}$)**: A set containing Epoch-Scoped Counters and Content Hashes.
  - $\mathcal{R}_{mem} = \text{UUID}_{Epoch} \times \mathbb{Z}_{Counter}$
- **Trace Digest**: A Merkle hash of the dependencies. This allows us to uniquely identify the "version" of a computation by hashing its inputs.
  - $D_{total} = Hash(D_{dependency_1} + D_{dependency_2} + \dots)$

## Implementation Strategy

### The "Wire" Protocol

The Client acts as a **Shadow Graph** of the Server.

1.  **Signal**: Server sends `Unit` (`{}`) to Client. "Universe Dirty."
2.  **Validate**: Client sends `Map<RootID, Digest>` to Server.
3.  **Response**: Server returns `Map<RootID, Status>`.
    - **Valid**: Client keeps current data.
    - **Invalid**: Client schedules a fetch.
4.  **Contextual Fetch (Verifiable Fetch)**: Client requests data for Invalid roots, **explicitly passing the expected Digest**.
    - If the Server's current digest matches the Client's request, the data is returned.
    - If the Server state has advanced and the digest mismatches, the request is rejected (`Stale`), forcing a re-validation.

#### Liveness Trade-off (Zeno's Paradox)

This protocol guarantees **Safety** (no tearing) at the cost of **Liveness**. If the server updates faster than the network Round Trip Time ($t_{update} < t_{RTT}$), the Client may be starved (continuously receiving Stale rejections).

- **Mitigation**: The UI should indicate "Loading" during this state.
- **Constraint**: High-frequency updates should be debounced at the Source level to ensure $t_{update} > t_{RTT}$ on average.

### Serialization & Bundles

Because the state is defined entirely by the **Trace** (a list of IDs), we can serialize the state of the IDE to a file.

- **Scenario**: User closes VS Code.
- **Action**: Serialize the `Map<RootID, Digest>` and the underlying Data Cache.
- **Restore**: On load, send the Digests to the Server. If the files haven't changed, the UI restores instantly without re-computing anything.

## Robustness & Constraints

- **Impurity Scope (Session Consistency)**:
  Impure inputs (e.g., `Math.random`) are "coupled" to the liveness of tracked dependencies. However, this determinism is scoped to the **Cache Session**. If the server experiences memory pressure and evicts the cache entry, the impurity may "re-roll" upon re-execution. We guarantee **Session Consistency**, not **Global Consistency**, for impure computations.
- **Cycles**:
  Cycles are structural errors. Computations must be pure. If strict DAG constraints (Action Phase vs. Render Phase) are followed, cycles are impossible.

## FAQ

**Q: Why not use RxJS / Signals / Push?**
Push systems require $O(Events \times Listeners)$ complexity and are prone to race conditions (glitches) over async boundaries. Validation systems are $O(ActiveRoots)$ and self-correcting.

**Q: Doesn't "Pull" introduce latency?**
The latency is effectively the same. In Push, you wait for the message. In Pull, you receive a signal and immediately fetch. The bandwidth savings (not sending data for hidden tabs) outweigh the extra round-trip for visible data.

