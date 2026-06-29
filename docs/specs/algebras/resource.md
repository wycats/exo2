# Formalization of Resource Lifecycle & Scopes

**Extension to RFC 0037**
**Version:** 1.3 (Full Restoration)

## 1. The Three-Phase Execution Model

To accommodate resources (Side Effects) without violating the Purity of the Render Phase (Core Algebra Sec 4), we formally expand the execution model to three phases.

### Definition: Phase Separation

**Phase A: Action (Mutation)**

- User inputs, IO events. Writes to Source Cells.
- **Output:** A Dirty signal.

**Phase B: Render (Derivation)**

- Functional calculation of the UI Tree and Resource Specs.
- **Constraint:** No side effects. No reading of volatile external state.
- **Output:** A Tree containing Nodes and PendingResources.

**Phase C: Commit (Reconciliation)**

- Comparison of the new Tree vs. the old Tree.
- **Logic:**
  - **Mount:** Execute `Init()` for new resources.
  - **Unmount:** Execute `Dispose()` for removed resources.
  - **Update:** If a Resource's Dependencies changed (Data Invalidation), execute `Dispose()` then `Init()`.

## 2. The Algebra of Scopes

Resources are structurally bound to the UI topology via Scopes.

### Definition: Scope ($\mathcal{S}$)

A Scope is a node in the Resource Construction Stack. It represents a lifecycle boundary (e.g., a Component, a conditional block).

$$
\mathcal{S} = \langle ID, Parent_{\mathcal{S}}, \{Resource_1, Resource_2, \dots\} \rangle
$$

### Definition: Resource Specification ($\rho$)

A descriptor produced during the Render Phase.

$$
\rho = \text{Constructor}(\text{Args})
$$

The specification must be **Stable** (identity or structural equality) if Args have not changed.

### Definition: Resource Instance ($R$)

The live object managed by the runtime.

$$
R = \langle \rho, \text{TeardownFn} \rangle
$$

## 3. The Lifecycle Invariants

### Invariant I: Structural Bounding

A resource $R$ attached to Scope $\mathcal{S}$ is live if and only if $\mathcal{S}$ is live.

$$
\text{Live}(R) \implies \text{Live}(\mathcal{S})
$$

**Implication: Automatic Disposal**
(This implements the **Drop Cascade** rule from Core Algebra Sec 11).

$$
\text{Dispose}(\mathcal{S}) \implies \forall r \in \mathcal{S}.resources, \text{Dispose}(r)
$$

### Invariant II: Reactive Bounding (The "Teardown" Trigger)

A resource specification $\rho$ depends on a set of reactive inputs $\mathcal{T}_{\rho}$ (its Trace).

If $\text{IsValid}(\mathcal{T}_{\rho}, \Sigma_{new})$ is **False**:

1.  The Render Phase produces a new specification $\rho'$.
2.  The Commit Phase detects $\rho \neq \rho'$.
3.  **Transition:**

