<!-- exo:228 ulid:01kmzxey0mbfvnm63pbzkc3vfy -->

# RFC 228: Terminology Normalization: Goal/Task Hierarchy


# RFC 00228: Terminology Normalization: Goal/Task Hierarchy

## Summary

Normalize the codebase terminology to match the actual schema hierarchy:

- **Goals**: High-level objectives in `plan.toml` (under phases)
- **Tasks**: Implementation steps in `implementation-plan.toml` (under goals)

Currently, Rust types use legacy "task" naming for what are actually goals, causing confusion and bugs.

## Motivation

The codebase has a terminology mismatch where:

```rust
// Current (confusing)
pub struct Phase {
    #[serde(rename = "goals")]
    pub tasks: Vec<Task>,  // Task struct actually holds goals!
}
```

This causes:

1. **Active confusion**: Strike overlay reads `phase.tasks` expecting tasks but gets goals
2. **Documentation mismatch**: Comments say "task" but mean "goal"
3. **API confusion**: Functions like `add_task_to_goal()` are misleading

### Correct Terminology

| File                       | Container | Contains                          |
| -------------------------- | --------- | --------------------------------- |
| `plan.toml`                | Phase     | **Goals** (high-level objectives) |
| `implementation-plan.toml` | Goal      | **Tasks** (implementation steps)  |

## Detailed Design

### Phase 1: Internal Type Renames (Safe)

Rename Rust types with serde aliases for backward compat:

```rust
// Before
pub struct Task { ... }  // Actually a goal

// After
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Goal {
    pub id: String,
    pub label: String,
    pub status: String,
    // ...
}
```

```rust
// Before
pub struct Phase {
    #[serde(rename = "goals")]
    pub tasks: Vec<Task>,
}

// After
pub struct Phase {
    #[serde(alias = "tasks")]  // backward compat
    pub goals: Vec<Goal>,
}
```

**Files affected:**

- `tools/exo/src/context.rs` - Task → Goal, Phase.tasks → Phase.goals
- `tools/exo/src/plan.rs` - function renames
- `tools/exo/src/strike.rs` - field access updates
- `tools/exo/src/derived.rs` - DerivedTaskStatus → DerivedGoalStatus

### Phase 2: Function Renames (Safe)

| Current                       | New                           |
| ----------------------------- | ----------------------------- |
| `find_task_in_active_phase()` | `find_goal_in_active_phase()` |
| `add_task_to_goal()`          | `add_goal_to_phase()`         |
| `derive_task_status()`        | `derive_goal_status()`        |
| `upgrade_task_labels()`       | `upgrade_goal_labels()`       |

### Phase 3: CLI Command Aliases (Backward Compat)

Keep old commands as aliases:

```rust
// plan add-task → plan add-goal (new canonical)
// plan add-task remains as alias
```

### Phase 4: JSON Output Aliases

Use serde to maintain JSON output compatibility:

```rust
#[serde(rename = "pending_task_count", alias = "pending_goal_count")]
pub pending_goal_count: usize,
```

### Phase 5: Documentation Updates

Update ~15 doc files to use correct terminology.

## Scope Summary

| Category          | Count  | Risk                 |
| ----------------- | ------ | -------------------- |
| Rust type renames | 4-6    | Low (serde aliases)  |
| Field renames     | 6-8    | Low (serde aliases)  |
| Function renames  | 8-10   | Low (internal)       |
| CLI commands      | 2      | Medium (add aliases) |
| JSON fields       | 4-6    | Medium (add aliases) |
| Docs/templates    | 15+    | Low                  |
| **Total files**   | ~30-40 |                      |

## Migration Strategy

**Incremental with compatibility layer:**

1. ✅ Rename internal types with serde aliases (no breaking change)
2. ✅ Rename functions (no breaking change)
3. ⚠️ Add new CLI commands, keep old as aliases
4. ⚠️ Add JSON output aliases for one release
5. ✅ Update documentation

## Success Criteria

- [ ] `Task` struct renamed to `Goal` in context.rs
- [ ] `Phase.tasks` renamed to `Phase.goals`
- [ ] All function names use correct terminology
- [ ] CLI accepts both old and new command names
- [ ] JSON output maintains backward compatibility
- [ ] Documentation uses consistent terminology
- [ ] No breaking changes to TOML schema

## Alternatives Considered

### Do Nothing

Keep the mismatch. Rejected: causes ongoing confusion and bugs.

### Big-Bang Rename

Rename everything at once without aliases. Rejected: breaks existing workflows.

### Rename Schema Instead

Change TOML to use "tasks" everywhere. Rejected: "goals" is the correct term for plan-level objectives.

