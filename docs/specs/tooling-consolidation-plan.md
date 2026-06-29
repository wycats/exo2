# Tooling Consolidation Plan

> **Purpose**: Get our tools under control by addressing friction points systematically.
> **Source**: Analysis of FRICTION.md, related RFCs, and brainstorming documents.
> **Validated**: 2026-01-29 by prepare agent

---

## Validation Summary

| Rating        | Meaning                    |
| ------------- | -------------------------- |
| ✅ Ready      | Can execute immediately    |
| ⚠️ Needs Prep | Some research still needed |
| 🔴 Blocked    | Has unresolved blockers    |

### Key Findings

1. **RFC 0154 doesn't exist as described** - Plan references "canonical vs projection artifacts" but actual RFC 0154 is about steering confidence. Need new RFC or different approach.

2. **Some friction points already resolved** - F014 (`exo-status`) and F016 (`exo-task-complete`) now exist in codebase.

3. **WP-D Task 2 already complete** - `exo map next` exists via `get-map` suggestion in steering.

4. **CommandSpec→LM tool generation works** - `lmtool-spec.ts` already generates from CommandSpec.

5. **Path corrections needed**:
   - CLI code: `tools/exo/src/commands/` (not `tools/exo/src/cmd/`)
   - LM tools: `packages/exosuit-vscode/lmtools/` (not `src/lm-tools/`)

---

## Work Package A: CommandSpec Parity & Tool Reliability

**Goal**: Make CommandSpec the single source of truth and eliminate CLI/LM tool drift.

**Priority**: Highest

**Readiness**: ⚠️ Needs Prep (3/5)

### Tasks

1. **Implement RFC 0135 parity path** ✅ Ready (4/5)
   - Generate LM tool registry from CommandSpec schema artifact
   - Ensure all CLI commands have corresponding LM tool coverage
   - Related: RFC 0132 (CommandSpec as law), RFC 0134 (CLI/LM unification)
   - **Status**: `lmtool-spec.ts` already generates from CommandSpec

2. **Add parity tests (RFC 0062)** ⚠️ Needs Prep (2/5)
   - Create test framework that validates CLI and LM tool schemas match
   - Run as part of CI to prevent drift
   - Related: RFC 0062 (CLI/tool parity testing - Stage 1)
   - **Missing**: No test framework implementation yet

3. **Fix tool-specific reliability gaps** ⚠️ Needs Prep (3/5)
   - ~~`exo-status` missing~~ ✅ Now exists
   - ~~`exo-task-complete` missing~~ ✅ Now exists
   - `cwd` handling in terminal tool (F015) - **External to our control**
   - `apply_patch` delete failures (F019) - **External to our control**
   - Rust test discovery gaps in `runTests` (F018) - **External to our control**
   - `exo-idea` positional args bug - needs investigation

### Dependencies

- CommandSpec coverage for key commands (RFC 0132)

### Success Criteria

- [ ] All CLI commands have LM tool equivalents
- [ ] Parity tests pass in CI
- [ ] Known reliability bugs fixed

---

## Work Package B: Implementation-Plan & Task Coherence

**Goal**: Eliminate task status divergence and impl-step pain.

**Priority**: High

**Readiness**: ⚠️ Needs Prep (3/5)

### Tasks

1. **Align task list and phase status** ⚠️ Needs Prep (3/5)
   - `exo task list` and `exo phase status` currently show different data
   - Either unify the data source or add explicit mismatch reporting
   - **Note**: RFC 0154 does NOT cover this - need new RFC or different approach
   - **Missing**: No RFC exists for canonical vs projection artifacts

2. **Add impl-step LM tools** ⚠️ Partial (3/5)
   - Existing: `exo-impl-add-step`, `exo-impl-update-step`, `exo-impl-complete-step`, `exo-impl-log`
   - Missing: `exo-impl-list`, `exo-impl-show`, `exo-impl-status`
   - Target: `packages/exosuit-vscode/lmtools/`
   - **Requires**: CLI commands first (`tools/exo/src/commands/impl_cmd.rs`)

3. **Clarify naming conventions** ⚠️ Needs Prep (2/5)
   - Resolve `details` vs `description` inconsistency
   - Document naming patterns in manual
   - **Missing**: No specific file refs identified

### Dependencies

- Work Package A (schema parity provides the pattern)

### Success Criteria

- [ ] Single source of truth for task status
- [ ] Agents can manage impl steps without CLI fallback
- [ ] Consistent argument naming across commands

---

## Work Package C: Workflow Bootstrapping & Phase Preparation

**Goal**: Enforce prepare→approve→start workflow and remove placeholder steps.

**Priority**: High

**Readiness**: ⚠️ Needs Prep (2/5)

### Tasks

1. **Implement `exo phase prepare` flow** ⚠️ Needs Prep (2/5)
   - Create command that generates implementation plan scaffold
   - Require at least one step before `phase start` succeeds
   - F024 confirmed: Placeholder "First Step" is real friction
   - **Missing**: Need to locate `init_phase()` code path in `tools/exo/src/commands/`

2. **Refuse start without preparation** ⚠️ Needs Prep (2/5)
   - `exo phase start` should fail with guidance if no steps defined
   - Steering tool should nudge toward preparation
   - **Missing**: No code location specified

3. **Add guidance for preparation** ⚠️ Needs Prep (2/5)
   - LM tool guidance should emphasize prepare→approve→start
   - Contextual hints when starting unprepared phase
   - **Missing**: Underspecified

### Dependencies

- None (can proceed independently)

### Success Criteria

