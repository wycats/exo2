use exosuit_core::ReactiveMap;
use exosuit_reactivity::{Availability, Runtime};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn test_map_has_stability_on_value_update() {
    let runtime = Runtime::new();
    let map = ReactiveMap::<String, i32>::new(&runtime);
    let execution_count = Rc::new(RefCell::new(0));

    // Setup: Map has "A" -> 1
    runtime.action(|| {
        map.insert("A".to_string(), 1);
    });

    // Computed: checks has("A")
    let count = execution_count.clone();
    let map_clone = map.clone();
    let has_a = runtime.computed("has_a", move || {
        *count.borrow_mut() += 1;
        let exists = map_clone.contains_key(&"A".to_string());
        Availability::Present(json!(exists))
    });

    // 1. Initial Read
    assert_eq!(has_a.read(), Availability::Present(json!(true)));
    assert_eq!(*execution_count.borrow(), 1);

    // 2. Update "A" -> 2 (Value change, Key existence same)
    runtime.action(|| {
        map.insert("A".to_string(), 2);
    });

    // 3. Check Invalidation
    // SPEC: "if the answer is yes: updates don't invalidate has()"
    let _ = has_a.read();
    assert_eq!(
        *execution_count.borrow(),
        1,
        "Should NOT re-run on value update"
    );
}

#[test]
fn test_map_has_invalidation_on_removal() {
    let runtime = Runtime::new();
    let map = ReactiveMap::<String, i32>::new(&runtime);
    let execution_count = Rc::new(RefCell::new(0));

    runtime.action(|| {
        map.insert("A".to_string(), 1);
    });

    let count = execution_count.clone();
    let map_clone = map.clone();
    let has_a = runtime.computed("has_a", move || {
        *count.borrow_mut() += 1;
        let exists = map_clone.contains_key(&"A".to_string());
        Availability::Present(json!(exists))
    });

    assert_eq!(has_a.read(), Availability::Present(json!(true)));
    assert_eq!(*execution_count.borrow(), 1);

    // 2. Remove "A"
    // SPEC: "if the answer is yes: deletions invalidate has()"
    runtime.action(|| {
        map.remove(&"A".to_string());
    });

    let res = has_a.read();
    assert_eq!(res, Availability::Present(json!(false)));
    assert_eq!(*execution_count.borrow(), 2, "Should re-run on removal");
}

#[test]
fn test_map_has_invalidation_on_insertion() {
    let runtime = Runtime::new();
    let map = ReactiveMap::<String, i32>::new(&runtime);
    let execution_count = Rc::new(RefCell::new(0));

    let count = execution_count.clone();
    let map_clone = map.clone();
    let has_b = runtime.computed("has_b", move || {
        *count.borrow_mut() += 1;
        let exists = map_clone.contains_key(&"B".to_string());
        Availability::Present(json!(exists))
    });

    // 1. Initial Read (False)
    assert_eq!(has_b.read(), Availability::Present(json!(false)));
    assert_eq!(*execution_count.borrow(), 1);

    // 2. Insert "B" -> 1
    // SPEC: "if the answer is no: later a map set occurs with the same key, it invalidates"
    runtime.action(|| {
        map.insert("B".to_string(), 1);
    });

    let res = has_b.read();
    assert_eq!(res, Availability::Present(json!(true)));
    assert_eq!(*execution_count.borrow(), 2, "Should re-run on insertion");
}
