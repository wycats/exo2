# **Algebra of the Reactive File System**

RFC 0119 Working Group
Version: 1.0

## **0. Introduction**

This document formalizes the **Reactive File System (RFS)** as a **Merkle-DAG** (Directed Acyclic Graph) of **Content-Addressed Cells**.

It proves that "File Watching", "Globbing", and "Build Caching" are not separate features, but **Equivalent Projections** of the core **Revision Algebra**.

## **1. The Algebra of Disk Cells**

We model the File System not as a mutable place, but as a **Function over Time** returning **Immutable Snapshots**.

### **Definition: The Disk Cell**

Let $F$ be a file at path $P$. We represent $F$ as a Cell.

$$C_F = \langle \text{ID}(P), \text{Value}(Bytes), \text{Revision}(H) \rangle$$

- **Identity**: The Absolute Path $P$.
- **Value**: The raw byte content.
- **Revision**: The Cryptographic Hash of the content ($H = \text{SHA256}(Bytes)$).

### **Property: Idempotent Mutation**

Unlike memory cells (where `set(v)` increments a counter), Disk Cells are **Content-Addressed**.

$$
\text{set}(C_F, v_{new}) \implies \begin{cases}
\text{No-Op} & \text{if } H(v_{new}) = H(v_{old}) \\
\text{Update} & \text{if } H(v_{new}) \neq H(v_{old})
\end{cases}
$$

**Implication**: The "Ingestion Loop" (Watcher) can be noisy. It can report the same file changing 100 times. If the hash settles to the same value, the Engine propagates **Zero Invalidations**.

## **2. The Algebra of Directories (The Merkle Tree)**

We model a Directory not as a container, but as a **Derived Signal** of its children.

### **Definition: The Directory Cell**

Let $D$ be a directory containing children $\{c_1, c_2, \dots\}$.

$$v(D) = \text{SortedMap} \{ \text{Name}(c_i) \to \text{Type}(c_i) \times \text{Hash}(c_i) \}$$

The Revision of the Directory is the **Merkle Hash** of this map.

$$H(D) = \text{Hash}(\sum \text{Hash}(Entry_i))$$

### **Property: Deep Stability (The "Deep Watch")**

Because $H(D)$ depends on $H(c_i)$, a change to a leaf file $F$ propagates hashes up to the Root.

$$\Delta(F) \implies \Delta(\text{Parent}(F)) \implies \dots \implies \Delta(\text{Root})$$

**The "Deep Watch" Optimization**:
To check if _anything_ in the project changed:

1.  Client holds $H_{old}(\text{Root})$.
2.  Client compares with $H_{new}(\text{Root})$.
3.  If equal, **The entire project is unchanged**. Validation cost is $O(1)$.

## **3. The Algebra of Observation (Globbing)**

We model "Globbing" (searching for files) as a **Recursive Query** over the Merkle Tree.

### **Definition: The Glob Function**

$$G(\text{Pattern}, \text{Dir}) \to \text{Set}\langle \text{Path} \rangle$$

Execution Trace:

1.  Read $D$. Record dependency on $C_D$.
2.  Filter entries matching Pattern.
3.  Recurse into matching sub-directories.

### **Property: Precise Invalidation**

Because $G$ depends on the Directory Cells $C_D$ encountered:

- **Adding a file**: Changes $H(D)$. Invalidates $G$.
- **Modifying a file**: Changes $H(F)$, which changes $H(D)$. Invalidates $G$.
- **Touching a file (same content)**: $H(F)$ is unchanged. $H(D)$ is unchanged. $G$ is **Stable**.

## **4. The Algebra of Corruption (Race Conditions)**

The File System is **External Mutable State**. We cannot lock it. Therefore, we must model **Inconsistency**.

### **Definition: The Corrupted State ($\bot$)**

Let $S$ be a Snapshot where $H(P) = h$.
A **TOCTOU (Time-of-Check to Time-of-Use)** race occurs if:

1.  Engine records $P \to h$.
2.  External Process deletes $P$.
3.  Client calls `fetch(P, h)`.

In the Algebra of Availability (RFC 0037), this is a distinct state:

$$\text{State}(P) = \text{Absent}(\text{Reason::Corrupted})$$

### **Propagation Rule: The Panic Button**

If a computation encounters $\bot$:

1.  It cannot proceed (cannot read file).
2.  It returns $\bot$.
3.  The Engine catches $\bot$.
4.  **Action**: The Engine declares the **Entire Snapshot Invalid**. It forces a full re-scan of the affected path to re-establish consistency.

## **5. The Algebra of Symlinks (Cycles)**

Symlinks break the Tree structure, introducing Graphs and Cycles.

### **Definition: The Link Cell**

A Symlink is a File Cell with a special type.
$$v(L) = \text{Path}(Target)$$

### **Resolution Rule**

To resolve a path `a/b/link/c`:

1.  Read `a/b`.
2.  Read `link`. (Dependency on Link).
3.  Resolve Target $T$.
4.  Read $T$. (Dependency on Target).

### **Cycle Detection**

The RFS must implement **Cycle Detection** during recursion (e.g., `glob`).

- **State**: `VisitedSet<Path>`.
- **Rule**: If $P \in Visited$, stop recursion. Return `CycleDetected`.

## **6. Integration with Build Systems**

The RFS Algebra provides the mathematical foundation for a **Reactive Build System**.

### **Definition: The Artifact**

A Build Artifact $A$ is a Computed Signal derived from Source Files.

$$A = f(S_1, S_2, \dots)$$

### **Property: Perfect Caching**

Because $S_i$ are Content-Addressed:

1.  If user changes $S_1$ then reverts it: $H(S_1)$ reverts.
2.  The Engine sees inputs match a previous trace.
3.  The Engine returns the **Cached Artifact** $A_{old}$ instantly.
4.  **No Rebuild Required**.

This is equivalent to `ccache` or Bazel, but implemented at the **Granularity of the Cell**, not the Process.
