use exosuit_reactivity::trace::StateProvider;
use exosuit_reactivity::{CellId, Engine, Revision, Trace};
use serde_json::json;
use std::time::Duration;

fn setup() -> Engine {
    Engine::new(Duration::from_secs(60))
}

#[test]
fn test_chain_invalidation() {
    let mut engine = setup();
    let epoch = exosuit_reactivity::revision::Epoch::new();

    // C is a file
    let c_id = CellId::root("C");
    engine.set_cell(c_id.clone(), Revision::memory(epoch, 1));

    // B depends on C
    let mut trace_b = Trace::new();
    trace_b.record(c_id.clone(), Revision::memory(epoch, 1));
    engine.register_root("B".to_string(), trace_b, json!("val_b"), true);

    // A depends on B
    // Note: B is a Root, so A depends on CellId::root("B")
    let b_id = CellId::root("B");
    let mut trace_a = Trace::new();
    // We need to get the revision of B to record it.
    // In a real scenario, we'd fetch B, get its revision, then record.
    // Here we simulate it.
    let b_rev = engine.get_revision(&b_id).expect("B should exist");
    trace_a.record(b_id.clone(), b_rev);
    engine.register_root("A".to_string(), trace_a, json!("val_a"), true);

    // Update C
    engine.set_cell(c_id.clone(), Revision::memory(epoch, 2));
    let affected = engine.invalidate_cell(&c_id);

    // Should invalidate B
    assert!(affected.contains(&"B".to_string()));

    // Now we must "recompute" B to propagate to A
    // In the real system, the host does this.
    // Let's simulate B recomputing and changing.
    let mut trace_b_new = Trace::new();
    trace_b_new.record(c_id.clone(), Revision::memory(epoch, 2));
    engine.register_root("B".to_string(), trace_b_new, json!("val_b_2"), true);

    // Now A should be invalid because B changed
    // But wait, `register_root` doesn't return invalidated roots.
    // `register_root` updates the cell for B.
    // We need to check if A is valid.

    let digest = engine.get_root("A").unwrap().digest.clone();
    let a_valid = engine.validate_root("A", &digest);
    assert!(!a_valid, "A should be invalid because B changed");
}

#[test]
fn test_diamond_dependency() {
    let mut engine = setup();
    let epoch = exosuit_reactivity::revision::Epoch::new();

    // D is the base
    let d_id = CellId::root("D");
    engine.set_cell(d_id.clone(), Revision::memory(epoch, 1));

    // B -> D
    let mut trace_b = Trace::new();
    trace_b.record(d_id.clone(), Revision::memory(epoch, 1));
    engine.register_root("B".to_string(), trace_b, json!("val_b"), true);

    // C -> D
    let mut trace_c = Trace::new();
    trace_c.record(d_id.clone(), Revision::memory(epoch, 1));
    engine.register_root("C".to_string(), trace_c, json!("val_c"), true);

    // A -> B, C
    let b_id = CellId::root("B");
    let c_id = CellId::root("C");
    let mut trace_a = Trace::new();
    trace_a.record(b_id.clone(), engine.get_revision(&b_id).unwrap());
    trace_a.record(c_id.clone(), engine.get_revision(&c_id).unwrap());
    engine.register_root("A".to_string(), trace_a, json!("val_a"), true);

    // Update D
    engine.set_cell(d_id.clone(), Revision::memory(epoch, 2));
    let affected = engine.invalidate_cell(&d_id);

    // Should invalidate B and C
    assert!(affected.contains(&"B".to_string()));
    assert!(affected.contains(&"C".to_string()));

    // A should be invalid
    let digest = engine.get_root("A").unwrap().digest.clone();
    let a_valid = engine.validate_root("A", &digest);
    assert!(!a_valid);
}

#[test]
fn test_conditional_dependency_switch() {
    let mut engine = setup();
    let epoch = exosuit_reactivity::revision::Epoch::new();

    let true_id = CellId::root("TRUE");
    let false_id = CellId::root("FALSE");
    let cond_id = CellId::root("COND");

    engine.set_cell(true_id.clone(), Revision::memory(epoch, 1));
    engine.set_cell(false_id.clone(), Revision::memory(epoch, 1));
    engine.set_cell(cond_id.clone(), Revision::memory(epoch, 1)); // 1 = True

    // A depends on COND. If True, depends on TRUE.
    let mut trace_a = Trace::new();
    trace_a.record(cond_id.clone(), Revision::memory(epoch, 1));
    trace_a.record(true_id.clone(), Revision::memory(epoch, 1));
    engine.register_root("A".to_string(), trace_a, json!("val_a"), true);

    // 1. Modify FALSE. A should NOT be invalid.
    engine.set_cell(false_id.clone(), Revision::memory(epoch, 2));
    let affected = engine.invalidate_cell(&false_id);
    assert!(!affected.contains(&"A".to_string()));

    let digest = engine.get_root("A").unwrap().digest.clone();
    assert!(engine.validate_root("A", &digest));

    // 2. Modify COND to point to FALSE.
    // This requires re-executing A.
    engine.set_cell(cond_id.clone(), Revision::memory(epoch, 2));
    let affected = engine.invalidate_cell(&cond_id);
    assert!(affected.contains(&"A".to_string()));

    // Recompute A -> Now depends on FALSE
    let mut trace_a_2 = Trace::new();
    trace_a_2.record(cond_id.clone(), Revision::memory(epoch, 2));
    trace_a_2.record(false_id.clone(), Revision::memory(epoch, 2)); // Using the new FALSE rev
    engine.register_root("A".to_string(), trace_a_2, json!("val_a_2"), true);

    // 3. Modify TRUE. A should NOT be invalid (now switched to FALSE).
    engine.set_cell(true_id.clone(), Revision::memory(epoch, 2));
    let affected = engine.invalidate_cell(&true_id);
    assert!(!affected.contains(&"A".to_string()));
}
