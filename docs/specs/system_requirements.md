# **System Requirements & Invariants: Validation-Based Architecture**

This document formalizes the constraints, phases, and invariants required to implement RFC 0037 safely. It specifically addresses the "Ember-style" constraints regarding data flow and side effects.

## **1. Execution Phases**

To prevent loops and ensure consistency, the system divides execution into two strict phases.

### **Phase A: The Action Phase (Mutation)**

- **Behavior:** Standard imperative programming.
- **Permissions:** Read/Write allowed on all Cells.
- **Reactivity:** Changes are not propagated immediately. Writes update the CurrentTransactionID.
- **Ordering:** Reads and writes occur instantly in memory.
- **Constraint:** No "Reacting" happens here. This is purely for updating the source of truth based on user input or IO.

### **Phase B: The Render Phase (Derivation)**

- **Behavior:** Functional derivation of state.
- **Permissions:** **Read-Only** (with the Lazy Init exception, see Appendix).
- **Reactivity:** Computations track dependencies (Traces).
- **Constraint:** This phase runs _after_ a Transaction closes.

## **2. The Core Invariants**

### **Invariant I: The Backflow Constraint**

"You cannot write to a Cell once it has been read during a Transaction."

Formally, for any Cell $C$ and Transaction $T$:  
If $Read(C, T)$ has occurred, then $Write(C, T)$ is Forbidden.

- **Reasoning:** If you read $C$ and get value $V_1$, then write $V_2$ to $C$, the system is now inconsistent. A downstream consumer might have cached $V_1$, but the final state of the transaction is $V_2$.
- **Enforcement:** The system must throw a hard error if a Write occurs on a Cell that has a LastReadTransaction == CurrentTransaction.

### **Invariant II: No untrack**

"There is no mechanism to read a Cell without recording a dependency."

- **Constraint:** Every read inside a Computation _must_ result in a dependency in the Trace.
- **Reasoning:** untrack breaks the "Pure Check" validation model. If a function reads state that isn't in the Trace, IsValid returns true (valid trace) even when the output should have changed.

### **Invariant III: Pure Validation**

"The Validation process depends ONLY on Cell Revisions, never on Computations."

- **Constraint:** The IsValid check cannot execute user code (getters, proxies, etc.).
- **Reasoning:** Validation must be $O(TraceLength)$, not $O(UserLogic)$.

## **Appendix: Implementation Constraints**

### **A. The "Lazy Initialization" Pattern**

A strict Read-Only Render Phase breaks common patterns like lazy caching.

The Refined Constraint:  
A Write is allowed during Render IF AND ONLY IF no other consumer has observed the EMPTY state during the current Frame.  
**Implementation Strategy: Generation Counters**

1. Cells have a GenerationID (Epoch + Counter).
2. When a Computation reads a Cell, it records (CellID, GenerationID).
3. Lazy Init does _not_ increment the GenerationID visible to the outside world, OR the system guarantees that the EMPTY state was never exposed.

### **B. Distributed Integrity (Verifiable Fetch)**

When the client requests data, we must prevent "Tearing" (Time-of-Check vs Time-of-Use race conditions).

Constraint:  
The Server MUST implement Verifiable Fetches.

1. **Request:** Fetch(RootID, ExpectedDigest)
2. **Logic:** The Server must strictly compare ExpectedDigest against the current state of RootID (or its history).
3. **Rejection:** If they do not match, the Server MUST return a StaleRevision error.
4. **No Streaming:** The server cannot stream the file response before performing this atomic check.

### **C. The Reincarnation Guard (Epochs)**

To handle server crashes/restarts safely:

1. **Server Boot:** Generate a random UUID (ServerEpoch).
2. **Revision Structure:** All in-memory revisions are tuples: (ServerEpoch, Counter).
3. **Validation:** If a Client presents a Revision with a mismatched ServerEpoch, it is strictly **Invalid**, regardless of the counter value.

### **D. Cache Eviction Policy (The Swiss Cheese Rule)**

To prevent creating valid snapshots that point to missing data:

1. **Tagging:** The cache must track whether a computed node is **Pure** (re-derivable) or **Impure** (Generative ID).
2. **Priority:** When memory pressure requires eviction, the system **MUST** evict Pure nodes before Impure nodes.
3. **Reasoning:** Evicting an Impure node destroys the Nonce, making it impossible to reconstruct the specific Revision ID required by the Snapshot.

### **E. History Buffer Mechanics**

The HistoryBuffer allows for Snapshot Pinning (solving Zeno's Paradox).

**Constraint 1: Time-Based Eviction (Mandatory)**

- The eviction policy **MUST** be Time-Based (TTL), ensuring data remains available for delta_retention > RTT.
- **Forbidden:** Count-Based eviction (e.g., "Keep last 10"). This re-introduces starvation for high-frequency updates.
- **Optimization:** Lazy TTL (checking timestamps on insert/read) is acceptable to avoid background threads.

**Constraint 2: Storage Optimization**

- The History Buffer **MUST NOT** store the full WorldState map.
- It SHOULD store only the RootNode (Merkle Root).
- **Reasoning:** Since the Store is content-addressable (CAS), the Root is sufficient to traverse and find any value. Storing the full map is redundant and memory-intensive.

### **F. Resource Safety (Concurrency & Error Types)**

To prevent leaks and loops in the Resource layer:

**Constraint 1: Structured Concurrency Only (Zombie Prevention)**

- **Forbidden:** tokio::spawn or any "Detached" task inside a Resource Init.
- **Requirement:** All async work must be bound to the returned Future. This ensures that when the Scope is disposed (and the Future is Dropped), the work is cancelled.

**Constraint 2: Error Normalization (NaN Trap Prevention)**

- **Forbidden:** Using std::error::Error trait objects or any Error type that relies on reference equality.
- **Requirement:** The Reason in Availability::Absent MUST be a value type (Enum/Struct) that implements PartialEq.
- **Strategy:** Wrap external errors: Absent(Reason::IOError(e.kind(), e.to_string())).

### **G. The "NaN" Fix (Canonical Value Equality)**

**Vulnerability:** Rust's `f64` implements `PartialEq` but `NaN != NaN`. A computation returning `NaN` triggers an infinite invalidation loop.

**Constraint:** You must implement **Canonical Value Equality**.

- **Action:** Do not store raw `f64`. Wrap it in a type where `PartialEq` explicitly handles `NaN` as equal (`is_nan() && is_nan() -> true`).

### **H. The "Async Drop" Fix (Deferred Cleanup)**

**Vulnerability:** `Drop` is synchronous. A Resource cannot perform async network cleanup (e.g., closing a remote transaction) during disposal, leading to distributed leaks.

**Constraint:** Implement **Deferred Cleanup**.

- **Action:** Expose `runtime.defer_cleanup(Future)`. When a Resource is dropped, it can hand off a cleanup task to the Runtime, which runs it in a bounded background pool. This is the _only_ exception to the "No Detached Tasks" rule.
- **Limit:** This queue must be bounded (Semaphore) to prevent resource exhaustion attacks.
