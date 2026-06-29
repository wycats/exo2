<!-- exo:10100 ulid:01kmzxeff5g8veyg0sr3zgbrwn -->


# RFC 10100: Test-Driven Development (TDD) Workflow for Agents

## Summary
Integrate Test-Driven Development (TDD) into the Exosuit agent workflow to ensure "Grounding" and reduce hallucinations.

## Motivation
Agents often "hallucinate" implementations that look correct but don't work. By forcing the agent to write a failing test first (Red), we ensure:
1.  **Grounding**: The agent understands the specific scenario.
2.  **Verification**: We have an executable proof of the fix/feature.
3.  **Stability**: Refactoring (Green/Refactor) is safe.

## Proposal

### 1. The Protocol (AGENTS.md)
Add a mandatory "Test-Driven Implementation" protocol to `AGENTS.md`.

**Draft Content:**
```markdown
### Protocol: Test-Driven Implementation

_Derived from "The Hands"_

1.  **Red (The Spec)**: Before writing implementation code, write a failing test that asserts the desired behavior or reproduces the bug.
    -   *Constraint*: The test must fail for the right reason.
2.  **Green (The Solution)**: Write the minimal code required to make the test pass.
3.  **Refactor (The Cleanup)**: Optimize and clean up the code while ensuring the test remains green.
```

### 2. The Plan (implementation-plan.toml)
Update the `implementation-plan.toml` schema to capture test intent at the task level. This forces the agent to think about *where* the test will live during the planning phase.

**Exception**: If a test is truly impossible or impractical (e.g., pure refactoring of types, documentation only), the agent MUST provide a detailed `no_test_reason`.

**Draft Schema:**
```toml
[[plan.goals]]
id = "goal-id"
title = "Goal Title"

[[plan.goals.tasks]]
id = "task-id"
title = "Task Title"
type = "feat"               # feat, fix, refactor, chore, docs, design
files = ["path/to/source.ts"]
tests = ["path/to/test.ts"] # Required for feat/fix/refactor
# no_test_reason = "..."     # Required if tests is empty for feat/fix/refactor
# tdd_status = "red"         # red | green (absent means not started)
details = "Description"
```

### 3. Task Types & Verification Strictness

To avoid "theater" (writing fake reasons for chores), we categorize tasks:

- **Strict TDD** (`feat`, `fix`, `refactor`):
  - **MUST** provide `tests` OR a valid `no_test_reason`.
  - **Goal**: Logic verification.

- **Relaxed TDD** (`chore`, `docs`, `design`):
  - `tests` are optional.
  - If `tests` are missing, `no_test_reason` is **NOT** required (implicit reason: "It's a chore").
  - **Goal**: Task completion verification (e.g., "Did the build pass?").

### 4. Criteria for "No Test" Reasons (Strict Mode)

To prevent abuse of the escape hatch, the `no_test_reason` must meet the following criteria:

- **Valid Reasons**:
  - "Pure documentation change (README, comments)."
  - "Type-system only refactor (verified by `cargo check` / `tsc`)."
  - "UI Visual Polish (verified by manual screenshot/preview, no logic change)."
  - "Experimental/Spike code (explicitly marked as throwaway)."

- **Invalid Reasons** (The "Lazy" List):
  - "It's a simple change." (Simple changes break things too).
  - "I'll add tests later." (You won't).
  - "Mocking is too hard." (Refactor the code to be testable).
  - "It works on my machine." (Irrelevant).

### 4. The Prompt (phase-start)
Update the `phase-start` prompt to enforce that the plan includes these test files.

**Draft Instruction:**
- "For each proposed change, identify the **Test File** that will verify it. If one doesn't exist, plan to create it."
