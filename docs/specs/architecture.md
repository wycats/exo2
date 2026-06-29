# **Validation-Based Reactivity: The Architectural Explainer**

Status: APPROVED (Dec 7, 2025\)  
Version: 1.1

## **1. The Core Philosophy**

We are building a **Validation-Based** reactivity system for a distributed editor (Exosuit Core $\leftrightarrow$ VS Code).

Traditional reactivity (RxJS, Signals) is Push-Based: "Value A changed to 42."  
Our system is Pull-Based: "Something changed. Check your assumptions."

### **Why Invert Control?**

1. **Glitch Freedom:** In a distributed system, pushing values creates race conditions ("Tearing"). Pulling allows the client to decide *when* to reconcile.  
2. **Bandwidth Efficiency:** We don't send data for hidden tabs. If the client doesn't ask, we don't calculate.  
3. **Traceability:** Validity is proven by a simple "Shopping Receipt" (Trace), not a complex graph traversal.

## **2. The Three Layers of Integrity**

The architecture is built in three interlocking layers. Each layer solves specific failure modes introduced by the layer below.

### **Layer 1: The Physics (The Algebra)**

* **Source:** reactivity_algebra.md  
* **Role:** Defines the mathematical invariants.  
* **Key Laws:**  
  * **Strict Equality:** revisions are only valid if identical. Partial ordering is forbidden.  
  * **Generative Identity:** Impure functions (randomness) must generate new IDs on every run.  
  * **Optimistic Concurrency:** We don't stop the world. We calculate, then verify inputs didn't change.

### **Layer 2: The Protocol (The RFC)**

* **Source:** rfc_0037.md  
* **Role:** Defines how the Physics is transmitted over the wire.  
* **Key Mechanism:** The **Verifiable Fetch**.  
  * Client: "I want File A, but only if it matches Digest X."  
  * Server: "Digest X is stale. Retry."  
  * *Result:* Frame Integrity. The client never renders mixed states.

### **Layer 3: The Runtime (Requirements)**

* **Source:** system_requirements.md  
* **Role:** Defines how the code must be written to respect the Physics.  
* **Key Constraints:**  
  * **Two-Phase Commit:** Action (Write) $\to$ Render (Read).  
  * **Backflow Prohibition:** Writing to a cell during Render causes a hard crash. This prevents cycles structurally.  
  * **Eviction Priority (The Swiss Cheese Rule):** Impure nodes (Generative IDs) must be prioritized for retention over Pure nodes. Pure nodes can be re-derived; Impure nodes are lost forever if evicted (Nonce loss).

## **3. The "Four Horsemen" of Distributed Reactivity**

We explicitly engineered this system to defeat four specific theoretical bugs.

| The Bug | The Symptoms | The Fix | Where it lives |
| :---- | :---- | :---- | :---- |
| **The Reincarnation Bug** | Server restarts. Counter resets to 1. Client thinks old data (Rev 1) is valid. | **Epoch-Scoped Revisions** | Algebra Sec 1 |
| **The Tearing Bug** | UI shows File List from $T_1$ and Editor Content from $T_2$. | **Verifiable Fetch** | Algebra Sec 6 |
| **The Split-Brain Bug** | Cache eviction causes "Old Randomness" to mix with "New Randomness." | **Generative Nonces** | Algebra Sec 7 |
| **Zeno's Paradox** | Fast updates starve slow clients (infinite loading). | **Snapshot Pinning** | Algebra Sec 6 |

## **4. Ecosystem Extensions**

### **Resource Management (resource_algebra.md)**

We treat Side Effects (Database connections, Sockets) as a special type of Reactive Computation.

* **The Facade Pattern:** The "Value" of the computation is a generic interface.  
* **The Lifecycle:** The "Implementation" is torn down and rebuilt whenever the Trace changes.  
* **Goal:** Use the reactivity system to manage the *lifecycle* of things that aren't reactive.

### **The Scheduler (scheduler_algebra.md)**

We treat Time as a constraint.

* **Coalescing:** If a value changes 50 times in a frame, we only run side effects for the final state.  
* **Priority:** Parent resources must init before Child resources.

## **5. How to Read the Spec Pack**

1. **Start with architecture_explainer.md** (This file) for the mental model.  
2. **Read rfc_0037.md** to understand the User Flow and API.  
3. **Read reactivity_algebra.md** when you need to know *exactly* how to implement a hash or comparison logic.  
4. **Refer to system_requirements.md** when writing the actual Rust/TS code to ensure you don't violate safety constraints.
