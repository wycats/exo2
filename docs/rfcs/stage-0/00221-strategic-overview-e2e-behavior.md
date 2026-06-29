<!-- exo:221 ulid:01kmzxefe349h50yb381ydb6rb -->

# RFC 221: Strategic Overview E2E Behavior


# RFC 00221: Strategic Overview E2E Behavior

## Summary
Define the canonical behavior of the Strategic Overview (Dashboard replacement) and establish the E2E test suite as the primary verification mechanism for these behaviors.

## Motivation
The Strategic Overview is the primary orientation surface for between-* modes. We need to lock in its behavior and verify it across mode transitions, navigation, and reactivity to prevent regressions.

## Specification (The "What")

### 1. Initial Load
- **Given**: The extension is active.
- **When**: The user runs "Exosuit: Show Strategic Overview".
- **Then**:
    - The Strategic Overview webview opens.
    - It displays the current progress mode header.
    - It displays the roadmap/epoch summary section.
    - It displays the phase selection or overview list.

### 2. Mode Transitions
- **Given**: The Strategic Overview is open.
- **When**: The progress mode changes to a between-* mode via the `exo` CLI.
- **Then**:
    - The Strategic Overview updates to match the new mode.
    - The header text and available actions update accordingly.

### 3. Navigation
- **Given**: The Strategic Overview is open.
- **When**: The user selects a phase or epoch item.
- **Then**: The corresponding markdown or detail view opens in a new editor column.

### 4. Reactivity
- **Given**: The Strategic Overview is open.
- **When**: Underlying `plan.toml` or `implementation-plan.toml` data changes via the `exo` CLI.
- **Then**:
    - The Strategic Overview updates instantly.
    - The "Project Plan" Tree View updates instantly.
    - **Constraint**: The test MUST use the `exo` CLI to perform the mutation, ensuring the toolchain is complete.

## Verification Plan (The "How")
We will implement `tests/e2e/strategic-overview-behavior.test.ts` using the existing E2E infrastructure.
- **Tooling**: Ensure the `exo` CLI is available in the test environment.
- **Scope**: Verify updates across the Strategic Overview and relevant Tree Views to ensure global consistency.

