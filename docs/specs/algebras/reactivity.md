# **Formalization of Validation-Based Reactivity**

RFC 0037 Working Group  
Version: 5.0 (Observation Kinds Added)

## **0. The Fundamental Principle: Zero-Execution Validation**

The central invariant of this architecture—"The Iron Rule"—distinguishes it from Push-based systems.

**Rule:** Validation is a **Metadata Operation**, never a **Data Operation**.

Let $f$ be a computation and $\mathcal{T}$ be its trace.  
The validity check $\text{IsValid}(\mathcal{T})$ operates exclusively on Revision IDs (Integers, Hashes, Nonces). It MUST NOT execute user logic, read file content, or inspect the values of cells.  
$$\text{Cost}(\text{IsValid}) \propto O(|\mathcal{T}|) \ll \text{Cost}(f)$$  
Implication:  
The system determines whether to run code without running code. If the Metadata (Revisions) matches, the Data (Values) must match (by the definition of Purity), so execution is skipped entirely.

## **1. Desiderata**

The following desiderata express the design goals of the formalism. They are not axioms (they do not state rules) but **adequacy criteria**: if the axioms cannot express a distinction required by a desideratum, the formalism is inadequate.

### **Desideratum: Identity-Equivalence Separation**

The formalism must distinguish a cell's **identity** (how it is addressed and referenced) from its **equivalence** (how change is detected). A revision bump represents a transition to a new equivalence class at a stable identity.

- **Identity** is structural: it is the address by which a cell is referenced ($\text{CellId} = \langle \text{Source}, \text{Pointer} \rangle$).
- **Equivalence** is semantic: it is determined by comparing revisions, which serve as proxies for value equality.

A system that conflates identity with equivalence cannot distinguish "the same thing changed" from "a different thing appeared."

### **Desideratum: Observation Kind Separation**

The formalism must distinguish the **kind** of observation recorded in a trace. At minimum, two kinds exist:

| Kind           | Question                                     | Equivalence Relation         |
| -------------- | -------------------------------------------- | ---------------------------- |
| **Membership** | "Is this element present in the collection?" | Set-theoretic (same members) |
| **Content**    | "What is the value at this identity?"        | Structural (same value)      |

These have different invalidation semantics: a Membership change (element added/removed) implies Content invalidation for affected elements, but a Content change (value modified at stable identity) does not imply Membership change.

A system that records only "something was read" without distinguishing the kind of observation cannot validate Membership and Content dependencies independently, forcing unnecessary coupling between them.

### **Desideratum: Absence Without Instantiation**

Observing that an element is **absent** from a collection must not require instantiating a cell for the absent element. Membership observations are properties of collections, not of individual members.

A system that requires per-element cells for absence checks leaks resources proportional to the query space rather than the data space.

## **2. The Algebra of Revisions**

To avoid logical inconsistencies and "Reincarnation Bugs" (identity collisions across server restarts), we adopt a **Strict Equality** model with Epoch Scoping.

### **Definition: Revision Space**

Let $\mathcal{R}$ be the set of all possible revisions.

$$\mathcal{R} = \mathcal{R}_{mem} \cup \mathcal{R}_{disk}$$  
Memory Revisions (Epoch-Scoped):  
To ensure that a revision ID from a previous server process is never valid in a new process, we define $\mathcal{R}_{mem}$ as a tuple:  
$$\mathcal{R}_{mem} = \text{UUID}_{Epoch} \times \mathbb{Z}_{Counter}$$

- $\text{UUID}_{Epoch}$: A unique identifier generated at server boot.
- $\mathbb{Z}_{Counter}$: A monotonic integer.

**Disk Revisions:**

$$\mathcal{R}_{disk} \subseteq \{0,1\}^k \quad \text{(Content Hash)}$$

### **Definition: Validity Operator ($\equiv$)**

We strictly use equality. Partial ordering is undefined across the disjoint union of Memory and Disk revisions.

$$\text{Valid}(r_{cached}, r_{current}) \iff r_{cached} = r_{current}$$  
**Plain English:**

1. **Strict Equality:** We stop pretending hashes have an order. If the ID matches, the data is the same. If it doesn't, it's different.
2. **Epochs:** If the server crashes and restarts, it gets a new "Epoch ID." Even if the counter resets to "1", the revision (Epoch_B, 1) will never match the client's old (Epoch_A, 1). This prevents the client from holding stale data that coincidentally looks valid.

