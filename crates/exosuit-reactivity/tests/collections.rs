use exosuit_reactivity::{Availability, Runtime};
use serde_json::json;

#[test]
fn test_list_addition_triggers_recompute() {
    let runtime = Runtime::new();
    let list_cell = runtime.cell("list", json!([1, 2]));

    let computed = runtime.computed("list_len", || match list_cell.get() {
        Availability::Present(value) => {
            let len = value.as_array().map(|items| items.len()).unwrap_or(0);
            Availability::Present(json!(len))
        }
        Availability::Absent(reason) => Availability::Absent(reason),
    });

    assert_eq!(computed.read(), Availability::Present(json!(2)));

    runtime.action(|| {
        list_cell.set(json!([1, 2, 3]));
    });

    assert_eq!(computed.read(), Availability::Present(json!(3)));
}

#[test]
fn test_list_removal_triggers_recompute() {
    let runtime = Runtime::new();
    let list_cell = runtime.cell("list", json!(["a", "b", "c"]));

    let computed = runtime.computed("list_len", || match list_cell.get() {
        Availability::Present(value) => {
            let len = value.as_array().map(|items| items.len()).unwrap_or(0);
            Availability::Present(json!(len))
        }
        Availability::Absent(reason) => Availability::Absent(reason),
    });

    assert_eq!(computed.read(), Availability::Present(json!(3)));

    runtime.action(|| {
        list_cell.set(json!(["a", "c"]));
    });

    assert_eq!(computed.read(), Availability::Present(json!(2)));
}

#[test]
fn test_deep_mutation_triggers_recompute() {
    let runtime = Runtime::new();
    let list_cell = runtime.cell(
        "list",
        json!([
            {"id": 1, "value": "alpha"},
            {"id": 2, "value": "beta"}
        ]),
    );

    let computed = runtime.computed("first_value", || match list_cell.get() {
        Availability::Present(value) => {
            let first_value = value
                .as_array()
                .and_then(|items| items.first())
                .and_then(|item| item.get("value"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            Availability::Present(first_value)
        }
        Availability::Absent(reason) => Availability::Absent(reason),
    });

    assert_eq!(computed.read(), Availability::Present(json!("alpha")));

    runtime.action(|| {
        list_cell.set(json!([
            {"id": 1, "value": "gamma"},
            {"id": 2, "value": "beta"}
        ]));
    });

    assert_eq!(computed.read(), Availability::Present(json!("gamma")));
}
