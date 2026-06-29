---
description: "Verifies Logic: Consistency of the Graph."
---

# Internal Coherence Check

**Focus**: Logical Consistency (Logic).
**Grimoire Mapping**: `inv` (Inversion), `fuzz` (Fuzz Logic).

## The Prime Directive

**Verify the Graph.**
The system is a graph of connected nodes (Goals, Decisions, Docs, Code). Broken edges are failures.

## Protocol

1.  **Cross-Reference Check**
    - **Plan vs RFCs**: Do current goals and active work from `exo status` / `exo plan review` contradict accepted or rejected RFC direction?
    - **Plan vs Ideas**: Are active goals and tasks reflected in the idea and inbox state exposed by `exo idea list` and `exo inbox list`?

2.  **Link Rot Detection**
    - Scan `docs/` for broken file links.
    - Verify that referenced symbols (classes, functions) in documentation still exist in the codebase.

3.  **State Surface Validation**
    - Verify that documented state sources align with current architecture: SQLite state in `.cache/exo.db`, `exo` CLI queries, machine-channel operations, SQL dumps in `docs/agent-context/*.sql` (git persistence only, not a read interface), and file-based configuration such as `exosuit.toml` and hook configuration.

## Output

- List of logical contradictions.
- List of broken links.
- Fixes for the internal graph.
