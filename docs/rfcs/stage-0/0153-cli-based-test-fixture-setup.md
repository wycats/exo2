<!-- exo:153 ulid:01kg5kp2jkhvg7gf75ga2gnms3 -->

# RFC 153: CLI-Based Test Fixture Setup


# RFC 0153: CLI-Based Test Fixture Setup

## Problem Statement

Our integration tests currently build fixtures by manually writing TOML files using `fs::write()`. This approach is:

1. **Fragile**: Manual TOML construction is error-prone and doesn't validate against the actual schema
2. **Drifts from Reality**: When CLI commands evolve, manual fixtures may represent states that are impossible to reach via the CLI
3. **Duplicative**: The same fixture patterns are repeated across many test files
4. **Hard to Maintain**: Schema changes require updating fixtures across all test files

Recent debugging of `phase_status_derived_satisfies` revealed this problem: the test's manually-constructed `implementation-plan.toml` had embedded tasks (`[[plan.goals.tasks]]`) that caused the `satisfies` link derivation logic to be bypassed. This bug would not have occurred if the fixture had been built using CLI commands.

## Proposed Solution

Replace manual TOML fixture construction with CLI command sequences that build the same state. This ensures:

1. **Validity**: Fixtures always represent reachable states
2. **Self-Documentation**: The test setup reads like a user workflow
3. **Resilience**: Schema changes automatically propagate through CLI commands
4. **Consistency**: One pattern for all tests

### CLI Fixture Pattern

```rust
// Initialize workspace
let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
cmd.current_dir(root).arg("init").assert().success();

// Add epoch
let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
cmd.current_dir(root)
    .args(["plan", "add-epoch", "epoch-1", "--title", "Epoch 1"])
    .assert().success();

// Add phase
let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
cmd.current_dir(root)
    .args(["plan", "add-phase", "epoch-1", "phase-1", "--title", "Phase 1"])
    .assert().success();

// Start phase
let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
cmd.current_dir(root)
    .args(["phase", "start", "phase-1"])
    .assert().success();

// Add implementation step
let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
cmd.current_dir(root)
    .args(["impl", "add-step", "Step Name", "--details", "Details"])
    .assert().success();
```

### Helper Module

Create a `test_support::fixtures` module with builder helpers:

```rust
pub struct FixtureBuilder<'a> {
    root: &'a Path,
}

impl<'a> FixtureBuilder<'a> {
    pub fn new(root: &'a Path) -> Self { Self { root } }

    pub fn init(&self) -> &Self {
        let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("exo");
        cmd.current_dir(self.root).arg("init").assert().success();
        self
    }

    pub fn add_epoch(&self, id: &str, title: &str) -> &Self { ... }
    pub fn add_phase(&self, epoch: &str, id: &str, title: &str) -> &Self { ... }
    pub fn start_phase(&self, id: &str) -> &Self { ... }
    pub fn add_step(&self, name: &str, details: &str) -> &Self { ... }
    // etc.
}
```

## Audit: Files Requiring Migration

### Already Migrated (using CLI helpers)

| File                                | Status                                                 |
| ----------------------------------- | ------------------------------------------------------ |
| `world_state.rs`                    | ✓ Uses `exo_init`, `exo_plan_add_epoch`, etc.          |
| `repro_phase_id_collision.rs`       | ✓ Uses `exo_init` and CLI commands                     |
| `repro_task_status_mismatch.rs`     | ✓ Uses `exo_init`, `exo_impl_add_step`, etc.           |
| `impl_add_task.rs`                  | ✓ Uses `exo_init`, `exo_impl_add_step`                 |
| `impl_add_step_duplicate.rs`        | ✓ Uses CLI helpers                                     |
| `state_machine_tests.rs`            | ✓ Uses CLI helpers (except deprecated projection test) |
| `structured_io_format_json.rs`      | ✓ Uses `exo_init`                                      |
| `rfc_edit.rs`                       | ✓ Migrated to `exo_rfc_create`                         |
| `rfc_edit_preserves_frontmatter.rs` | ✓ Migrated to `exo_rfc_create`                         |
| `rfc_promote_path.rs`               | ✓ Migrated to `exo_rfc_create`                         |

### Valid Exceptions (require manual fixtures)

| File                                     | Reason                                                                        |
| ---------------------------------------- | ----------------------------------------------------------------------------- |
| `rfc_rename.rs`                          | Tests title/filename mismatch - a legacy state that cannot be created via CLI |
| `update_archives_legacy_axioms.rs`       | Tests legacy format migration from pre-v1 format                              |
| `update_migrates_tool_presentation.rs`   | Tests legacy format migration                                                 |
| `update_refreshes_task_list_snapshot.rs` | Tests deprecated projection handling                                          |
| `upgrade_gate.rs`                        | Tests legacy format detection and upgrade blocking                            |
| `upgrade_integration.rs`                 | Tests full upgrade path from pre-v1 to v1.0                                   |
| `state_machine_tests.rs` (partial)       | `add_deprecated_projection()` creates legacy task-list.toml                   |
| `machine_channel_feedback.rs`            | Creates empty `feedback.toml` - no CLI init for this yet                      |
| `machine_channel_feedback_mutations.rs`  | Creates empty `feedback.toml`                                                 |
| `machine_channel_coverage.rs`            | Creates support files (feedback.toml, verify script)                          |
| `machine_channel_docs_links.rs`          | Creates markdown test input files                                             |
| `cases.rs`                               | Tests edge cases (no active phase, completed phases, verify scripts, etc.)    |
| `init.rs`                                | Tests non-empty directory behavior                                            |

## Implementation Plan

1. ~~Create `test_support/fixtures.rs` with `FixtureBuilder` helper~~ (Already exists in `test_support/mod.rs`)
2. ~~Migrate tests in dependency order (simpler tests first)~~ (Most already migrated)
3. Document special cases that retain manual fixtures (Done above)
4. Ensure all 503+ tests pass after migration

## Success Criteria

- [x] Helper functions exist in `test_support/mod.rs`
- [x] All applicable tests use CLI commands for fixture setup
- [x] Legacy/upgrade tests are documented as exceptions
- [ ] All tests pass (verify with `cargo test -p exo`)

