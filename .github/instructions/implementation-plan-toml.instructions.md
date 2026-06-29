---
applyTo: "**/docs/agent-context/current/implementation-plan.toml"
---

# Implementation Plan

The implementation plan is the tactical view of the active phase—goals and their tasks, plus status and execution details.

**Workflow:**

- View current phase details with `exo-phase`
- Add goals with `exo-run("goal add ...")`
- Add tasks with `exo-run("task add --goal <id> ...")`
- Mark completion with `exo-run("task complete ...")`

Goals contain tasks. Both maintain synchronized state with the broader plan through these tools.
