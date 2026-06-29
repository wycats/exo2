use exosuit_core::{ReactiveNode, ReactiveSequence};
use exosuit_reactivity::Runtime;
use serde_json::json;

struct Component<'a> {
    _node: ReactiveNode<'a>,
    seq: ReactiveSequence<'a, i32>,
}

#[test]
fn test_complex_graph_leak() {
    let runtime = Runtime::new();
    assert_eq!(runtime.active_cell_count(), 0);

    {
        let comp = Component {
            _node: ReactiveNode::new(&runtime, json!("test")),
            seq: ReactiveSequence::new(&runtime),
        };
        comp.seq.push(1);

        // 1 node cell + 1 sequence structure cell + 1 sequence item cell = 3 cells
        assert_eq!(runtime.active_cell_count(), 3);
    } // comp dropped

    // We assert that it DOES NOT LEAK.
    assert_eq!(
        runtime.active_cell_count(),
        0,
        "Runtime should be empty after component drop"
    );
}
