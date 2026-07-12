<!-- exo:75 ulid:01kg5m2ye892ch29s4dtbtb8ca -->

# RFC 75: Distinguishing Spec vs. Work RFCs

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 0075: Distinguishing Spec vs. Work RFCs

## Summary
Introduce a distinct numbering scheme (`W-XXXX`) and lifecycle for "Work" RFCs to separate ephemeral implementation plans from permanent architectural specifications (`S-XXXX` or just `XXXX`).

## Motivation
Currently, all RFCs are treated equally in the `docs/rfcs/` directory. This creates noise:
1.  **History Pollution**: Future readers (human or agent) have to wade through implementation details of past phases to find the "Laws" of the system.
2.  **Lifecycle Confusion**: "Spec" RFCs should evolve into the Manual. "Work" RFCs should be archived once the work is done.

## Proposal

### 1. Two Types of RFCs

#### Spec RFCs (`XXXX` or `S-XXXX`)
- **Purpose**: Define the "What" and "Why". Changes the "Laws" (Architecture, API, Behavior).
- **Lifecycle**: Stage 0 -> Stage 4 (Stable).
- **Persistence**: Content is merged into `docs/manual/` or `docs/specs/`. The RFC remains as a historical record of the *decision*.

#### Work RFCs (`W-XXXX`)
- **Purpose**: Define the "How". Describes a specific migration, refactor, or test plan.
- **Lifecycle**: Stage 0 -> Stage 3 (Implemented) -> Archived.
- **Persistence**: Once the work is verified, the RFC is moved to `docs/rfcs/archive/`. It does not need to be maintained or cited in the Manual.

### 2. Numbering Scheme
- **Specs**: Continue using sequential integers (`0033`, `0034`...).
- **Work**: Use `W-` prefix + sequential integers (`W-0001`, `W-0002`...).

### 3. Dashboard Integration
- The Dashboard should group these separately:
    - **Specs**: "Proposed Laws"
    - **Work**: "Active Work Orders"

## Transition Plan
1.  Update `AGENTS.md` to reflect this distinction.
2.  Update the `exo` CLI (if necessary) to handle the `W-` prefix.
3.  Update the Dashboard to render them distinctly.
