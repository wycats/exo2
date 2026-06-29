use exosuit_core::ReactiveNode;
use exosuit_reactivity::Runtime;
use serde_json::json;

#[test]
fn test_leak_verification_harness() {
    let runtime = Runtime::new();

    assert_eq!(runtime.active_cell_count(), 0, "Runtime should start empty");

    let node_id;
    {
        let node = ReactiveNode::new(&runtime, json!("test"));
        node_id = node.id().clone();

        assert_eq!(runtime.active_cell_count(), 1, "Runtime should have 1 cell");
        assert!(runtime.get_cell(&node_id).is_some());
    } // node is dropped here

    // Verify that we can DETECT the leak.
    // Now we expect the leak to be GONE.
    assert_eq!(
        runtime.active_cell_count(),
        0,
        "Runtime should be empty after node drop"
    );
    assert!(
        runtime.get_cell(&node_id).is_none(),
        "Cell should be removed from Runtime"
    );
}