## **3. System State and Trace Semantics**

### **Primitives**

Definition: Cell Identity & Containment  
A Cell Identifier $ID$ is a tuple $\langle Source, Pointer \rangle$.  
Axiom: Containment  
For sub-structural addressing to be sound, the data referenced by $Pointer$ must be a strict structural subset of $Source$.

$$\text{Data}(Pointer) \subseteq \text{Data}(Source)$$  
Property: Adaptive Granularity (Conservative Approximation)  
Calculating the exact fine-grained revision can be expensive. It is mathematically sound to substitute a coarser revision (e.g., the file hash) for the fine revision because $\Delta(Pointer) \implies \Delta(Source)$ (by Containment).  
$$r_{coarse}(ID) = \text{Hash}(v(Source))$$  
Constraint (The Granularity Trap):  
If fine-grained revisions are used, the Extract function must be backed by an index or memoized tree structure such that its cost approximates $O(1)$ or $O(\log N)$. If Extract requires $O(N)$ parsing of the Source, the validation cost will dominate execution time.  
Definition: Pure Computation  
A computation $f$ is **pure** if it satisfies Referential Transparency: given the same input values for its read-set, it produces the same output.

Definition: Read-Set  
The Read-Set of a computation $f$ under state $\Sigma$ is the mapping from cells to values:

$$\text{Reads}(f, \Sigma) = \{ c \mapsto \Sigma(c).v \mid c \in \text{cells accessed by } f \}$$

Property: Referential Transparency  
If two states agree on the read-set, the computation must produce the same result:

$$\left( \forall c \in \text{dom}(\text{Reads}(f, \Sigma)),\; \Sigma(c).v = \Sigma'(c).v \right) \implies f(\Sigma) = f(\Sigma')$$

Note: The memoization system uses **Revisions** as a proxy for value equality. The revision model guarantees that equal revisions imply equal values (by the definition of Write Bumping). The converse is not required — different revisions may correspond to the same value, which causes unnecessary re-execution but never incorrect results.

Definition: Observation Kind  
An Observation Kind $k \in \mathcal{K}$ classifies the nature of a dependency (per the Observation Kind Separation desideratum, §1):

- **Content** ($k_c$): "What is the value at this identity?" — structural equivalence.
- **Membership** ($k_m$): "Is this element present in the collection?" — set-theoretic equivalence.

Definition: Trace  
A Trace $\mathcal{T}$ is a **set** of (cell, observation kind, revision) triples recording the observations made during a computation.

$$\mathcal{T} = \{ (c_1, k_1, r_1), (c_2, k_2, r_2), \dots, (c_n, k_n, r_n) \}$$

Traces are unordered. Two entries with the same cell but different observation kinds are **distinct** dependencies — they track different equivalence relations and are validated independently. If a cell is observed with the same kind multiple times, one entry is retained (the revision is identical within an epoch, so duplicates are redundant).

### **Axiom: Organic Consumption**

Dependency Recording is a side-effect of observation. Each observation records the cell, its observation kind, and the current revision.

$$\text{Read}(Cell) \implies \text{Record}(Cell, \text{Content}, \text{CurrentRevision})$$
$$\text{Observe}(Collection) \implies \text{Record}(Collection, \text{Membership}, \text{CurrentRevision})$$

### **Axiom: Mediated Access (Trace Completeness)**

All data access by a pure computation is mediated through kind-specific read operations. No untracked side-channels exist.

**Content Access**: All value reads are mediated by `Read(Cell)`:

$$\forall c \in \text{dom}(\text{Reads}(f, \Sigma)),\; (c, \text{Content}, \Sigma(c).r) \in \mathcal{T}_f$$

**Membership Access**: All collection membership observations are mediated by `Observe(Collection)`:

$$\forall C \in \text{collections observed by } f,\; (C, \text{Membership}, \Sigma(C).r) \in \mathcal{T}_f$$

Combined with Organic Consumption, this establishes the biconditional: a cell is in the read-set if and only if it appears in the trace with the appropriate observation kind.

### **Property: Existential Dependency (Membership Implies Content)**

