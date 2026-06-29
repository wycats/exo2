<!-- exo:89 ulid:01kg5kp2fc58cbqa9nz622h73e -->

# RFC 89: The Exosuit Application Architecture


# RFC 0089: The Exosuit Application Architecture

## Abstract

This RFC defines the high-level application architecture for the Exosuit VS Code extension. It builds directly upon the **Validation-Based Reactivity** primitives defined in **RFC 0026** to create a "Twin-State" system.

In this model, the **Rust Core** acts as the authoritative "Server" (The Brain), and the **VS Code Extension** acts as a thin, verifiable "Client" (The Body). This separation ensures robustness, eliminates "glitches," and enables cryptographic verification of the UI state by AI agents.

## 1. Foundations: The "Physics" of Reactivity

This architecture relies entirely on the invariants provided by **RFC 0026**:

1.  **Strict Equality**: State changes are detected via strict equality of Revisions (Digests/Counters).
2.  **Snapshot Isolation**: The Client never sees a "torn" frame.
3.  **Verifiable Fetch**: The Client pulls state only when it matches the expected Digest.

**RFC 0026 provides the "Wire Protocol." RFC 0025 defines the "Application Layer" built on top of it.**

## 2. The "Twin-State" Model

We model the system as two distinct entities connected by the Reactivity Bridge.

### 2.1. The Core (Server)

- **Runtime**: Rust (WASM).
- **Role**: **The Source of Truth**.
- **Responsibilities**:
  - **State Management**: Manages `Cells` (Files, Config, In-Memory State).
  - **Computation**: Executes business logic and records `Traces`.
  - **Root Exposure**: Exposes specific Computations as "Roots" (e.g., `ThePlan`, `TheDashboard`).
  - **Derived Roots**: Defines computed roots that act as reactive caches over primary state.
- **Invariants**: Pure, Deterministic, Epoch-Scoped.

### 2.2. The Shell (Client)

- **Runtime**: TypeScript (VS Code Extension Host).
- **Role**: **The Projection Layer**.
- **Responsibilities**:
  - **Shadow Graph**: Maintains a local cache of the Core's Roots, indexed by `RootID`.
  - **Rendering**: Projects the Shadow Graph onto VS Code APIs (TreeViews, Webviews, Decorators).
  - **Interaction**: Captures user events and forwards them as **Actions** to the Core.
- **Invariants**: "Dumb" (No Business Logic), Reactive.

## 3. The "Volar-Like" Abstraction

We adopt the architectural pattern popularized by Volar (Vue's LSP) and other modern tools: **The IDE is just a View.**

### 3.1. Zero Business Logic in TypeScript

The TypeScript layer must contain **zero** business logic. It is strictly a **Renderer**.

- **Bad**: `if (task.status === 'done') { icon = 'check' }` (Logic in TS).
- **Good**: `icon = task.icon` (Logic in Rust, TS just renders).

### 3.2. Renderers as Pure Functions

Every UI element in VS Code is treated as a reactive projection of a Core Root.

#### A. Tree Views

- **Root**: `Plan` (TOML).
- **Renderer**: `renderPlanTree(plan: Plan) -> TreeItem[]`.
- **Reactivity**: When the `Plan` digest changes, the Shell fetches the new Plan and re-runs `renderPlanTree`.

#### B. Webviews (Dashboards)

- **Root**: `DashboardContext`.
- **Renderer**: `renderDashboard(ctx: DashboardContext) -> HTML`.
- **Reactivity**: The Webview is a "Sub-Client." It receives `RemoteSignal`s from the Shell and updates its DOM using a fine-grained framework (e.g., Svelte Runes).

#### C. Decorators & Status Bar

- **Root**: `FileStatus`.
- **Renderer**: `renderDecorations(status: FileStatus) -> DecorationOptions[]`.

### 3.3. Derived Roots as Reactive Caches

Derived roots are computed projections of core state that behave like roots in the Reactivity Bridge. The `DerivedRootRegistry` keeps these caches consistent by tracking dependencies and invalidating derived outputs when underlying roots change. The Shell treats derived roots exactly like disk-backed roots: it fetches them on demand, renders them as pure projections, and relies on the same invalidation pipeline to ensure correctness without duplicating business logic in TypeScript.

## 4. AI Verifiability & The Feedback Loop

This architecture solves the "Blind Agent" problem. An AI agent modifying code needs to know _for sure_ that the UI has updated to reflect its changes.

### 4.1. The Verification Protocol

Because state is **Content-Addressable** (via Digests), verification becomes an $O(1)$ equality check.

1.  **Action**: Agent modifies `plan.toml`.
2.  **Prediction**: Agent calculates the expected Digest $D_{new}$ of the plan file.
3.  **Observation**: Agent polls the Shell's **Introspection API**.
4.  **Verification**: Agent waits until `Shell.getDigest("ThePlan") == D_{new}`.

**Proof**: If the Shell reports Digest $D_{new}$, and the Shell enforces **Frame Integrity** (RFC 0026), then the UI _must_ be rendering the data corresponding to $D_{new}$. Visual inspection is mathematically unnecessary.

### 4.2. Introspection API

The Shell exposes a "Debug Port" for the Agent (and human developers).

- **Command**: `exosuit.debug.dumpState`
- **Output**:
  ```json
  {
    "epoch": "550e8400-e29b-...",
    "roots": {
      "ThePlan": {
        "digest": "a1b2c3d4...",
        "status": "valid",
        "last_updated": 1678886400
      },
      "TheDashboard": {
        "digest": "e5f6g7h8...",
        "status": "fetching"
      }
    }
  }
  ```

## 5. Implementation Strategy

### Phase 41: Reactive Core (RFC 0026)

Implement the "Physics": Cells, Revisions, Traces, and the Wire Protocol in `exosuit-core`.

### Phase 42: The Shell Infrastructure

Implement the "Body":

- **`ExosuitClient`**: The TypeScript class that manages the connection to WASM.
- **`ShadowGraph`**: The local cache of Roots.
- **`ReconciliationLoop`**: The `Validate -> Fetch -> Render` cycle.

### Phase 43: Migration

Migrate existing views to the new architecture:

- `PlanTreeView` -> `PlanRenderer`.
- `DashboardWebview` -> `DashboardRenderer`.

