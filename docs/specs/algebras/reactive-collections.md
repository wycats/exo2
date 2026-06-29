# **Algebra of Reactive Collections**

RFC 0118 Working Group
Version: 1.0

## **0. Introduction**

This document formalizes the algebraic layering of **Reactive Collections** (Surgical Context) on top of the core **Validation-Based Reactivity** (Trace/Revision).

It proves that high-level constructs like "Stable Chunk Trees", "Transient Signals", and "Query Scopes" are not ad-hoc extensions, but **Equivalent Projections** of the core primitives.

## **1. The Algebra of Collections**

We model a Collection not as a monolithic value, but as a **Recursive Structure of Cells**.

### **Definition: The Stable Sequence**

Let $S$ be a sequence of items. We represent $S$ as a tree of cells, strictly adhering to the **Split Entity Rule** (Thunk Barrier).
$$S = \text{Node}(\text{Header}, \text{Body})$$

- **Header (Hot)**: Stores the **Topology** (list of children IDs) and **Summary** (Associative). This corresponds to `Cell_meta`.
- **Body (Cold)**: The list of children, stored as **Lazy Thunks** (Hashes).

### **Property: Structural Stability**

Mutation is defined as a **Local Topology Change**.
To insert item $x$ at index $i$:

1.  Locate Leaf $L$ containing $i$.
2.  Split $L$ into $L_1, L_2$. (New Identities).
3.  Update Parent $P$.
    $$v(P)_{new} = [ \dots, L_1, L_2, \dots ]$$
4.  **Invalidation Scope**: Only $P$ and the path to the Root are invalidated. Siblings of $L$ are untouched.
    $$\text{Invalidated} = \text{Path}(L \to \text{Root})$$

### **Property: Summary Aggregation (The Query)**

Let $M$ be an **Associative Summary** type (e.g., `Count`, `BloomFilter`).
The value of a Node is the combination of its children's summaries.
$$v(P).summary = \bigoplus_{c \in children} v(c).summary$$

**Query Efficiency**:
A query $Q$ traverses the tree. It consumes $v(P).summary$.

- If $Q$ prunes the branch (based on summary), it records a dependency on $P$.
- If $Q$ descends, it records a dependency on $Child$.
  This ensures that $Q$ is only invalidated if the _relevant_ parts of the tree change.

## **2. The Algebra of Intents (Transactions)**

We model a Surgical Intent as a **State Transition Function** over Revisions.

### **Definition: The Intent Function**

$$I: \Sigma \to \Sigma'$$
Where $\Sigma$ is the set of all Cell Revisions.

### **Mechanism: Optimistic Replay**

Let $I$ be an intent.

1.  **Read Phase**: $I$ executes on $\Sigma_{start}$. It produces a Read Set $\mathcal{R}$ and a Write Set $\mathcal{W}$.
2.  **Commit Phase**: The system attempts to apply $\mathcal{W}$.
3.  **Validation**:
    $$\text{Valid}(\mathcal{W}) \iff \forall (c, r) \in \mathcal{R}, \text{Current}(c) = r$$
4.  **Replay**: If Invalid, we discard $\mathcal{W}$ and re-execute $I$ on $\Sigma_{current}$.

This proves that **"Ghost Mode"** and **"Rebase"** are simply the application of $I$ to different states ($\Sigma_{fork}$ vs $\Sigma_{live}$).

## **3. The Algebra of Queries (Scopes)**

We model a Query Scope as a **Reader Monad** over the Trace.

### **Definition: The Scope**

A Scope $S$ is a context that intercepts reads.
$$S(\text{read}(c)) \to \text{Trace.record}(c); \text{return } v(c)$$

### **Property: Late Binding (Symlinks)**

A "Symlink" is a function $L: \text{Path} \to \text{Cell}$.
A Query $Q$ executes $L(\text{path})$.

1.  $Q$ reads the Directory Cell $D$ to resolve the path.
2.  $Q$ reads the Target Cell $T$.
    $$\text{Trace}(Q) = \{ D, T \}$$
    If the file moves ($D$ changes), $Q$ invalidates.
    If the content changes ($T$ changes), $Q$ invalidates.
    This proves that **Graph Traversal** is just a composition of **Cell Reads**.

## **4. The Algebra of Associative Collections (Maps & Sets)**

We extend the tree model to Hash Array Mapped Tries (HAMT) to support efficient `has(key)` and `get(key)` operations.

### **Definition: The Path Dependency**

Let $K$ be a key with hash $H(K)$.
The operation `has(K)` traverses the trie following the bits of $H(K)$.
It stops at a Node $N$ when:

1.  **Found**: $N$ contains $K$.
2.  **Absent**: $N$ does not contain $K$ and has no child for the next bit chunk.

### **Guarantee: Precise Invalidation (The "Keyhole" Principle)**

The dependency set of `has(K)` is the **Path of Nodes** visited.
$$\text{Trace}(\text{has}(K)) = \{ N_{root}, N_{1}, \dots, N_{stop} \}$$

