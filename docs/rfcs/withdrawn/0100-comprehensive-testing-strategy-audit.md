<!-- exo:100 ulid:01kg5kp2fzpr70krmfrn3vbkbp -->

# RFC 100: Comprehensive Testing Strategy & Audit

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 0100: Comprehensive Testing Strategy & Audit

## Summary

This RFC formalizes the findings of the recent test suite audit and defines the testing required for the next phase of development. It mandates the removal of obsolete tests (Walkthroughs, Permissions) and mandates specific coverage for new features (Reactive Collections, Shared Agent Runtime, Externalized Prompts).

## Motivation

Our codebase has accumulated technical debt in the form of tests for features that are no longer relevant (e.g., legacy UI walkthroughs). Simultaneously, new core architectural components (Reactive Collections) lack sufficient test coverage.

To support the transition to the new phase, we must:
1.  Eliminate noise by deleting dead tests.
2.  Lock in behavior of new core primitives with high-quality tests.
3.  Ensure the test suite reflects the *current* architectural reality (RFC 0118, RFC 0012).

## Detailed Design

### 1. Deletions (Legacy Debt)

The following test files target features that have been superseded or removed from the product requirement. They must be deleted.

#### Legacy Walkthroughs
The "Walkthrough" UI feature has been replaced by the new Onboarding flow and Phase system.
- `tests/e2e/walkthrough-view.test.ts`
- `tests/e2e/walkthrough-empty.test.ts`
- `tests/e2e/walkthrough-entries.test.ts`
- `tools/exo/tests/walkthrough_remove.rs`

#### Legacy Permissions
The old permission system (read-only flags) has been replaced by RFC 0111 (Capability Security).
- `tools/exo/tests/update_repairs_rfc_permissions.rs`

### 2. New Coverage Requirements

The following areas require new or expanded test suites.

#### Reactive Collections (RFC 0118)
We need to verify that `exosuit-reactivity` correctly handles collection updates.
- **Scope**: Rust unit tests in `crates/exosuit-reactivity`.
- **Scenarios**:
  - Adding items to a tracked list triggers subscribers.
  - Removing items triggers subscribers.
  - Deep mutation of items (if supported) triggers subscribers.

#### Shared Agent Runtime (RFC 0121)
We need to verify the Agent Runtime correctly manages state across boundaries.
- **Scope**: Integration tests.

#### Externalized Prompts (RFC 0012)
Verify that prompts are correctly loaded from the file system and interpolated.
- **Scope**: TypeScript integration or Rust core tests (depending on implementation location).
- **Scenarios**:
  - Loading a prompt by ID.
  - Interpolating variables into the prompt.
  - Failures on missing prompt files.

## Implementation Plan

- [ ] **Delete Legacy Tests**: Remove the identified files.
- [ ] **Scaffold Reactive Tests**: Create `crates/exosuit-reactivity/tests/collections.rs`.
- [ ] **Scaffold Prompt Tests**: Create new test file for prompt loading.
- [ ] **Verify CI**: Ensure `pnpm test` and `cargo test` pass after deletions.

