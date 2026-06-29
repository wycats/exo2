use exosuit_reactivity::engine::Engine;
use exosuit_reactivity::revision::{Epoch, Revision};
use exosuit_reactivity::trace::{Trace, TraceDigest};
use exosuit_reactivity::types::CellId;
use std::time::Duration;

#[test]
fn test_recursive_root_invalidation() {
    let mut engine = Engine::new(Duration::from_secs(60));
    let epoch = Epoch::new();

    // 1. Setup Leaf Cell C1
    let c1_id = CellId::root("C1");
    let rev1 = Revision::memory(epoch, 1);
    engine.set_cell(c1_id.clone(), rev1.clone());

    // 2. Setup Middle Root R1 (depends on C1)
    let mut trace_r1 = Trace::new();
    trace_r1.record(c1_id.clone(), rev1.clone());
    let val_r1 = serde_json::json!("R1_v1");
    engine.register_root("R1".to_string(), trace_r1.clone(), val_r1.clone(), true);
    let digest_r1 = trace_r1.digest();

    // 3. Setup Top Root R2 (depends on R1)
    // R2 depends on R1 as a Cell
    let r1_as_cell = CellId::root("R1");
    // The revision of R1 is its Disk revision (hash)
    let rev_r1 = Revision::Disk {
        hash: digest_r1.clone(),
    };

    let mut trace_r2 = Trace::new();
    trace_r2.record(r1_as_cell.clone(), rev_r1.clone());
    let val_r2 = serde_json::json!("R2_v1");
    engine.register_root("R2".to_string(), trace_r2.clone(), val_r2.clone(), true);
    let digest_r2 = trace_r2.digest();

    // 4. Verify R2 is valid initially
    assert!(engine.validate_root("R2", &digest_r2));

    // 5. Update C1 (Invalidates R1, which should invalidate R2)
    let rev2 = Revision::memory(epoch, 2);
    engine.set_cell(c1_id.clone(), rev2);

    // 6. Verify R2 is now invalid
    // validate_root("R2") -> checks R2.trace -> checks R1 revision
    // get_revision("R1") -> checks R1.trace -> checks C1 revision
    // C1 revision mismatch -> R1 invalid -> get_revision("R1") returns None
    // R2.trace sees None -> R2 invalid
    assert!(!engine.validate_root("R2", &digest_r2));
}
