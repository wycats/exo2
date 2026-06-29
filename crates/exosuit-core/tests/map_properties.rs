use exosuit_core::ReactiveMap;
use exosuit_reactivity::Runtime;
use proptest::prelude::*;
use std::collections::HashMap;

#[derive(Debug, Clone)]
enum MapOperation {
    Insert(i32, i32),
    Remove(i32),
}

proptest! {
    #[test]
    fn test_map_model_consistency(ops in prop::collection::vec(any_map_op(), 0..50)) {
        let runtime = Runtime::new();
        let map = ReactiveMap::<i32, i32>::new(&runtime);
        let mut model = HashMap::new();

        for op in ops {
            match op {
                MapOperation::Insert(k, v) => {
                    runtime.action(|| {
                        map.insert(k, v);
                    });
                    model.insert(k, v);
                }
                MapOperation::Remove(k) => {
                    runtime.action(|| {
                        map.remove(&k);
                    });
                    model.remove(&k);
                }
            }

            // Verify Length
            assert_eq!(map.len(), model.len(), "Length mismatch");

            // Verify Content
            for (k, v) in &model {
                assert_eq!(map.get(k), Some(*v), "Content mismatch for key {}", k);
            }
        }
    }
}

fn any_map_op() -> impl Strategy<Value = MapOperation> {
    prop_oneof![
        (any::<i32>(), any::<i32>()).prop_map(|(k, v)| MapOperation::Insert(k, v)),
        any::<i32>().prop_map(MapOperation::Remove),
    ]
}
