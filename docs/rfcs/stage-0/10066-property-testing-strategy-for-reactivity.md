<!-- exo:10066 ulid:01kmzxefedbgwkg04grt5rppe8 -->


# RFC 10066: Property Testing Strategy for Reactivity

## Summary

This RFC establishes a comprehensive strategy for property-based testing of the Exosuit Reactivity Engine and Collections. It moves beyond simple model consistency to verify complex behaviors like graph propagation, caching, and composition.

## Motivation

Current property tests verify that `ReactiveSequence` behaves like `Vec` and `ReactiveMap` behaves like `HashMap`. While essential, this is insufficient for a reactivity engine. We must also guarantee:

1.  **Composition**: Nested collections (e.g., `Map<String, Sequence<i32>>`) behave correctly.
2.  **Propagation**: Updates propagate correctly through deep computation graphs.
3.  **Efficiency**: The engine does not over-compute (Caching/Memoization).
4.  **Glitch Freedom**: No intermediate inconsistent states are observable.

## Strategy

We will use `proptest` to generate random **Reactivity Graphs** and **Operation Sequences**.

### 1. The Graph Model

Instead of testing a single collection, we will generate a random DAG of computations:

- **Inputs**: `Cell<T>`, `ReactiveSequence<T>`, `ReactiveMap<K, V>`.
- **Nodes**: `Computed<T>` (pure functions of inputs or other nodes).
- **Outputs**: The final values read from leaf nodes.

### 2. Test Scenarios

#### A. Composition & Nesting

- **Goal**: Verify that collections can contain other reactive primitives.
- **Scenario**: Create a `ReactiveMap<String, ReactiveSequence<i32>>`.
- **Operations**:
  - Add/Remove keys (Map level).
  - Push/Pop items in a specific sequence (Nested level).
- **Invariant**: The aggregate state matches a `HashMap<String, Vec<i32>>`.

#### B. Deep Propagation

- **Goal**: Verify updates travel from source to sink through multiple layers.
- **Scenario**: `A -> B -> C -> D`.
- **Operation**: Mutate `A`.
- **Invariant**: `D` updates to the correct value derived from `A`.

#### C. Caching & Efficiency

- **Goal**: Verify that `Computed` nodes only re-run when dependencies change.
- **Scenario**: `A -> B`, `C -> D`. `E = B + D`.
- **Operation**: Mutate `A`.
- **Invariant**: `B` and `E` re-run. `D` does **not** re-run.
- **Mechanism**: Instrument `Computed` closures with counters to track execution count.

#### D. Glitch Freedom

- **Goal**: Verify that a diamond dependency (`A -> B`, `A -> C`, `D = B + C`) never sees inconsistent versions of `A`.
- **Operation**: Mutate `A`.
- **Invariant**: `D` is only observed after both `B` and `C` have updated.

### 3. Implementation Plan

1.  **Graph Generator**: A `proptest` strategy to generate random DAGs of `Computed` nodes.
2.  **Instrumentation**: A wrapper around `Runtime` or `Computed` to track execution counts.
3.  **Model Oracle**: A parallel "Shadow Graph" using standard Rust types (`Rc`, `RefCell`) to predict expected values.

## Example: Caching Test

```rust
proptest! {
    #[test]
    fn test_caching_behavior(ops in vec(any_op(), 0..100)) {
        let runtime = Runtime::new();
        let input = runtime.cell("input", 0);

        let counter = Rc::new(RefCell::new(0));
        let c = counter.clone();

        let derived = runtime.computed("derived", move || {
            *c.borrow_mut() += 1;
            input.get().unwrap()
        });

        // ... execute ops ...

        // Assert that counter only increments when input actually changes
    }
}
```
