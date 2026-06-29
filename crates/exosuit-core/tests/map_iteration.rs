use exosuit_core::ReactiveMap;
use exosuit_reactivity::{Availability, Runtime};
use serde_json::json;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn test_keys_stability() {
    let runtime = Runtime::new();
    let map = ReactiveMap::<String, i32>::new(&runtime);
    let execution_count = Rc::new(RefCell::new(0));

    runtime.action(|| {
        map.insert("A".to_string(), 1);
        map.insert("B".to_string(), 2);
    });

    let count = execution_count.clone();
    let map_clone = map.clone();
    let keys_computed = runtime.computed("keys", move || {
        *count.borrow_mut() += 1;
        let keys = map_clone.keys();
        // Sort for deterministic comparison
        let mut sorted = keys.clone();
        sorted.sort();
        Availability::Present(json!(sorted))
    });

    // 1. Initial Read
    assert_eq!(
        keys_computed.read(),
        Availability::Present(json!(["A", "B"]))
    );
    assert_eq!(*execution_count.borrow(), 1);

    // 2. Update Value of "A"
    // Should NOT invalidate keys()
    runtime.action(|| {
        map.insert("A".to_string(), 100);
    });

    let _ = keys_computed.read();
    assert_eq!(
        *execution_count.borrow(),
        1,
        "keys() should be stable under value updates"
    );

    // 3. Add Key "C"
    // Should invalidate keys()
    runtime.action(|| {
        map.insert("C".to_string(), 3);
    });

    let res = keys_computed.read();
    assert_eq!(res, Availability::Present(json!(["A", "B", "C"])));
    assert_eq!(
        *execution_count.borrow(),
        2,
        "keys() should update on insertion"
    );
}

#[test]
fn test_entries_invalidation() {
    let runtime = Runtime::new();
    let map = ReactiveMap::<String, i32>::new(&runtime);
    let execution_count = Rc::new(RefCell::new(0));

    runtime.action(|| {
        map.insert("A".to_string(), 1);
    });

    let count = execution_count.clone();
    let map_clone = map.clone();
    let entries_computed = runtime.computed("entries", move || {
        *count.borrow_mut() += 1;
        let mut entries = map_clone.entries();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Availability::Present(json!(entries))
    });

    // 1. Initial Read
    assert_eq!(
        entries_computed.read(),
        Availability::Present(json!([["A", 1]]))
    );
    assert_eq!(*execution_count.borrow(), 1);

    // 2. Update Value of "A"
    // Should invalidate entries()
    runtime.action(|| {
        map.insert("A".to_string(), 2);
    });

    let res = entries_computed.read();
    assert_eq!(res, Availability::Present(json!([["A", 2]])));
    assert_eq!(
        *execution_count.borrow(),
        2,
        "entries() should update on value change"
    );
}

#[test]
fn test_set_equality_noop() {
    let runtime = Runtime::new();
    let map = ReactiveMap::<String, i32>::new(&runtime);
    let execution_count = Rc::new(RefCell::new(0));

    runtime.action(|| {
        map.insert("A".to_string(), 1);
    });

    let count = execution_count.clone();
    let map_clone = map.clone();
    let get_a = runtime.computed("get_a", move || {
        *count.borrow_mut() += 1;
        let val = map_clone.get(&"A".to_string());
        Availability::Present(json!(val))
    });

    // 1. Initial Read
    assert_eq!(get_a.read(), Availability::Present(json!(1)));
    assert_eq!(*execution_count.borrow(), 1);

    // 2. Set "A" to 1 (Same Value)
    // Should NOT invalidate
    runtime.action(|| {
        map.insert("A".to_string(), 1);
    });

    let _ = get_a.read();
    assert_eq!(
        *execution_count.borrow(),
        1,
        "Setting same value should be no-op"
    );

    // 3. Set "A" to 2 (Different Value)
    // Should invalidate
    runtime.action(|| {
        map.insert("A".to_string(), 2);
    });

    let res = get_a.read();
    assert_eq!(res, Availability::Present(json!(2)));
    assert_eq!(*execution_count.borrow(), 2);
}
