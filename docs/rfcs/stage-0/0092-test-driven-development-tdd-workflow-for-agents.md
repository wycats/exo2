<!-- exo:92 ulid:01kg5kp2fj5stbjv30rv82t464 -->

# RFC 92: Test-Driven Development (TDD) Workflow for Agents


# RFC 0092: Test-Driven Development (TDD) Workflow for Agents

## Summary

Integrate Test-Driven Development (TDD) into the Exosuit agent workflow to ensure "Grounding" and reduce hallucinations. TDD operates at the **task level** — it is an annotation on individual tasks that involve code changes, not a separate entity or a goal-level concern.

## Motivation

Agents often "hallucinate" implementations that look correct but don't work. By forcing the agent to write a failing test first (Red), we ensure:

1. **Grounding**: The agent understands the specific scenario.
2. **Verification**: We have an executable proof of the fix/feature.
3. **Stability**: Refactoring is safe because tests guard behavior.

## Proposal

### 1. The Protocol

TDD follows the classic Red/Green cycle, applied per-task:

1. **Red (The Spec)**: Before writing implementation code, write a failing test that asserts the desired behavior or reproduces the bug.
   - *Constraint*: The test must fail for the right reason.
2. **Green (The Solution)**: Write the minimal code required to make the test pass. Refactoring happens here — clean up while the test stays green.

The state machine is: `red → green → (task completion)`. There is no separate "refactor" state — refactoring is part of the green phase, validated by the same test.

When TDD confirms green, the task is *verified* but not yet *complete*. The agent completes the task via `exo task complete` with a log that references the TDD verification. This keeps TDD and the completion cascade as orthogonal concerns that compose naturally.

### 2. Task-Level TDD Annotation

TDD annotates **existing tasks** in `implementation-plan.toml` rather than creating new entities. When an agent starts a TDD cycle for a task, the task gains `tdd_status` and `tests` fields:

```toml
[[plan.goals.tasks]]
id = "implement-parser"
title = "Implement TOML parser"
type = "feat"                                   # feat | fix | refactor | chore | docs | design
status = "pending"                              # Task lifecycle (orthogonal)
tdd_status = "red"                              # TDD state: red | green
tests = ["crates/parser/tests/toml_parse.rs"]   # Test file(s) for this task
no_test_reason = ""                             # Required if tests empty for code types
completion_log = ""                             # Filled on task completion
```

**Key design choices**:

- `tdd_status` and `status` are **orthogonal**. The task lifecycle (`pending → completed`) is about *work tracking*. TDD status (`red → green`) is about *verification*. A task can be in `tdd_status = "green"` while `status = "pending"` — the code is verified but the agent hasn't logged completion yet.
- `tdd_status` is **retained** after task completion. It remains as evidence that the task was TDD-verified, which is useful for reporting and narrative generation. Clearing it would lose information.
- Only **one task** should be in `tdd_status = "red"` at a time. Steering prioritizes the active red task and blocks starting new TDD cycles until the current one resolves.

### 3. Task Types and When TDD Applies

Each task has a `type` field that determines how steering treats it. The type is the **authoritative signal** for TDD enforcement — not heuristics, not inference.

**Code types** — TDD steered hard:
- `feat`: New functionality. TDD required.
- `fix`: Bug fix. TDD required.
- `refactor`: Restructuring existing code. TDD required.

**Non-code types** — TDD ignored entirely:
- `chore`: Maintenance, cleanup, config. No TDD.
- `docs`: Documentation. No TDD.
- `design`: Planning, design work, research. No TDD.

For code-type tasks, steering suggests `exo tdd start <task-id>` before implementation. The agent writes a failing test first, confirms red, implements, confirms green.

For non-code-type tasks, steering skips TDD entirely and suggests direct work or completion. No `tdd_status` field is needed.

If `type` is absent, steering defaults to suggesting TDD (err on the side of verification). Setting the type explicitly is preferred — it gives steering solid ground to stand on.

**Escape hatch**: If a code-type task genuinely cannot be tested, the agent sets `no_test_reason` on the task. This is an escape hatch, not a default.