$$
\text{Dispose}(R_{old}) \to R_{new} = \text{Init}(\rho')
$$

### Invariant III: Temporal Closure (Snapshot Pinning)

To prevent "Time Travel" (Tearing) between the Render Phase and the Init Phase:

1.  The Constructor must capture the current Snapshot ID ($D_{frame}$) from the context.
2.  The Init function must use this captured ID for any external fetches.

$$
\text{Init}(\rho, D_{frame}) \xrightarrow{\text{Fetch}} \text{Server}(D_{frame})
$$

## 4. The Facade Pattern

The user describes a pattern where the Resource Constructor returns a "Facade" (a bucket of functions/signals).

### Definition: Stable Facade

While the internal side effect (e.g., a socket connection) may be torn down and recreated, the external facade (the object returned to the component) should ideally remain stable or be handled via reactive primitives.

**Pattern A: The Reactive Handle**
The Resource exposes a Cell representing its state.

- **Init:** Connects to external source, writes to Cell.
- **Dispose:** Disconnects.
- **Component:** Reads Cell.
- **Result:** The Component sees the Cell update. It does not necessarily know the Resource re-initialized.

**Pattern B: The Stable Container**
The Constructor returns a proxy or a container object.

$$
\text{Constructor}() \to \text{Proxy}
$$

The Init logic swaps the implementation backing the Proxy.

## 5. Composition with Core Algebra

This extension aligns with the Core Algebra by treating Resource Construction as a specialized Computation.

- **Trace:** The Resource Constructor generates a Trace $\mathcal{T}$ during Render.
- **Validation:** The Core System validates $\mathcal{T}$ exactly like any other computation.
- **Difference:** The "Value" of this computation is a Side Effect Plan, not a DOM Node.

## 6. The Availability Algebra (Async Propagation)

To avoid hardcoding specific async states (like "Loading"), we model async resources using the **Availability** primitive ($\mathbb{A}$). This is algebraically equivalent to an Applicative Functor that propagates failure (Absence).

### Definition: The Availability Type

Let $\mathbb{A}\langle T \rangle$ be the set of possible states for a value of type $T$.

$$
\mathbb{A}\langle T \rangle = \{ \text{Present}(v) \mid v \in T \} \cup \{ \text{Absent}(\epsilon) \mid \epsilon \in \text{Reasons} \}
$$

### Constraint: Identity Stability (The NaN Trap)

Unlike IEEE 754 NaN (where $NaN \neq NaN$), Absent values in a reactive system **MUST** satisfy structural equality.

$$
\text{Absent}(\epsilon_1) \equiv \text{Absent}(\epsilon_2) \iff \epsilon_1 = \epsilon_2
$$

**Reasoning:**
If Absent values are treated as distinct on every render (reference inequality), they will trigger false positives in the Memoization check (Core Algebra), leading to infinite invalidation loops.

### Definition: Lifting (Function Application)

We define the operation **Apply** that lifts a standard function $f: T \to U$ into the Availability domain $f^*: \mathbb{A}\langle T \rangle \to \mathbb{A}\langle U \rangle$.

$$
f^*(\mathbb{A}\langle x \rangle) = \begin{cases}
\text{Present}(f(x)) & \text{if } \mathbb{A}\langle x \rangle = \text{Present}(x) \\
\text{Absent}(\epsilon + \text{Context}) & \text{if } \mathbb{A}\langle x \rangle = \text{Absent}(\epsilon)
\end{cases}
$$

**Note on Provenance:**
When propagating Absent, the system SHOULD append the dependency path to $\epsilon$ to prevent "Context Swallowing" (where a root failure obscures the leaf cause).

### Rule: Automatic Propagation (Short-Circuiting)

For a multi-argument function $f(x, y)$, if **any** input is Absent, the function is not evaluated, and the result is Absent.

$$
f^*(\text{Present}(x), \text{Absent}(\epsilon)) \equiv \text{Absent}(\epsilon)
$$

**Implication:**
This provides the mathematical guarantee that a "Pending" or "Error" state bubbles up the dependency graph automatically. A computation cannot partially execute on missing data.

### Definition: Coalescing (The Boundary)

To exit the Availability domain (e.g., to render a UI), one must provide a **Fallback**.

$$
\text{Coalesce}(\mathbb{A}\langle T \rangle, \text{Fallback}) = \begin{cases}
v & \text{if } \text{Present}(v) \\
\text{Fallback} & \text{if } \text{Absent}(\epsilon)
\end{cases}
$$

### Implication: The Suspense Equivalence

The algebraic operation of Coalescing is equivalent to the UI pattern known as **Suspense**.

- The Dependency Tree represents the computation $f^*$.
- The Suspense Boundary represents the Coalesce function.
- The Fallback UI represents the fallback value.

**Proof:**
Since Absent states propagate up the tree automatically (Short-Circuiting), a single "Pending" resource deep in the hierarchy effectively "suspends" the entire branch until it hits a Coalesce boundary.

## 7. The Hysteresis Guard (Self-Healing)

**Vulnerability:** If the Runtime suppresses a `Suspend` signal to prevent flicker (Hysteresis), but the data _never_ comes back, the UI hangs on stale data forever.

**Constraint:** Implement **Self-Healing Timers**.

- **Action:** When the Runtime decides to suppress a `Suspend` signal (Output Policy), it **MUST** schedule a oneshot timer (e.g., 50ms). When the timer fires, it forces a re-evaluation to commit the `Absent` state if the data is still missing.
- **Optimization:** If the resource recovers before the timer fires, **cancel the timer** to avoid "Ghost Wakeups."