A Content observation on cell $c$ within collection $C$ presupposes that $c$ is a member of $C$. If $c$'s membership status changes (it is added to or removed from $C$), any prior Content observation on $c$ is invalidated — the referent either didn't exist before or no longer exists.

$$\text{MembershipChanged}(c, C) \implies \text{ContentInvalid}(c)$$

The converse does not hold: a Content change (modification of value at a stable identity) does not imply a Membership change.

$$\text{ContentChanged}(c) \not\Rightarrow \text{MembershipChanged}(c, C)$$

**Consequence for validation**: When a Membership observation is invalid for collection $C$, all Content observations on members of $C$ whose membership may have changed must be revalidated. When only Content observations are invalid, Membership observations may still be valid — the set of identities is unchanged, only the values behind them changed.

### **Property: One-Sided Error (Monotone Soundness)**

The system's model of equivalence may diverge from ground truth in two ways. Both produce one-sided, bounded errors:

| Divergence                                                 | Direction                                            | Failure Mode             | Consequence                 |
| ---------------------------------------------------------- | ---------------------------------------------------- | ------------------------ | --------------------------- |
| **Under-tracking** (untracked input, too-coarse revision)  | System believes "nothing changed" when something did | Stale data served        | Missed re-execution         |
| **Over-tracking** (too-coarse validator, too-coarse delta) | System believes "something changed" when it didn't   | Unnecessary re-execution | Wasted work, correct result |

Crucially, neither failure mode can produce **corruption**: no phantom writes, no structural divergence, no cascading inconsistency.

**Composition**: Errors are independent across trace entries. If computation $f$ depends on cells $A$ and $B$, and $A$ is under-tracked while $B$ is properly tracked:

- When $B$ changes: the system correctly re-executes $f$, producing $f(A_{current}, B_{new})$. The under-tracking of $A$ is irrelevant.
- When $A$'s untracked input changes: the system does not re-execute. The stale result reflects $A_{old}$, but $B$'s contribution is still valid.
- The failure is **bounded to the axis of the violated axiom**.

This same principle governs over-coarse validators and deltas: a coarse validator may trigger unnecessary re-execution for one trace entry, but does not affect the precision of validation for other entries in the same trace.

**Practical implication**: Computations that read clocks, environment variables, or random sources are _knowingly impure_ and should be marked as Volatile (§8) rather than silently violating Mediated Access. The One-Sided Error property guarantees that such violations are contained — but only if the engineer understands which axis they are weakening.

## **4. The Memoization Theorem**

Let $f$ be a pure computation.  
Let $(v_{old}, \mathcal{T}_{old})$ be the result of $f(\Sigma_{old})$.

### **Property: Soundness of Memoization**

If the trace $\mathcal{T}_{old}$ is valid with respect to a new state $\Sigma_{new}$:

$$\left( \forall (c, k, r) \in \mathcal{T}_{old},\; \Sigma_{new}(c).r_k = r \right) \implies f(\Sigma_{new}) \equiv v_{old}$$

Where $\Sigma(c).r_k$ denotes the revision for cell $c$ under observation kind $k$.

**Proof:**

1. **Mediated Access (Axiom):** All observations by $f$ — both Content reads and Membership observations — pass through kind-specific operations, which record them in the trace with their observation kind (by Organic Consumption). Therefore $\mathcal{T}_{old}$ contains every observation made during $f(\Sigma_{old})$.
2. **Revision-Value Correspondence:** The validity predicate ensures $\Sigma_{new}(c).r_k = r$ for all trace entries. By the revision model, equal revisions under the same observation kind imply equal observations: same values (Content) or same membership (Membership).
3. **Existential Dependency:** Any Content entry $(c, \text{Content}, r_c) \in \mathcal{T}_{old}$ presupposes a Membership entry $(C, \text{Membership}, r_C) \in \mathcal{T}_{old}$ for $c$'s collection $C$. If both are valid, $c$ still exists with the same value.
4. **Referential Transparency:** Since all observations are identical across both kinds, $f(\Sigma_{new}) = f(\Sigma_{old}) = v_{old}$ (by the definition of Pure Computation).
5. **Q.E.D.** Return $v_{old}$.

### **The Cost of Validation**

The cost is linear to the cut (immediate dependencies).

$$\text{Cost}(Validation) = O(|\mathcal{T}|)$$

## **5. Execution Model: Optimistic Concurrency**

