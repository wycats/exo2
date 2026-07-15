<!-- exo:10124 ulid:01kmzxey1ejpjjg9qznbbjrfgj -->


# RFC 10124: E2E Verified Dashboard Behavior

- **Superseded by**: RFC 0074


## Summary
Define the canonical behavior of the Exosuit Dashboard (V2) and establish the E2E test suite as the primary verification mechanism for these behaviors.

## Motivation
We have fixed the "API Acquired" bug (Phase 44) and established E2E infrastructure (Phase 45). Now we need to rigorously define *what* the Dashboard does and verify it automatically to prevent regressions.

## Specification (The "What")

### 1. Initial Load
- **Given**: The extension is active.
- **When**: The user runs "Exosuit: Show Context".
- **Then**:
    - The Dashboard Webview opens.
    - It displays the "Current Phase" section.
    - It displays the "RFCs" section.
    - It displays the "Feedback" section.

### 2. Reactivity (The "Magic")
- **Given**: The Dashboard and relevant Tree Views are open.
- **When**: The underlying `plan.toml` is modified using the `exo` CLI (e.g., `exo phase start`).
- **Then**: 
    - The Dashboard UI updates instantly.
    - The "Phase Details" Tree View updates instantly.
    - The "Project Plan" Tree View updates instantly.
    - **Constraint**: The test MUST use the `exo` CLI to perform the mutation, ensuring the toolchain is complete.

### 3. Navigation
- **Given**: The Dashboard is open.
- **When**: The user clicks an RFC item.
- **Then**: The corresponding markdown file opens in a new editor column.

## Verification Plan (The "How")
We will implement `tests/e2e/dashboard-behavior.test.ts` using the Phase 45 infrastructure.
- **Tooling**: We will ensure the `exo` CLI is available in the test environment.
- **Scope**: We will verify updates across multiple views (Dashboard + Tree Views) to ensure global consistency.
