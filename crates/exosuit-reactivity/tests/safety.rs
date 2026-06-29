use exosuit_reactivity::types::SafeFloat;
use exosuit_reactivity::{Availability, Reason, Runtime};
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn test_nan_safety() {
    let nan1 = SafeFloat(f64::NAN);
    let nan2 = SafeFloat(f64::NAN);
    let num = SafeFloat(1.0);

    assert_eq!(nan1, nan2, "NaN should equal NaN");
    assert_ne!(nan1, num, "NaN should not equal 1.0");

    // Test Hashing
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(nan1);
    assert!(set.contains(&nan2), "HashSet should treat NaNs as same key");
}

#[tokio::test(start_paused = true)]
async fn test_hysteresis_logic() {
    let runtime = Runtime::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    runtime.set_invalidation_sender(tx);

    let cell = runtime.cell("source", json!(1));

    let computed = runtime
        .computed("comp", || {
            let val = cell.get();
            match val {
                Availability::Present(v) => {
                    if v.as_i64().unwrap() == -1 {
                        Availability::Absent(Reason::Loading)
                    } else {
                        Availability::Present(v)
                    }
                }
                _ => val,
            }
        })
        .with_hysteresis(Duration::from_millis(50));

    // 1. Initial Value
    assert_eq!(computed.read(), Availability::Present(json!(1)));

    // 2. Update to "Loading" (simulated by -1)
    runtime.action(|| {
        cell.set(json!(-1));
    });

    // 3. Should return Stale Value (1) due to Hysteresis
    assert_eq!(
        computed.read(),
        Availability::Present(json!(1)),
        "Should return stale value"
    );

    // 4. Wait for invalidation signal (Timer should fire)
    // Advance time past the hysteresis window
    tokio::time::advance(Duration::from_millis(60)).await;

    let msg = rx.recv().await;
    assert_eq!(msg, Some("comp".to_string()));

    // 5. Should return Absent now that time has passed
    let res = computed.read();
    assert!(
        matches!(res, Availability::Absent(Reason::Loading)),
        "Should return Absent after grace period"
    );
}

#[test]
fn test_corrupted_error_api() {
    use exosuit_reactivity::engine::FetchError;

    let err = FetchError::Corrupted;
    assert_eq!(
        format!("{}", err),
        "Corrupted: Snapshot metadata exists but content is missing"
    );
}

#[tokio::test]
async fn test_async_drop_defer_cleanup() {
    let runtime = Runtime::new();
    let flag = Arc::new(Mutex::new(false));
    let flag_clone = flag.clone();

    runtime.defer_cleanup(async move {
        let mut lock = flag_clone.lock().unwrap();
        *lock = true;
    });

    // Give tokio a moment to run the spawned task
    tokio::time::sleep(Duration::from_millis(10)).await;

    assert!(*flag.lock().unwrap(), "Cleanup task should have run");
}