- [ ] Cannot start phase without implementation steps
- [ ] Guidance actively directs toward preparation
- [ ] Placeholder step creation removed

---

## Work Package D: CLI UX Hygiene

**Goal**: Reduce friction from missing commands/aliases and poor discoverability.

**Priority**: Medium

**Readiness**: ✅ Ready (4/5)

### Tasks

1. **Add impl command aliases** ✅ Ready (4/5)
   - `exo impl list` → alias/wrapper for listing impl steps
   - `exo impl show <id>` → show step details
   - `exo impl status` → current step status
   - Target: `tools/exo/src/commands/impl_cmd.rs`
   - F025 confirmed: These commands don't exist

2. **Add map command parity** ✅ Already Done
   - ~~`exo map next` or document the equivalent~~
   - **Status**: `get-map` exists in steering suggestions

3. **Clarify upgrade vs update** ⚠️ Minor (3/5)
   - Document in CLI help
   - Add alias if feasible (`exo upgrade` → `exo update`)
   - **Missing**: Need to verify where help text is defined

4. **Fix pre-push hook exit handling** ⚠️ Needs Prep (3/5)
   - SIGPIPE exit 141 after success blocks pushes (F020)
   - Add proper signal handling
   - **Missing**: Need to locate hook file

### Dependencies

- None (mostly independent fixes)

### Success Criteria

- [ ] Common expected commands exist or have clear aliases
- [ ] Pre-push hook doesn't block on success
- [ ] Help text clarifies command availability

---

## Work Package E: Extension Bridge & Test Environment

**Goal**: VS Code tooling aligns with CLI-managed workflows.

**Priority**: Medium

**Readiness**: ⚠️ Needs Prep (2/5)

### Tasks

1. **Update tool wrapper for agent-context projects** ⚠️ Needs Prep (2/5)
   - Detect when only agent-context TOMLs exist (no `implementation-plan.toml`)
   - Use `exo task list --format json` as fallback
   - Currently fails silently when file missing
   - **Missing**: Underspecified - need to locate relevant extension code

2. **Add test environment preflight checks** ⚠️ Needs Prep (2/5)
   - Check for VS Code test deps (`xvfb`, `libnspr4`)
   - Document system requirements
   - Add to CI setup scripts
   - **Missing**: Underspecified

### Dependencies

- None

### Success Criteria

- [ ] Extension works with minimal project setup
- [ ] Test environment requirements documented and checked

---

## Quick Wins (<1 hour each)

These can be done immediately, independent of work packages:

| ID  | Fix                                               | Effort | Ready | Notes                                                |
| --- | ------------------------------------------------- | ------ | ----- | ---------------------------------------------------- |
| QW1 | Add `exo impl list/show/status` read-only aliases | 30m    | ✅    | Target: `tools/exo/src/commands/impl_cmd.rs`         |
| QW2 | Add `--details` alias for `--description`         | 15m    | ⚠️    | Need to find where `--description` is defined        |
| QW3 | Document `exo update` vs `exo upgrade` in help    | 15m    | ⚠️    | Need to locate help text                             |
| QW4 | Make task/phase status call out mismatches        | 30m    | ⚠️    | Need to find status output code                      |
| QW5 | ~~Add warning when `cwd` can't be honored~~       | N/A    | 🔴    | External - VS Code's terminal tool, not controllable |

---

## RFC Coverage Analysis

### Strong Coverage (Stage 3)

- RFC 0132: CommandSpec as law ✓
- RFC 0134: CLI/LM unification ✓
- RFC 0135: Machine channel from CommandSpec ✓

### Partial Coverage (Stage 1)

- RFC 0062: CLI/tool parity testing (needs implementation)
- RFC 0105: RFC-centric workflow model

### Gaps (No RFC)

- **Canonical vs projection artifacts** - Plan assumed RFC 0154 covered this, but it doesn't
- Impl command parity (CLI aliases for common agent expectations)
- Task status mismatch between views
- Pre-push hook signal handling

### Outdated RFC References

- RFC 0154 is actually "Steering Confidence Model" not "Canonical vs Projection"
- RFC 0101 is Stage 3, not Stage 4
- RFC 0093 is Stage 3, not Stage 4

---

## Suggested Execution Order

### Ready Now

1. **QW1**: Add `exo impl list/show/status` — Target file clear: `tools/exo/src/commands/impl_cmd.rs`
2. **WP-D Task 2**: Already complete — `get-map` exists in steering

### Needs Prep First

3. **WP-C**: Workflow bootstrapping — need to locate `init_phase()` and phase start code paths
4. **WP-A Task 2**: Parity test framework — RFC 0062 exists but implementation undefined
5. **WP-B Task 1**: Task coherence — need new RFC (0154 doesn't cover this)

### External/Blocked

- QW5, WP-A Task 3 (cwd, apply_patch, runTests) — VS Code/external tool issues, not in our control

---

## Blockers Identified

### 🔴 RFC 0154 Mismatch

**Plan assumed**: RFC 0154 covers "canonical vs projection artifacts" and phase preparation  
**Reality**: RFC 0154 is `0154-steering-confidence-model.md` - completely different topic  
**Action needed**: Create new RFC for canonical/projection model, or find different approach

### 🔴 External Tool Issues

Several friction points (F015, F018, F019) are about VS Code's terminal tool, `apply_patch`, and `runTests` — we cannot fix these directly.

### ⚠️ Stale Friction Entries

F014 (`exo-status`) and F016 (`exo-task-complete`) are marked as friction but now exist. FRICTION.md should be updated.
