use exosuit_reactivity::engine::Engine;
use exosuit_reactivity::revision::{Epoch, Revision};
use exosuit_reactivity::trace::{Trace, TraceDigest};
use exosuit_reactivity::types::CellId;
use std::time::Duration;

#[test]
fn test_source_scoped_invalidation() {
    let mut engine = Engine::new(Duration::from_secs(60));
    let epoch = Epoch::new();

    // 1. Setup Sub-Structural Cell
    let source_id = "config.json";
    let pointer = "/debug";
    let cell_id = CellId::new(source_id, pointer);
    let rev1 = Revision::memory(epoch, 1);

    engine.set_cell(cell_id.clone(), rev1.clone());

    // 2. Setup Root depending on it
    let mut trace = Trace::new();
    trace.record(cell_id.clone(), rev1.clone());
    let value = serde_json::json!({"debug": true});
    engine.register_root("root1".to_string(), trace.clone(), value.clone(), true);
    let digest = trace.digest();

    // 3. Verify Root is valid
    assert!(engine.validate_root("root1", &digest));

    // 4. Invalidate the SOURCE (File Event)
    let source_cell = CellId::root(source_id);
    let affected = engine.invalidate_cell(&source_cell);

    // 5. Verify Root is affected
    assert!(affected.contains(&"root1".to_string()));

    // 6. Verify Root is now invalid (because cell is missing from state)
    assert!(!engine.validate_root("root1", &digest));
}

#[test]
fn test_trace_deduplication() {
    let mut trace = Trace::new();
    let epoch = Epoch::new();
    let cell_id = CellId::root("common");
    let rev = Revision::memory(epoch, 1);

    // Record same dependency twice
    trace.record(cell_id.clone(), rev.clone());
    trace.record(cell_id.clone(), rev.clone());

    // Should be 1 entry
    assert_eq!(trace.entries().count(), 1);

    // Digest should be same as if recorded once
    let mut trace2 = Trace::new();
    trace2.record(cell_id.clone(), rev.clone());
    assert_eq!(trace.digest(), trace2.digest());
}
