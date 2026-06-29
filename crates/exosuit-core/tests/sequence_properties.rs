use exosuit_core::ReactiveSequence;
use exosuit_reactivity::{Availability, Runtime};
use proptest::prelude::*;
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone)]
enum Operation {
    Push(i32),
    // We can add more operations later as the API expands (Insert, Remove, etc.)
}

proptest! {
    #[test]
    fn test_sequence_model_consistency(ops in prop::collection::vec(any_op(), 0..50)) {
        let runtime = Runtime::new();
        let seq = ReactiveSequence::<i32>::new(&runtime);
        let mut model = Vec::new();

        for op in ops {
            match op {
                Operation::Push(val) => {
                    runtime.action(|| {
                        seq.push(val);
                    });
                    model.push(val);
                }
            }

            // Verify Length
            assert_eq!(seq.len(), model.len(), "Length mismatch");

            // Verify Content
            for (i, &expected) in model.iter().enumerate() {
                assert_eq!(seq.get(i), Some(expected), "Content mismatch at index {}", i);
            }
        }
    }
}

fn any_op() -> impl Strategy<Value = Operation> {
    prop_oneof![any::<i32>().prop_map(Operation::Push),]
}

#[test]
fn test_granularity_check() {
    // This is a specific test to verify that updates are granular.
    // Since we don't have fine-grained subscriptions yet (ReactiveSequence currently updates as a whole),
    // this test documents the CURRENT behavior (coarse-grained) or verifies fine-grained if implemented.

    // NOTE: Based on current implementation of ReactiveSequence:
    // struct SequenceState<T> { items: Vec<T> }
    // It stores the WHOLE vector in a single cell.
    // Therefore, ANY push updates the WHOLE cell.
    // So granularity is currently "Collection Level", not "Item Level".

    let runtime = Runtime::new();
    let seq = ReactiveSequence::<i32>::new(&runtime);
    let seq_clone = seq.clone();

    // Track how many times the length computation runs
    let run_count = Rc::new(RefCell::new(0));
    let run_count_clone = run_count.clone();

    let _computed = runtime.computed("len_tracker", move || {
        *run_count_clone.borrow_mut() += 1;
        let len = seq_clone.len();
        Availability::Present(json!(len))
    });

    // Initial read
    let _ = _computed.read();
    assert_eq!(*run_count.borrow(), 1, "Should run once initially");

    // Push item
    runtime.action(|| {
        seq.push(1);
    });

    // Read again
    let _ = _computed.read();
    assert_eq!(*run_count.borrow(), 2, "Should run again after push");

    // Push another
    runtime.action(|| {
        seq.push(2);
    });

    let _ = _computed.read();
    assert_eq!(*run_count.borrow(), 3, "Should run again after second push");
}

#[derive(Debug, Clone)]
enum CellOperation {
    Set(i32),
}

proptest! {
    #[test]
    fn test_cell_model_consistency(ops in prop::collection::vec(any_cell_op(), 0..50)) {
        let runtime = Runtime::new();
        let cell = runtime.cell("test_cell", json!(0));

        for op in ops {
            match op {
                CellOperation::Set(val) => {
                    runtime.action(|| {
                        cell.set(json!(val));
                    });
                    let cell_val = cell.get().unwrap().as_i64().unwrap() as i32;
                    assert_eq!(cell_val, val, "Content mismatch");
                }
            }
        }
    }
}

fn any_cell_op() -> impl Strategy<Value = CellOperation> {
    prop_oneof![any::<i32>().prop_map(CellOperation::Set),]
}
