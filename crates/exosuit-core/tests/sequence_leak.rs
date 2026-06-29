use exosuit_core::ReactiveSequence;
use exosuit_reactivity::Runtime;

#[test]
fn test_sequence_leak() {
    let runtime = Runtime::new();

    assert_eq!(runtime.active_cell_count(), 0, "Runtime should start empty");

    {
        let seq = ReactiveSequence::<i32>::new(&runtime);
        seq.push(42);

        // 1 structure cell + 1 item cell = 2 cells
        assert_eq!(
            runtime.active_cell_count(),
            2,
            "Runtime should have 2 cells"
        );
    } // seq is dropped here

    // Verify leak is gone
    assert_eq!(
        runtime.active_cell_count(),
        0,
        "Runtime should be empty after sequence drop"
    );
}