We reject the "Stationarity Axiom" as physically impossible in distributed systems without locking. We replace it with an **Optimistic Concurrency Control (OCC)** model.

### **Definition: Transactional Post-Condition**

A computation $f$ produces a result $(v, \mathcal{T})$. This result is valid **if and only if**:

$$\forall (c, r) \in \mathcal{T}, \quad \Sigma_{start}(c).r = \Sigma_{end}(c).r$$

### **Implication: Persistence of Sub-Computations**

While the _top-level_ result of a torn execution is discarded, the side-effects (memoized cache entries) of any **completed sub-computations** are preserved. The system automatically minimizes re-work via the memoization hierarchy.

### **Implication: Extended Consistency (Temporal Closure)**

If a computation $f$ produces a specification for a deferred effect $E$ (e.g., a Resource Init), that effect must satisfy the same consistency predicate as $f$.

$$\text{TargetState}(E) \equiv \Sigma_{f}$$  
**Implication:** Resource Constructors are **Closures over Time**. They must capture the SnapshotID of the state used to generate them. When the Resource initializes (Phase C), it must use this captured ID to ensure it interacts with the world as it existed during Render (Phase B).

### **Axiom: Well-Foundedness (No Cycles)**

The dependency graph implies a strict partial order. No computation may depend, directly or transitively, on its own output during the same Revision Epoch.

### **Axiom: Collision Resistance**

We assume a cryptographic hash function $H$ where the probability of $H(a) = H(b)$ given $a \neq b$ is negligible.

## **6. Handling Edge Cases**

### **Dynamic Dependencies (Trace Divergence)**

For branching logic if (A) read(B) else read(C):

- If $A$ is unchanged, the trace holds.
- If $A$ changes, the trace is invalid. Re-execution generates a new trace $\mathcal{T}_{new}$.

### **Phantom Reads (Collections)**

We introduce **Collection Cells** where $v(D)$ is the membership set (e.g., the set of filenames in a directory). Iterating a collection produces a **Membership** observation on its Collection Cell (per §1, Observation Kind Separation), ensuring invalidation if elements are added or removed. Content observations on individual members are recorded separately — they track value equivalence at a stable identity, independent of the collection's membership.

Per the Existential Dependency property (§3), if a collection's membership changes such that an element is removed, any Content observation on that element is also invalidated.

Per the Absence Without Instantiation desideratum (§1), querying whether an element exists in a collection records only the Membership observation on the collection cell — no individual cell is created for the absent element.

## **7. The Distributed Cut (Snapshot Pinning)**

