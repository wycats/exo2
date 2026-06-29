use exosuit_core::ReactiveSequence;
use exosuit_reactivity::{Availability, Runtime};
use serde_json::json;

#[test]
fn test_sequence_reactivity() {
    let runtime = Runtime::new();
    let seq = ReactiveSequence::<i32>::new(&runtime);
    let seq_clone = seq.clone();

    // Create a derived computation that depends on the sequence length
    let count_computed = runtime.computed("count", move || {
        let len = seq_clone.len();
        Availability::Present(json!(len))
    });

    assert_eq!(
        count_computed.read(),
        Availability::Present(json!(0)),
        "Initial length should be 0"
    );

    // Push an item
    runtime.action(|| {
        seq.push(10);
    });

    // Verify computed updated
    assert_eq!(
        count_computed.read(),
        Availability::Present(json!(1)),
        "Length should update to 1"
    );

    // Push another
    runtime.action(|| {
        seq.push(20);
    });

    assert_eq!(
        count_computed.read(),
        Availability::Present(json!(2)),
        "Length should update to 2"
    );
}
