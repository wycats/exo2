<!-- exo:214 ulid:01kmzxefdx4hy1gjgt9vza3m01 -->

# RFC 214: Autofix Check Ordering


# RFC 00214: Autofix Check Ordering

## Summary

When a lane runs in parallel mode with smart scheduling, autofix checks (like `rust-clippy-fix`) run *after* their corresponding verify checks (like `rust-clippy`). This ordering is backwards: if the autofix would have fixed an issue, the verify check fails first—producing redundant, confusing errors.

## Motivation

### Current Behavior

The `coherence` lane (pre-commit) uses `parallel = true` with smart scheduling:

1. **Phase 1 (parallel)**: All non-autofix checks run concurrently
2. **Phase 2 (sequential)**: Autofix checks with `restage = "auto"` run one-by-one

This means `rust-clippy` (strict verify) runs in Phase 1, while `rust-clippy-fix` (autofix) runs in Phase 2.

### The Problem

If there's a machine-applicable clippy warning:
1. `rust-clippy` fails with the warning (Phase 1)
2. `rust-clippy-fix` would have fixed it (Phase 2, but we never get there)
3. User sees a failure that could have been auto-fixed

In the `dev` lane (sequential, no smart scheduling), the checks run in config order: fix first, then verify. This works correctly.

### Why This Matters

- **Wasted developer time**: Users investigate failures that would auto-resolve
- **Inconsistent behavior**: `dev` lane works correctly; `coherence` lane doesn't
- **Confusing UX**: The "smart" scheduling produces worse outcomes than naive sequential

## Open Questions

### Q1: Do we need both `rust-clippy-fix` and `rust-clippy`?

**Current state**: Two separate checks with different commands:
- `rust-clippy-fix`: `cargo clippy --fix --allow-dirty --allow-staged -- -D warnings`
- `rust-clippy`: `cargo clippy -- -D warnings`

**Possible answers**:
- **Yes, both needed**: Fix applies machine-applicable fixes; verify catches non-machine-applicable warnings
- **No, merge them**: A single check could run fix first, then verify (two-phase within one check)
- **Depends on context**: In `dev` lane, fix-then-verify makes sense; in CI, verify-only is sufficient

### Q2: Should autofix checks always run before their verify counterparts?

**Current smart scheduling logic**: Autofix checks are deferred to Phase 2 because they modify files and need sequential execution with restaging.

**The tension**: 
- Autofix checks need sequential execution (can't run in parallel with each other)
- But they should run *before* verify checks that would catch the same issues

**Possible approaches**:
- Invert the phases: sequential autofix first, then parallel verify
- Introduce explicit ordering/dependencies between checks
- Let checks declare "I fix issues that check X would catch"

### Q3: Is this a config issue or a runner scheduling issue?

**Config perspective**: The user listed checks in a specific order (`rust-clippy-fix` before `rust-clippy`). Smart scheduling ignores this order.

**Runner perspective**: Smart scheduling optimizes for parallelism, not semantic correctness. It doesn't know that `rust-clippy-fix` and `rust-clippy` are related.

**Possible answers**:
- **Config issue**: Add explicit dependencies or ordering hints
- **Runner issue**: Change smart scheduling to respect declared order within categories
- **Both**: Runner needs to understand fix/verify relationships; config needs to express them

### Q4: How should "fix then verify" be expressed?

**Current model**: Checks are independent units. `autofix = true` is a property of a check, not a relationship between checks.

**Possible models**:

1. **Explicit dependencies**: `rust-clippy.depends_on = ["rust-clippy-fix"]`

2. **Fix/verify pairs**: 
   ```toml
   [check."rust-clippy"]
   fix_command = "cargo clippy --fix ..."
   verify_command = "cargo clippy ..."
   ```

3. **Workflow-level fix policy** (from domain model review):
   ```toml
   [lane.coherence]
   fix_policy = { restage = "auto", containment = "fail" }
   ```
   Checks declare `fix = true` to opt in; runner handles ordering.

4. **Phase annotations**:
   ```toml
   [check."rust-clippy-fix"]
   phase = "fix"  # Runs in fix phase
   
   [check."rust-clippy"]
   phase = "verify"  # Runs in verify phase
   ```

## Sketched Solutions

### Option A: Invert Smart Scheduling Phases

Change the runner to execute:
1. **Phase 1 (sequential)**: Autofix checks with restaging
2. **Phase 2 (parallel)**: Non-autofix checks

**Pros**: Minimal config changes; fixes the immediate problem
**Cons**: Autofix checks become a serial bottleneck before any parallel work starts

### Option B: Unified Fix/Verify Checks

Merge related fix and verify checks into a single check that runs both phases internally:

```toml
[check."rust-clippy"]
fix_command = "cargo clippy --fix --allow-dirty --allow-staged -- -D warnings"
verify_command = "cargo clippy -- -D warnings"
autofix = true
```

**Pros**: Eliminates ordering problem; cleaner config
**Cons**: Requires runner changes; less flexible for checks that don't have a fix mode

### Option C: Explicit Check Dependencies

Allow checks to declare dependencies:

```toml
[check."rust-clippy"]
after = ["rust-clippy-fix"]
```

**Pros**: Explicit and flexible
**Cons**: Verbose; easy to misconfigure; doesn't solve the parallel/sequential tension

### Option D: Phase-Based Scheduling

Introduce explicit phases that checks can belong to:

```toml
[lane.coherence]
phases = ["fix", "verify"]

[check."rust-clippy-fix"]
phase = "fix"

[check."rust-clippy"]
phase = "verify"
```

**Pros**: Clear mental model; generalizes beyond fix/verify
**Cons**: More complex config; may be overkill for this problem

## Drawbacks

Any solution adds complexity to the hooks system. The current behavior, while suboptimal, is not broken—it just produces redundant failures.

## Future Possibilities

- **Intelligent deduplication**: If `rust-clippy-fix` succeeds, skip `rust-clippy` entirely (they check the same things)
- **Fix-only mode**: A lane mode that only runs autofix checks, for quick iteration
- **Conditional checks**: Skip verify checks if their corresponding fix check made changes

## References

- `.config/exo/hooks.toml` - Current configuration
- `crates/exohook/src/runner/smart_schedule.rs` - Current partitioning logic

