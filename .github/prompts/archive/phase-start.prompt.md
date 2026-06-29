---
agent: agent
description: Start a new phase in the Exosuit phased development workflow.
---

### Starting a New Phase

Use the LM tools to orient yourself and start a phase:

1. **Orient**: Call `exo-status` to get a quick project snapshot.
2. **Start Phase**: Call `exo-phase-start` with the phase ID to activate it.
3. **Plan**: 
   - Review the phase goals from `exo-plan` output.
   - Add tasks using `exo-add-task` for each piece of work.
   - Stop for user approval before implementing.
4. **Implement**: Work through tasks one at a time. Call `exo-task-complete` as you finish each one.

**Note**: The implementation plan lives in `docs/agent-context/current/implementation-plan.toml`. Don't edit it directly—use the LM tools.
