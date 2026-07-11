<!-- exo:151 ulid:01kg5kp2jgfhbdp20b8cpgwh6q -->

# RFC 151: Test Fixture Strategy

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**: Withdrawn by RFC 10180 storage disposition: this proposal depends on retired file-backed phase context or direct editing/protection of legacy docs/agent-context current artifacts.

# RFC 0151: Test Fixture Strategy

## Summary

Establish a consistent test fixture strategy for Exosuit integration tests that prevents accidental interaction with the live workspace and ensures test isolation.

## Problem

Integration tests that use `repo_root()` (pointing to `CARGO_MANIFEST_DIR`) or similar patterns can accidentally interact with the live workspace. This causes:

1. **Recursive test execution** - Tests that invoke `verify.run` execute `scripts/verify-phase.sh`, which runs `cargo test`, causing infinite recursion
2. **Side effects** - Tests may modify live workspace files
3. **Flaky tests** - Results depend on workspace state at test time
4. **Slow tests** - The `machine_channel_coverage_all_operations` test took 4+ minutes due to recursive `cargo test`

## Solution

### Recommended Crate Stack

1. **`tempfile`** (already in use) - Creates temporary directories that auto-cleanup
2. **`assert_fs`** - Provides `TempDir` with builder pattern for creating file trees
3. **`rstest`** - Parameterized test fixtures with `#[fixture]` attribute

### Fixture Pattern

```rust
fn setup_minimal_fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("create tempdir");
    let root = temp.path();

    // Initialize git repo (required for many commands)
    Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .expect("git init");

    // Create minimal directory structure
    std::fs::create_dir_all(root.join("docs/agent-context/current")).unwrap();
    std::fs::create_dir_all(root.join("docs/rfcs/stage-0")).unwrap();

    // Create minimal plan.toml
    std::fs::write(
        root.join("docs/agent-context/plan.toml"),
        r#"surgical_strikes = []
[meta]
schema_version = "1.0.0"
[[epochs]]
id = "test-epoch"
title = "Test Epoch"
status = "pending"
"#,
    ).unwrap();

    temp
}
```

### Anti-Patterns to Avoid

1. **`repo_root()` pointing to live workspace** - Use temp fixtures instead
2. **`CARGO_MANIFEST_DIR` for test data** - Only use for reading static test fixtures
3. **Commands that run `cargo test`** - Mock or skip in test fixtures

### Migration Path

1. Audit tests using `repo_root()` or `CARGO_MANIFEST_DIR`
2. Create shared fixture helpers in `test_support/`
3. Update dangerous tests to use temp fixtures
4. Add CI check to prevent new anti-patterns

## Test Audit Results

### Dangerous (Fixed)

- `machine_channel_coverage.rs` - Now uses `setup_minimal_fixture()`

### Risky (Need Review)

- `update_archives_legacy_axioms.rs` - Uses live repo for context
- `dispatch_parity.rs` - Uses live repo for command dispatch

### Safe

- 44 tests already use `tempfile::tempdir()` properly

## References

- RFC 0135: Machine Channel Parity (Step 14 coverage test)
- `tools/exo/tests/test_support/mod.rs` - Existing test helpers

