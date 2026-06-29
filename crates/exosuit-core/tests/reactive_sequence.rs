use exosuit_core::{ReactiveSequence, Runtime};

#[test]
fn test_reactive_sequence_creation() {
    let runtime = Runtime::new();
    let seq = ReactiveSequence::<i32>::new(&runtime);

    assert_eq!(seq.len(), 0);
    assert!(seq.is_empty());
}

#[test]
fn test_reactive_sequence_push() {
    let runtime = Runtime::new();
    let seq = ReactiveSequence::new(&runtime);

    runtime.action(|| {
        seq.push(10);
        seq.push(20);
    });

    assert_eq!(seq.len(), 2);
    assert_eq!(seq.get(0), Some(10));
    assert_eq!(seq.get(1), Some(20));
}
