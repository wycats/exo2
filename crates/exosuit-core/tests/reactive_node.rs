use exosuit_core::{ReactiveNode, Runtime};

#[test]
fn test_reactive_node_structure() {
    let runtime = Runtime::new();
    let node = ReactiveNode::new(&runtime, 42);

    let node_copy = node.clone(); // Should be Clone
    assert_eq!(node.get::<i32>(), 42);
    assert_eq!(node_copy.get::<i32>(), 42);
}