### 4. Criteria for "No Test" Reasons

To prevent abuse of the escape hatch, the `no_test_reason` must meet these criteria:

**Valid Reasons**:
- "Pure documentation change (README, comments)."
- "Type-system only refactor (verified by `cargo check` / `tsc`)."
- "UI Visual Polish (verified by manual screenshot/preview, no logic change)."
- "Experimental/Spike code (explicitly marked as throwaway)."

**Invalid Reasons** (The "Lazy" List):
- "It's a simple change." (Simple changes break things too.)
- "I'll add tests later." (You won't.)
- "Mocking is too hard." (Refactor the code to be testable.)
- "It works on my machine." (Irrelevant.)

### 5. Integration with the Completion Cascade

TDD feeds into the completion cascade naturally:

1. **Task level**: `exo tdd green` confirms the test passes → `tdd_status = "green"`. The agent then completes the task with `exo task complete <id> --log "..."`, referencing what was tested and verified.
2. **Goal level**: When all tasks for a goal are complete, steering nudges the agent to write a goal completion log. The task logs (including TDD verification evidence) provide material for this narrative.
3. **Phase level**: When all goals have completion logs, steering nudges phase completion. The goal narratives aggregate TDD coverage.

The cascade flows: **task.tdd_status → task.completion_log → goal.completion_log → phase narrative**

TDD doesn't short-circuit any cascade level — it enriches the task completion log with verification evidence, which then feeds upward organically.

### 6. Steering Integration

Steering suggests TDD actions based on task state:

| Task State | Steering Action |
|---|---|
| Code type (`feat`/`fix`/`refactor`), no `tdd_status` | Suggest `exo tdd start <task-id>` (high confidence) |
| `tdd_status = "red"` | Suggest `exo tdd green` — "Confirm tests pass" |
| `tdd_status = "green"` | Suggest `exo task complete <id>` — "Log completion" |
| Non-code type (`chore`/`docs`/`design`) | Skip TDD, suggest direct work or completion |
| No `type` set | Default to suggesting TDD (assume code until told otherwise) |

Chore phases suppress TDD nudges entirely (lighter ceremony). Regular phases steer hard toward test-first for code-type tasks.

### 7. Test Runner Infrastructure

The TDD workflow includes a language-aware test runner that can execute tests for verification:

- **Rust**: Infers `cargo test --test <stem>` from `.rs` files
- **TypeScript/JavaScript**: Runs `vitest run <file>` with package-aware CWD detection
- **Configurable runners**: `exosuit.toml` supports custom runner definitions with glob patterns, command templates, and CWD strategies

```toml
# exosuit.toml
[[tdd.runners]]
glob = "*.test.ts"
command = "vitest run {relative_file}"
cwd = "package_root"
```

The runner infrastructure is decoupled from the TDD state machine — it handles test execution mechanics while the state machine handles workflow tracking.

## Amendment History

- **Original (Stage 3)**: Proposed `[[plan.changes]]` schema with per-change TDD. Implementation created `[[plan.goals]]` entries in implementation-plan.toml.
- **Amendment 1**: Reframed TDD as task-level annotation. `[[plan.changes]]` replaced by `tdd_status` + `tests` fields on existing tasks. Added completion cascade integration and steering documentation. Motivated by the goal/task separation (plan.toml goals vs implementation-plan.toml tasks) and the completion cascade design (task logs → goal narrative → phase narrative).

## Migration Note

The current implementation (`tdd.rs`) still operates at the goal level — `start_step()` creates `[[plan.goals]]` entries and steering suggests TDD per-goal. This amendment specifies the target architecture; the implementation must be updated to match:

1. `start_step()` → annotate existing task with `tdd_status` + `tests` instead of creating goal entries
2. `confirm_red()` / `confirm_green()` → look up tasks by `tdd_status` instead of goals by status
3. Steering → suggest TDD per-task (code-change tasks without `tdd_status`) instead of per-goal
4. LM tool descriptions → update from goal terminology to task terminology

Until this migration is complete, the implementation does not fully match this RFC.

