//! Integration tests for parsing 'goals' field as alias for 'tasks' in plan.toml
//!
//! Per RFC 00177 (Goals and Tasks unified model), the plan parser should accept
//! either 'tasks' (legacy) or 'goals' (new, preferred) under phases.

use exo::context::ExoState;

#[test]
fn parse_goals_as_tasks_alias() {
    // A plan using 'goals' instead of 'tasks'
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
label = "First Goal"
status = "pending"

[[epochs.phases.goals]]
id = "goal-2"
label = "Second Goal"
status = "pending"
"#;

    let state: ExoState = toml::from_str(plan_toml).expect("Should parse 'goals' field");

    assert_eq!(state.epochs.len(), 1);
    let phase = &state.epochs[0].phases[0];
    assert_eq!(phase.goals.len(), 2);
    assert_eq!(phase.goals[0].id, "goal-1");
    assert_eq!(phase.goals[0].label, "First Goal");
    assert_eq!(phase.goals[1].id, "goal-2");
    assert_eq!(phase.goals[1].label, "Second Goal");
}

#[test]
fn parse_tasks_field_still_works() {
    // Legacy plan using 'tasks'
    let plan_toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test Epoch"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Test Phase"
status = "pending"

[[epochs.phases.tasks]]
id = "task-1"
label = "First Task"
status = "pending"
"#;

    let state: ExoState = toml::from_str(plan_toml).expect("Should parse 'tasks' field");

    assert_eq!(state.epochs.len(), 1);
    let phase = &state.epochs[0].phases[0];
    assert_eq!(phase.goals.len(), 1);
    assert_eq!(phase.goals[0].id, "task-1");
}

#[test]
fn goals_takes_precedence_over_tasks() {
    // If both 'goals' and 'tasks' are present, 'goals' takes precedence
    let plan_toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test Epoch"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Test Phase"
status = "pending"
tasks = ["legacy-task"]

[[epochs.phases.goals]]
id = "goal-1"
label = "Primary Goal"
status = "pending"
"#;

    let state: ExoState = toml::from_str(plan_toml).expect("Should parse with both fields");

    let phase = &state.epochs[0].phases[0];
    // goals takes precedence
    assert_eq!(phase.goals.len(), 1);
    assert_eq!(phase.goals[0].id, "goal-1");
    assert_eq!(phase.goals[0].label, "Primary Goal");
}

#[test]
fn goals_with_label_shorthand() {
    // Goals can also be a simple list of labels (like legacy tasks)
    let plan_toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test Epoch"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Test Phase"
status = "pending"
goals = ["Goal One", "Goal Two", "Goal Three"]
"#;

    let state: ExoState = toml::from_str(plan_toml).expect("Should parse goals shorthand");

    let phase = &state.epochs[0].phases[0];
    assert_eq!(phase.goals.len(), 3);
    // Shorthand creates slugified IDs
    assert_eq!(phase.goals[0].label, "Goal One");
    assert_eq!(phase.goals[1].label, "Goal Two");
    assert_eq!(phase.goals[2].label, "Goal Three");
}

#[test]
fn empty_goals_produces_empty_tasks() {
    let plan_toml = r#"
[[epochs]]
id = "epoch-1"
title = "Test Epoch"
status = "active"

[[epochs.phases]]
id = "phase-1"
title = "Test Phase"
status = "pending"
"#;

    let state: ExoState = toml::from_str(plan_toml).expect("Should parse without goals/tasks");

    let phase = &state.epochs[0].phases[0];
    assert!(phase.goals.is_empty());
}
