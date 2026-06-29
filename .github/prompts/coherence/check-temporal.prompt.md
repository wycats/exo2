---
description: "Synchronizes Time: Plan vs Manual vs Code."
---

# Temporal Coherence Check

**Focus**: Synchronization of State (Time).
**Grimoire Mapping**: `var` (Invariants), `sci` (Scientific Method).

## The Prime Directive

**Code is Reality.**
If the Manual disagrees with the Code, the Manual is hallucinating.
If the Plan disagrees with the Code, the Plan is lagging.

## Protocol

1.  **Reality Check (Read the Code)**
    - Scan `src/` to determine what _actually_ exists and works.
    - Identify features that are implemented but not documented.
    - Identify features that are documented but not implemented.

2.  **Manual Audit (Update the Docs)**
    - **Hallucination Removal**: Delete or mark as "Planned" any feature in `docs/manual/` that does not exist in `src/`.
    - **Amnesia Cure**: Document any feature in `src/` that is missing from `docs/manual/`.

3.  **Plan Audit (Update the Plan)**
    - **Mark Completed**: If a goal reported by `exo status`, `exo plan review`, or `exo phase status` is implemented, update the underlying SQLite-backed state with the appropriate `exo` command.
    - **Resurrect Pending**: If state claims work is complete but the code is missing or broken, reopen or correct it with the appropriate `exo` command.

## Output

- A list of discrepancies found (Hallucinations, Amnesia, Lag).
- A set of file edits to synchronize the artifacts.
