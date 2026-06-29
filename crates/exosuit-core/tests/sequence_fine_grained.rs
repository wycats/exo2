use exosuit_core::reactive_sequence::ReactiveSequence;
use exosuit_reactivity::{Availability, Runtime};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn test_fine_grained_updates() {
    let runtime = Runtime::new();
    let seq = ReactiveSequence::<i32>::new(&runtime);

    seq.push(10);
    seq.push(20);

    let revisions = Rc::new(RefCell::new(Vec::new()));
    let rev_clone = revisions.clone();

    let seq_clone = seq.clone();
    // Signal 1: Depends on index 0
    let _s1 = runtime.computed("s1", move || {
        rev_clone.borrow_mut().push("s1");
        let val = seq_clone.get(0).unwrap();
        Availability::Present(json!(val))
    });

    let rev_clone = revisions.clone();
    let seq_clone = seq.clone();
    // Signal 2: Depends on length (structure)
    let _s2 = runtime.computed("s2", move || {
        rev_clone.borrow_mut().push("s2");
        let len = seq_clone.len();
        Availability::Present(json!(len))
    });

    // Initial run
    let _ = _s1.read();
    let _ = _s2.read();
    revisions.borrow_mut().clear();

    // Update index 1 (should not affect s1 or s2)
    runtime.action(|| {
        seq.set(1, 30);
    });

    // Re-read to trigger re-evaluation if invalidated
    let _ = _s1.read();
    let _ = _s2.read();

    // In a fine-grained system, neither s1 nor s2 should run.
    // s1 depends on index 0.
    // s2 depends on structure (len). Updating an existing item doesn't change structure.
    assert!(
        revisions.borrow().is_empty(),
        "Updates to index 1 should not invalidate index 0 or len"
    );

    // Update index 0 (should affect s1)
    runtime.action(|| {
        seq.set(0, 11);
    });
    let _ = _s1.read();
    let _ = _s2.read();

    assert_eq!(
        revisions.borrow().as_slice(),
        &["s1"],
        "Update to index 0 should invalidate s1"
    );
    revisions.borrow_mut().clear();

    // Push item (should affect s2)
    runtime.action(|| {
        seq.push(40);
    });
    let _ = _s1.read();
    let _ = _s2.read();

    assert_eq!(
        revisions.borrow().as_slice(),
        &["s2"],
        "Push should invalidate s2 (structure)"
    );
}

#[test]
fn test_equality_optimization() {
    let runtime = Runtime::new();
    let seq = ReactiveSequence::<i32>::new(&runtime);

    seq.push(10);

    let revisions = Rc::new(RefCell::new(0));
    let rev_clone = revisions.clone();
    let seq_clone = seq.clone();

    let _s = runtime.computed("s", move || {
        *rev_clone.borrow_mut() += 1;
        let val = seq_clone.get(0).unwrap();
        Availability::Present(json!(val))
    });

    let _ = _s.read();
    revisions.replace(0);

    // Set same value
    runtime.action(|| {
        seq.set(0, 10);
    });
    let _ = _s.read();
    assert_eq!(*revisions.borrow(), 0, "Setting same value should be no-op");

    // Set different value
    runtime.action(|| {
        seq.set(0, 20);
    });
    let _ = _s.read();
    assert_eq!(
        *revisions.borrow(),
        1,
        "Setting different value should invalidate"
    );
}
