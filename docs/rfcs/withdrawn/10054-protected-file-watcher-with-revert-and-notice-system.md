<!-- exo:10054 ulid:01kmzxeffa7zxmfq06y4t9ee81 -->


# RFC 10054: Protected File Watcher with Revert and Notice System

## Summary

CLI-managed files like `plan.toml` should not be edited directly. When an agent (or user) attempts to edit these files, the system should:
1. Detect the change via file watcher
2. Revert immediately
3. Emit a notice explaining what happened and what to use instead

## Motivation

**The Problem:**
When an agent's edit tool attempts to modify a read-only file, it may silently fail. The agent receives a "success" response but the file is unchanged. This leads to confusion, wasted effort, and incorrect assumptions about state.

Alternatively, if we stop making files read-only, direct edits would succeed but violate the CLI-managed invariant.

**Observed Failure Mode:**
1. Agent reads `plan.toml` to find edit location
2. Agent calls `replace_string_in_file` 
3. Tool reports success (but file was read-only, nothing changed)
4. Agent proceeds as if edit happened
5. Later confusion when state doesn't match expectations

## Proposed Solution

### File Watcher
A VS Code extension file watcher monitors CLI-managed files:
- `docs/agent-context/plan.toml`
- `docs/agent-context/current/implementation-plan.toml`
- Other designated read-only projection files

### On Change Detection
1. **Revert**: Run `git checkout -- <file>` to restore the committed version
2. **Queue Notice**: Append to `.exo/pending-notices.toml`:
   ```toml
   [[notices]]
   timestamp = "2025-12-29T16:30:00Z"
   file = "docs/agent-context/plan.toml"
   severity = "warning"
   message = "Direct edit to plan.toml was reverted. Use exo plan/task commands instead."
   suggested_commands = ["exo plan add-task", "exo task add"]
   ```

### Notice Emission
Notices are emitted via multiple channels:

1. **Machine Channel**: Next `exo json channel` response includes pending notices in the `reminders` field
2. **CLI Verifiers**: Next human-mode `exo` command emits to stderr
3. **VS Code Extension**: Could show a notification to human users

After emission, the notice is cleared from the pending queue.

## Integration Points

- **Existing Reminders Protocol**: The `ResponseEnvelope.reminders` field already exists
- **Global Verifiers**: `run_global_verifiers()` already runs on CLI commands
- **VS Code Extension**: File watcher would live here

## Files to Protect

Initial list (configurable in `exosuit.toml`):
- `docs/agent-context/plan.toml` → use `exo plan`, `exo task`
- `docs/agent-context/current/implementation-plan.toml` → use `exo impl`, `exo phase`
- `docs/agent-context/ideas.toml` → use `exo idea`
- `docs/agent-context/current/walkthrough.toml` → use `exo walkthrough`

## Open Questions

1. Should we also log reverted diffs for debugging?
2. Should notices persist across VS Code restarts?
3. What if git checkout fails (e.g., file not tracked)?

## Alternatives Considered

1. **Read-only permissions**: Silent failure, no feedback (current broken state)
2. **AGENTS.md documentation**: Agent doesn't always read it before editing
3. **Comment headers in files**: Might help but not guaranteed to be read
4. **Post-edit verification by agent**: Relies on agent discipline, easy to forget

