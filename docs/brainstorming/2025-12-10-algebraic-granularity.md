# Council Session: Algebraic Unification of Granularity

**Date**: December 10, 2025
**Topic**: Lifting "Invalidates When" (Granularity) into the Core Reactivity Algebra.
**Attendees**:

- **Leslie Lamport** (The Logician)
- **Umut Acar** (The Academic)
- **Barbara Liskov** (The Architect)
- **Niko Matsakis** (The Pragmatist)
- **Claude Shannon** (The Wildcard)
- **Yehuda Katz** (The User)

---

## Part 1: The Graveyard (Anti-Patterns)

**Niko Matsakis**: We need to be honest about the current state of "Incremental" systems. In Salsa (and most build systems), we rely on **Coarse Equivalence**. If a file changes, we re-parse the whole thing. If a struct changes, we invalidate all readers. We try to patch it with "Early Cutoff" (checking if the output is equal), but that requires _running_ the computation first.

**Yehuda Katz**: Exactly. In a text editor, "running first" is too late. If I type a character in a comment, I can't afford to re-run the "Find References" query just to see that the result is the same. I need to know _a priori_ that the change is irrelevant.

**Barbara Liskov**: The trap is **Leaky Abstractions**. I see proposals for "Bitmasks" and "Slot IDs" flying around. If we bake `HAMT_SLOT_5` into the core reactivity engine, we've failed. The engine shouldn't know about Hash Tries. It should know about _Abstract Validators_.

**Umut Acar**: The academic graveyard is full of **"Dynamic Dependency Graphs"** that are too heavy. If we store a closure for every dependency to check validity, the memory overhead kills us. We need something that is $O(1)$ to store and $O(1)$ to check.

**Claude Shannon**: The biggest anti-pattern is **False Entropy**. We treat a "Revision ID change" as a signal that "Information has changed." But often, it's just noise (a timestamp update, a re-allocation). We are reacting to noise, not signal.

---

## Part 2: The Blue Sky (The Magic Wand)

**Leslie Lamport**: Let's formalize this. A dependency is not a pointer to a cell. It is a **Predicate** over the state space.
$$D = \langle C, P \rangle$$
The system is valid if $P(C_{now}) \iff P(C_{then})$.
If we can define $P$ such that it only observes the "relevant bits," we solve the problem.

**Yehuda Katz**: But Leslie, we can't evaluate arbitrary predicates at runtime. That's too slow.

**Claude Shannon**: Think of it as **Information Channels**. A Cell is a noisy channel. It broadcasts a massive signal (the whole value). The Observer is a filter. It only cares about specific frequencies (e.g., "Does key 'foo' exist?").
We need a way for the Channel to broadcast a **Delta** (what changed) and the Observer to check its **Filter** against that Delta.
$$\text{Invalidated} = \text{Entropy}(\text{Delta} \cap \text{Filter}) > 0$$

**Barbara Liskov**: This sounds like a Type System. We need an interface.

```rust
trait Validator {
    type Delta;
    fn isValid(&self, delta: &Self::Delta) -> bool;
}
```

The `Cell` defines the `Delta` type. The `Dependency` holds the `Validator`.
For a simple Cell, `Delta` is `()`. `isValid` is always `false` (if changed).
For a Map, `Delta` is `KeyHash`. `isValid` checks `self.key != delta.key`.

**Niko Matsakis**: This fits perfectly into the "Revision" model.
Currently: `Revision = u64`.
New Model: `Revision = (u64, Delta)`.
When we verify a dependency:

1. Check `current_revision == recorded_revision`. (Fast path).
2. If different, fetch `delta` from the cell.
3. Run `validator.isValid(delta)`. (The "Double Check").

**Umut Acar**: This is **"Change Propagation"** but lazy. Instead of pushing changes to all readers, we pull the delta only when we validate. It keeps the system "Pull-Based" (which Exosuit prefers) but gives us the precision of a "Push-Based" system.

---

## Part 3: The Synthesis (The Consensus)

**The Core Concept**: **The Intersection Principle**.
Invalidation occurs only when the **Mutation Delta** intersects with the **Query Projection**.

**Key Mechanisms**:

1.  **Abstract Delta Types**:
    The Core Engine does not define what a "Delta" is. It is an associated type of the `ReactiveCell`.

    - `AtomicCell`: Delta = `All`.
    - `CollectionCell`: Delta = `Path(Hash)`.
    - `StructCell`: Delta = `FieldID`.

2.  **The Validator Protocol**:
    Dependencies store a lightweight, opaque `Validator` object alongside the Revision ID.

    - `read()` stores `Validator::All`.
    - `get(k)` stores `Validator::Key(k)`.

3.  **The Two-Phase Check**:
    - **Phase 1 (Epoch)**: Has the Cell changed at all? (Integer compare).
    - **Phase 2 (Intersection)**: If changed, does `Validator.intersects(Cell.last_delta)`?

**Alignment Check**:

- **Exosuit Way**: "Context is King." This allows the Context (the Cell) to explain _how_ it changed, rather than just _that_ it changed.
- **Performance**: The `Validator` is usually just a `u64` (bitmask or hash). The check is bitwise AND. Extremely fast.
- **Generality**: Works for Maps, Sets, DOM Nodes (Attribute changes), and even Text Buffers (Range changes).

---

## Part 4: The Action Plan

1.  **Update Core Algebra**: We need to modify `reactivity.md` to include the **Validator Algebra**.
2.  **Define the Interface**: Create a spec for the `Validator` trait.
3.  **Retrofit Collections**: Ensure `reactive-collections.md` implements this interface, rather than defining its own ad-hoc logic.

**Consensus Reached.**
