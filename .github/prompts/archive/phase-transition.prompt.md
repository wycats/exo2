---
agent: agent
description: Finish the current phase and transition to the next one.
---

### Phase Transition

1. **Verify Completion**: 
   - Check `exo-phase` to see task status. All tasks should be complete.
   - Run `exo verify` if verification hooks are configured.

2. **Finish Phase**: Call `exo-phase-finish` to mark the phase complete.
   - This updates the plan and prepares for the next phase.
   - The command will tell you what phase comes next.

3. **RFC Promotion** (if applicable):
   - If this phase implemented a Stage 3 RFC, ensure `docs/manual/` is updated.
   - Use `exo rfc promote` to advance the RFC to Stage 4.

4. **Handoff**: If ending the session, call `exo-context` to generate a handoff summary for the next agent.

**Note**: If you hit friction during this process (missing command, confusing output), report it. That's valuable feedback.
