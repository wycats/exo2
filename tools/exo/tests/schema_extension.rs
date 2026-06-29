use chrono::{DateTime, Utc};
use exo::context::ExoState;

#[test]
fn parse_goal_with_strike_fields() {
    // Note: "aborted" in TOML is normalized to "abandoned" on read
    let plan_toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test Epoch"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Test Phase"
status = "pending"

[[epochs.phases.goals]]
id = "goal-1"
label = "Strike Goal"
status = "aborted"
kind = "strike"
started_at = "2026-01-30T12:34:56Z"
description = "Finish the strike"
"#;

    let state: ExoState = toml::from_str(plan_toml).expect("Should parse strike fields");

    let goal = &state.epochs[0].phases[0].goals[0];
    assert_eq!(goal.kind.as_deref(), Some("strike"));
    assert_eq!(goal.description.as_deref(), Some("Finish the strike"));
    // "aborted" is normalized to "abandoned" on read
    assert_eq!(goal.status, "abandoned");

    let expected = DateTime::parse_from_rfc3339("2026-01-30T12:34:56Z")
        .unwrap()
        .with_timezone(&Utc);
    assert_eq!(goal.started_at, Some(expected));
}

#[test]
fn parse_goal_without_new_fields_is_backward_compatible() {
    let plan_toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test Epoch"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Test Phase"
status = "pending"

[[epochs.phases.goals]]
id = "goal-1"
label = "Legacy Goal"
status = "pending"
"#;

    let state: ExoState = toml::from_str(plan_toml).expect("Should parse legacy goals");

    let goal = &state.epochs[0].phases[0].goals[0];
    assert!(goal.kind.is_none());
    assert!(goal.started_at.is_none());
    assert!(goal.description.is_none());
    assert_eq!(goal.kind.as_deref().unwrap_or("regular"), "regular");
}

#[test]
fn serialize_goal_with_strike_fields() {
    let plan_toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test Epoch"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Test Phase"
status = "pending"

[[epochs.phases.goals]]
id = "goal-1"
label = "Strike Goal"
status = "in-progress"
kind = "strike"
started_at = "2026-01-30T12:34:56Z"
description = "Finish the strike"
"#;

    let state: ExoState = toml::from_str(plan_toml).expect("Should parse goals");
    let output = toml::to_string_pretty(&state).expect("Should serialize plan");

    assert!(output.contains("kind = \"strike\""));
    assert!(output.contains("started_at = \"2026-01-30T12:34:56Z\""));
    assert!(output.contains("description = \"Finish the strike\""));
}