**Implication:**

- **Stability**: Inserting a key $J$ only invalidates `has(K)` if $H(J)$ and $H(K)$ share a prefix that includes $N_{stop}$.
- **Absence Stability**: If `has(K)` is false, it remains valid until a key is inserted _at the exact location_ where $K$ would have been.

### **Corollary: The Bloom Optimization**

If nodes carry a Bloom Filter summary $B$:

1.  `has(K)` checks $B(N)$.
2.  If $K \notin B(N)$, the traversal stops early.
3.  Dependency is recorded on $N$ (the summary), not the children.
    This allows `has(K)` to return `false` with $O(1)$ dependency cost in sparse regions, ignoring deep structure changes in sub-trees that definitely don't contain $K$.

## **5. The Algebra of Granular Invalidation**

To achieve **Precise Invalidation**, we implement the **Validator Algebra** defined in the Core Reactivity Spec.

### **Definition: The Validator Projection**

We replace the generic "Dependency" with the typed **Validator**.

$$\text{Dep} = \langle \text{Cell}, \text{Revision}, \text{Validator} \rangle$$

- **Validator**: An opaque, serializable token implementing the `Validator` trait (Lattice).
- **Revision**: The version of the cell at the time of reading.

### **Definition: The Mutation Delta**

When a Cell transitions from $r_{old}$ to $r_{new}$, it produces a **Delta** $\Delta$ representing the _subset_ of data modified.

### **The Intersection Rule**

A dependency is valid if the Cell is unchanged, **OR** if the mutation is disjoint from the validator.

$$\text{Valid}(\text{Dep}, \text{Cell}_{now}) \iff (\text{Cell}_{now}.r = \text{Dep}.r) \lor \neg \text{Dep.Validator.intersects}(\text{Cell}_{now}.\Delta)$$

### **Application to Reactive Constructs**

#### **1. Atomic Cells**

- **Operation**: `read()`
- **Tag**: $\top$ (All).
- **Mutation**: `write(v)` $\to \Delta = \top$.
- **Intersection**: $\top \cap \top \neq \emptyset$.
- **Result**: Any write invalidates any read. (Standard Equivalence).

#### **2. Computations (Derived Signals)**

- **Operation**: `compute()`
- **Tag**: $\text{Digest}(\text{Dependencies})$.
- **Mutation**: Re-execution produces a new value.
- **Intersection**: If $v_{new} \neq v_{old}$, then $\Delta = \top$.
- **Result**: Invalidates downstream only if the _output value_ changes (Cut-off).

#### **3. Reactive Collections (HAMT)**

- **Operation**: `get(K)` / `has(K)`
  - **Tag**: $\text{Path}(H(K))$ (The sequence of slots visited).
- **Operation**: `set(K, V)`
  - **Mutation**: Updates the path for $K$.
  - **Delta**: $\text{Path}(H(K))$.
- **Intersection**:
  - If $H(K_1)$ and $H(K_2)$ diverge at Node $N$, their paths are disjoint beyond $N$.
  - The dependency on $N$ uses a **Slot Mask** validator.
  - $\text{Slot}(K_1) \cap \text{Slot}(K_2) = \emptyset$.
- **Result**: `set(K1)` does **not** invalidate `get(K2)` unless they have a true Hash Collision in the same slot.

## **6. The Algebra of Persistence (Serialization)**

To ensure the system survives restarts without re-indexing, we define the **Equivalence of Persistence**.

### **Definition: The Canonical Encoding**

Every Node $N$ has a deterministic binary representation $B(N)$.
$$B(N) = \text{Serialize}(\text{Metadata} + \text{ChildrenIDs})$$

### **The Merkle Equivalence**

The Identity of a Node is the Cryptographic Hash of its encoding.
$$\text{ID}(N) = H(B(N))$$

**Implication:**

- **Location Independence**: A Node is the same whether it lives in RAM (`Arc<Node>`) or on Disk (Blob).
- **Deduplication**: Identical sub-trees (e.g., common code blocks) are physically shared on disk.

### **Definition: The Backing Store (CAS)**

The persistence layer is a **Content-Addressable Store** (CAS).
$$\text{Store}: \text{Hash} \to \text{Bytes}$$

### **Feature: Lazy Rehydration**

A Collection is never "loaded". It is **mounted**.

1.  The system loads the **Root Hash** from a checkpoint.
2.  The Root Node is deserialized.
3.  Children are represented as **Lazy Thunks** (Hashes).
4.  A child is only deserialized when a Query **consumes** it.

This guarantees that the startup time of the system is $O(1)$ with respect to the size of the index.

### **Corollary: Ephemeral Consumers (The UI)**

While the Data Graph is persistent, Ephemeral Consumers (like UI Renderers) typically lose their state on restart.
However, because the Data Graph is **Structurally Identical** (Same Hashes), any **Persisted Computations** (e.g., Analysis Results stored in the CAS) remain valid.
The UI re-renders, but it hits "Warm Cache" for all expensive derived data.