To solve race conditions (Tearing) and Liveness failures (Zeno's Paradox), we use **Pinned Fetches**.

### **Definition: Snapshot Window ($W$)**

The Server maintains a set $W$ of recent Root Digests that remain valid for retrieval, even if they are no longer the latest state.

$$W = \{ D_t \mid t > \text{Now} - \delta_{retention} \}$$  
Constraint: The retention window $\delta$ must account for both Network Latency ($RTT$) and Resource Initialization Time ($T_{init}$).

$$\delta_{retention} > \text{Max}(RTT, T_{init})$$

### **Definition: Pinned Fetch**

The client requests "File A consistent with Digest $D$".

$$\text{Fetch}(Root, D_{req}) = \begin{cases} (v, \mathcal{T}) & \text{if } D_{req} \in W \quad (\text{Safe}) \\ \bot & \text{if } D_{req} \notin W \quad (\text{Stale}) \end{cases}$$

### **Guarantee: Safety + Liveness**

- **Safety:** Tearing is impossible because $D_{req}$ enforces internal consistency of the frame.
- **Liveness:** A slow client (or a slow Resource) can successfully fetch a frame $D_{t-1}$ even if the server has advanced to $D_t$, provided $D_{t-1}$ is still in $W$.

**Plain English:** The Digest acts like a ticket. If the "show" (state) changes while you are in line, your ticket is still valid for a short grace period (the Snapshot Window). This prevents you from getting stuck in an infinite loop of "Stale" errors if your internet is slow or your setup takes a moment.

## **8. The Impurity Bound (Generative Identity)**

To resolve the "Cache Eviction Paradox" (Split-Brain) where re-computing an impure function creates identity collisions, impure functions must use **Generative Identity**.

### **Definition: Volatile Computation (Impure)**

If a computation $f$ reads untracked inputs $U$ (Impure), its revision ID must be generative.

$$r(f) = \text{Hash}(\text{Trace} + \text{Nonce})$$

Where $\text{Nonce}$ is a unique random value generated each time $f$ executes.

### **Guarantee: Session Safety**

1. **Execution 1:** $f$ runs, uses $Nonce_1$. Generates $r_1$. Returns $v_1$.
2. **Eviction:** Cache is wiped. $Nonce_1$ is lost.
3. **Execution 2:** $f$ runs (same inputs), uses $Nonce_2$. Generates $r_2$. Returns $v_2$.
4. **Collision Avoidance:** Even if $Inputs$ are identical, $r_1 \neq r_2$.
5. **Result:** A client holding $r_1$ will correctly fail validation against a server holding $r_2$.

### **Rule: Eviction Priority (The Swiss Cheese Rule)**

Because the Nonce is irrecoverable, Impure computations are more valuable than Pure computations.  
If the system must evict cache entries due to memory pressure, it MUST prioritize retaining Impure nodes over Pure nodes.

- **Loss of Pure Node:** Can be re-derived. $r_{new} \equiv r_{old}$. Snapshot integrity maintained.
- **Loss of Impure Node:** Cannot be re-derived. $r_{new} \neq r_{old}$. Snapshot becomes partially corrupt ("Swiss Cheese"), forcing total invalidation for any client pinned to that snapshot.

**Plain English:** If a function uses randomness, we stamp the result with a unique serial number. If the server forgets the result (cache eviction) and has to re-calculate it (potentially getting a different random number), it stamps it with a _new_ serial number. Therefore, the server should try really hard not to forget these random numbers, because losing one breaks the "ticket" (Snapshot) for everyone currently using it.

## **9. Transient Computations (Trace Flattening)**

To support "Identity-Free" computations (e.g., anonymous closures, iterators, or transient query nodes) without violating the Iron Rule, we introduce **Trace Flattening**.

### **Definition: Transient Node**

A Transient Node $T$ is a computation that:

1.  Has no persistent Identity (no Cell ID).
2.  Has no persistent Storage (no Cache).
3.  Is executed strictly within the context of a Parent Computation $P$.

### **Property: Trace Flattening**

Let $P$ be a parent computation with trace $\mathcal{T}_P$.
Let $T$ be a transient computation called by $P$.
Let $\mathcal{T}_T$ be the trace of $T$.

If $T$ is transient, then:
$$\mathcal{T}_P' = \mathcal{T}_P \cup \mathcal{T}_T$$

Since traces are sets, this is standard set union. Duplicate cells are collapsed.

**Proof:**

1.  $P$ executes. $\text{ActiveTrace} = \mathcal{T}_P$.
2.  $P$ calls $T$.
3.  $T$ executes. It reads cell $C$.
4.  Since $T$ has no identity, it does not switch the `ActiveTrace`.
5.  The read of $C$ is recorded directly into $\mathcal{T}_P$.
6.  $T$ returns value $v$.
7.  $P$ continues.

**Result:**
The dependency graph is "flattened". $P$ depends directly on $C$, as if $P$ had read $C$ itself. The intermediate structure of $T$ is erased from the graph.

### **Implication: The Inlining Equivalence**

A Transient Node is isomorphic to an inlined function.
$$\text{Trace}(P \to T \to C) \equiv \text{Trace}(P \to C)$$
This allows us to build complex, modular query logic (e.g., `filter`, `map`, `flatMap`) without incurring the memory overhead of creating a persistent node for every intermediate step. The reactivity system "sees through" the abstraction.

## **10. The Validator Algebra (Granular Invalidation)**

To support precise invalidation without the overhead of Dynamic Dependency Graphs (DDGs), we introduce the **Validator Algebra**.

### **Axiom: The Rejection of DDGs**

We explicitly reject Dynamic Dependency Graphs (where dependencies are closures or pointers to live nodes) because they violate the **Iron Rule** (Metadata vs. Data) and incur unacceptable memory overhead ($O(N)$ pointers per node).

Instead, we define a Dependency as a **Static Projection** of the state.

### **Definition: Delta (Change Descriptor)**

A Delta $\delta$ describes _what changed_ in a cell between two consecutive revisions. The representation is cell-type-specific:

| Granularity       | Delta Type                                  | Example                                      |
| ----------------- | ------------------------------------------- | -------------------------------------------- |
| None (default)    | $\delta = \top$ (opaque change)             | Revision differs → assume everything changed |
| Column-level      | $\delta \subseteq \text{Columns}$ (bitmask) | `{status, updated_at}`                       |
| Content-addressed | $\delta = (h_{old}, h_{new})$ (digest pair) | Content hash comparison                      |

When $\delta = \top$, the Validator intersection check is trivially true, and the system reduces to pure revision comparison. This is the default — granular deltas are an optimization, not a requirement.

**Axiom: Delta Consistency**  
If $r_{old} = r_{new}$, then $\delta$ is empty. If $r_{old} \neq r_{new}$, then $\delta$ is a sound (possibly conservative) description of the change. Deltas may over-report (include unchanged fields) but must never under-report (omit changed fields).

**Constraint: Metadata Bound**  
$\delta$ must be fixed-size metadata, not a diff of the actual data. This preserves the Iron Rule — validation never touches values.

### **Definition: The Validator Interface**

A Dependency is a tuple $\langle \text{CellID}, \text{ObservationKind}, \text{Revision}, \text{Validator} \rangle$.

The `ObservationKind` ensures that Membership and Content dependencies on the same cell are validated independently with kind-appropriate validators.

The `Validator` is an opaque, serializable token that implements the **Intersection Principle**.

```rust
trait Validator {
    type Delta;
    /// Returns true if the change (Delta) overlaps with the observed projection.
    fn intersects(&self, delta: &Self::Delta) -> bool;
}
```

### **The Intersection Rule**

A dependency is valid if:

1.  **Identity Check**: The Cell's Revision ID matches the recorded ID. (Fast Path).
2.  **Intersection Check**: If the ID changed, the `Validator` does _not_ intersect the Cell's `Delta`. (Granular Path).

$$\text{Valid}(D, C) \iff (C.r = D.r) \lor \neg D.validator.intersects(C.delta)$$

### **Result: The O(1) Bound**

Because `Validator` and `Delta` are fixed-size metadata (e.g., Bitmasks, Bloom Filters, Field IDs), the validation cost remains $O(1)$ per dependency, preserving the performance characteristics of the system while enabling fine-grained reactivity.

## **11. The Lattice of Validators (Algebraic Composition)**

To enable efficient composition of dependencies, Validators must form a **Bounded Distributive Lattice**.

### **Definition: Validators as Delta Predicates**

A Validator $V$ is a predicate on deltas: $V: \Delta_C \to \{0, 1\}$. It answers "does this change affect me?"

Equivalently, $V$ corresponds to a subset of the delta space: $V \subseteq \Delta_C$ (the set of deltas that invalidate this dependency).

### **Definition: The Lattice Structure**

The set of validators $\mathcal{V} = \mathcal{P}(\Delta_C)$ forms a **powerset lattice** under set inclusion:

$$\langle \mathcal{V}, \subseteq, \cup, \cap, \emptyset, \Delta_C \rangle$$

- **Order ($\leq$)**: $V_1 \leq V_2 \iff V_1 \subseteq V_2$ (more sensitive = higher in the lattice).
- **Bottom ($\bot = \emptyset$)**: `Never` — invalidates on nothing. Represents a constant or dead dependency.
- **Top ($\top = \Delta_C$)**: `Always` — invalidates on any change. Represents a full read.
- **Join ($\lor = \cup$)**: $V_A \lor V_B$ invalidates if $A$ invalidates **OR** $B$ invalidates.
- **Meet ($\land = \cap$)**: $V_A \land V_B$ invalidates if $A$ invalidates **AND** $B$ invalidates.

**Why distributive?** Powerset lattices are always distributive: $A \cup (B \cap C) = (A \cup B) \cap (A \cup C)$.

### **Property: Dependency Compression**

If a computation observes the same cell with the same observation kind multiple times using different validators $V_1, V_2, \dots, V_n$, the system can store a single dependency with the **Join** of the validators. Observations of different kinds (e.g., Membership vs. Content) are distinct dependencies and must not be compressed together.
$$D_{total} = \bigvee_{i=1}^n V_i \quad \text{(same cell, same kind)}$$

**Optimization Rules:**

1.  $V \lor \top = \top$ (If you read everything, specific queries are redundant).
2.  $V \lor \bot = V$ (Dead dependencies vanish).
3.  $V \lor V = V$ (Idempotence).

### **Property: Approximation Soundness**

The full powerset $\mathcal{P}(\Delta_C)$ is too large to materialize. Practical validators are **conservative approximations** — e.g., bitmasks over a known column set, or bloom filters — that may over-report intersections but never under-report.

Let $V_{exact} \subseteq \Delta_C$ be the ideal validator (the set of deltas that _actually_ affect the computation).
Let $V_{approx} \supseteq V_{exact}$ be the implemented validator.

The **false positive rate** is:
$$\text{FPR}(V_{approx}) = \frac{|V_{approx} \setminus V_{exact}|}{|V_{approx}|}$$

| FPR                  | Meaning           | Example                                                           |
| -------------------- | ----------------- | ----------------------------------------------------------------- |
| $0$                  | Exact             | Bitmask over a known, fixed column set                            |
| $0 < \text{FPR} < 1$ | Lossy but bounded | Bloom filter (FPR is a function of filter size and hash count)    |
| $1$                  | Degenerate        | $V = \top$: always invalidates, equivalent to no validator at all |

**Invariant**: $V_{exact} \subseteq V_{approx}$ guarantees no false negatives. The system may re-execute when it didn't need to (wasted work), but never skips re-execution when it should have (correctness).

The lattice laws hold on approximations because union of supersets is still a superset: if $V_1 \supseteq V_{1,exact}$ and $V_2 \supseteq V_{2,exact}$, then $V_1 \cup V_2 \supseteq V_{1,exact} \cup V_{2,exact}$.

**Cost model**: The cost of unnecessary re-execution is bounded by $\text{FPR} \times \text{Cost}(f)$ per validation cycle. If this exceeds the cost of maintaining a more precise validator, upgrade the approximation.

This is the Validator-level instance of the One-Sided Error property (§3): over-coarse validators cause wasted work, never stale data.

### **Axiom: The Thunk Barrier (Pure Headers)**

To enforce "Zero-Execution Validation," we mandate a physical separation of Metadata and Data.

**The Split Entity Rule:**
Every Reactive Node consists of two distinct memory regions:

1.  **The Header (Hot)**: Contains `RevisionID`, `Delta`, and `ContentHash`. Must be resident in RAM.
2.  **The Body (Cold)**: Contains the actual data. May be a Lazy Thunk or swapped to disk.

**The Validation Constraint:**
The `intersects` function MUST be pure and MUST operate exclusively on the `Header`. It is physically impossible for validation to trigger I/O or body deserialization.

## **12. The Algebra of Lifecycle (Cleanup)**

To ensure the system does not leak memory over time, we formalize the **Lifecycle of Reactive Nodes**.

### **Definition: The Liveness Predicate**

A Node $N$ is **Live** if and only if:

1.  It is a **Root** (e.g., an active UI component or a persistent file watcher).
2.  It is **Reachable** from a Live Node via the Dependency Graph.

$$\text{Live}(N) \iff \text{IsRoot}(N) \lor \exists P \in \text{Parents}(N) : \text{Live}(P)$$

### **Rule: The Drop Cascade**

When a Node transitions from Live to Dead (Reference Count drops to zero), it MUST:

1.  **Unsubscribe** from all its dependencies (decrementing their refcounts).
2.  **Drop** its Body (releasing memory).
3.  **Drop** its Header (if no WeakRefs remain).

### **Constraint: Cycle Freedom**

Because we use Reference Counting (Arc/Rc), **Cycles are Forbidden**.
The "Well-Foundedness" Axiom (Section 4) guarantees that the Dependency Graph is a DAG.
However, **Inverse Dependencies** (Listeners) must be stored as **Weak References** to prevent cycles between Parents and Children.

### **Verification: The Leak Test**

A "Leak" is defined as a Node that remains in memory despite satisfying $\neg \text{Live}(N)$.
The implementation MUST provide a `verify_leaks()` utility that asserts:

1.  The Global Node Count is zero after a full `reset()`.
2.  Specific "Ghost Nodes" (from transient forks) are dropped after the fork is discarded.
