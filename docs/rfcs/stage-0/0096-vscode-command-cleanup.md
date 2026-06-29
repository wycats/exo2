<!-- exo:96 ulid:01kg5kp2fs1whqrnbfw7ew4edy -->

# RFC 96: VS Code Extension Command Cleanup


# RFC 0096: VS Code Extension Command Cleanup

## Summary

Remove dead commands and document deferred features in the VS Code extension to reduce confusion, improve maintainability, and ensure the command inventory matches actual functionality.

## Motivation

An audit of `packages/exosuit-vscode/package.json` revealed:

- **7 dead commands**: Registered in manifest but have no implementation
- **2 deferred chat participants**: Features planned but not yet activated

These cause:

1. **User confusion**: Commands appear in palette but fail silently
2. **Agent confusion**: LM may try to invoke non-functional commands
3. **Maintenance burden**: Dead code obscures actual functionality
4. **Audit failures**: Security/quality scans flag unused declarations

## Detailed Design

### Commands to Remove

The following commands should be **completely removed** from `package.json`:

| Command ID                   | Title                | Reason                              |
| ---------------------------- | -------------------- | ----------------------------------- |
| `exosuit.addIdea`            | Add Idea to Plan     | Replaced by `exo-idea` LM tool      |
| `exosuit.promoteToDecision`  | Promote to Decision  | Feature not implemented             |
| `exosuit.promoteToTask`      | Promote to Task      | Feature not implemented             |
| `exosuit.insertTaskTemplate` | Insert Task Template | Feature not implemented             |
| `exosuit.listPrompts`        | List Prompts         | Superseded by `exosuit.openPrompts` |
| `exosuit.insertPhase`        | Insert Phase         | Feature not implemented             |
| `exosuit.completePhase`      | Complete Phase       | Replaced by `exo phase finish` CLI  |

### Working Commands (Keep)

| Command ID             | Title               | Status     |
| ---------------------- | ------------------- | ---------- |
| `exosuit.openStudio`   | Open Exosuit Studio | ✅ Working |
| `exosuit.openTree`     | Open Tree View      | ✅ Working |
| `exosuit.openPrompts`  | Open Prompts        | ✅ Working |
| `exosuit.openContext`  | Open Context View   | ✅ Working |
| `exosuit.openPlan`     | Open Plan View      | ✅ Working |
| `exosuit.showArtifact` | Show Artifact       | ✅ Working |
| `exosuit.refreshTree`  | Refresh Tree        | ✅ Working |
| `exosuit.refreshPlan`  | Refresh Plan        | ✅ Working |
| `exosuit.insertPrompt` | Insert Prompt       | ✅ Working |
| `exosuit.attachPrompt` | Attach Prompt       | ✅ Working |
| `exosuit.runPrompt`    | Run Prompt          | ✅ Working |

### Chat Participants (Document as Deferred)

The following chat participants are registered but intentionally not activated. Add explanatory comments:

#### `@exosuit` Participant

**Current State:** Declared in package.json but handler not connected  
**Intent:** General-purpose Exosuit chat interface  
**Action:** Add comment explaining deferral

```json
{
  "id": "exosuit",
  "name": "exosuit",
  "description": "The Exosuit AI assistant for project management",
  "isSticky": true,
  "__comment": "DEFERRED: Chat participant not yet activated. See RFC 0096."
}
```

**Note:** JSON doesn't support comments. Instead, document in:

1. Code comment in extension.ts where registration would occur
2. README.md under "Planned Features"

#### `@exosuit-triage` Participant

**Current State:** Declared but not activated  
**Intent:** Specialized triage workflow participant  
**Action:** Same documentation approach as `@exosuit`

### Implementation Steps

#### Step 1: Remove Dead Commands from package.json

Delete these entries from `contributes.commands`:

```diff
- {
-   "command": "exosuit.addIdea",
-   "title": "Add Idea to Plan",
-   "category": "Exosuit"
- },
- {
-   "command": "exosuit.promoteToDecision",
-   "title": "Promote to Decision",
-   "category": "Exosuit"
- },
- {
-   "command": "exosuit.promoteToTask",
-   "title": "Promote to Task",
-   "category": "Exosuit"
- },
- {
-   "command": "exosuit.insertTaskTemplate",
-   "title": "Insert Task Template",
-   "category": "Exosuit"
- },
- {
-   "command": "exosuit.listPrompts",
-   "title": "List Prompts",
-   "category": "Exosuit"
- },
- {
-   "command": "exosuit.insertPhase",
-   "title": "Insert Phase",
-   "category": "Exosuit"
- },
- {
-   "command": "exosuit.completePhase",
-   "title": "Complete Phase",
-   "category": "Exosuit"
- }
```

#### Step 2: Remove Dead Keybindings (if any)

Check `contributes.keybindings` for bindings to removed commands.

#### Step 3: Remove Dead Menu Items (if any)

Check `contributes.menus` for items referencing removed commands.

#### Step 4: Document Deferred Chat Participants

Add to `packages/exosuit-vscode/src/extension.ts`:

```typescript
// ============================================================================
// DEFERRED FEATURES (RFC 0096)
// ============================================================================
// The following features are declared in package.json but not yet activated:
//
// Chat Participants:
// - @exosuit: General-purpose chat interface (waiting for LM tool maturity)
// - @exosuit-triage: Specialized triage workflow (waiting for triage RFC)
//
// These are intentionally NOT activated. See docs/rfcs/stage-0/0096-*.md
// ============================================================================
```

#### Step 5: Update README

Add "Planned Features" section listing deferred capabilities.

### Verification

After cleanup, run:

```bash
# Verify no dangling references
grep -r "addIdea\|promoteToDecision\|promoteToTask\|insertTaskTemplate\|listPrompts\|insertPhase\|completePhase" packages/exosuit-vscode/

# Should return no results (except this RFC and changelog)
```

## Implementation Plan (Stage 2)

- [ ] Remove 7 dead commands from package.json
- [ ] Remove any associated keybindings
- [ ] Remove any associated menu items
- [ ] Add deferred feature documentation to extension.ts
- [ ] Update README.md with planned features section
- [ ] Run verification grep
- [ ] Update changelog

## Success Criteria

1. **Zero Dead Commands**: All declared commands have implementations
2. **Documented Deferrals**: Chat participants clearly marked as planned
3. **Clean Audit**: `npm run lint` passes with no unused declarations
4. **No Regressions**: All working commands still function

## Alternatives Considered

### Alternative 1: Stub Implementations

Add stub implementations that show "Coming Soon" messages.

**Rejected:** Clutters codebase, still confusing to users.

### Alternative 2: Keep as Placeholders

Leave declarations for future implementation.

**Rejected:** Already causing confusion, no timeline for implementation.

### Alternative 3: Feature Flags

Hide commands behind feature flags.

**Rejected:** Overkill for dead code removal. Use for gradual rollout instead.

## Context Updates (Stage 3)

- [ ] Update `docs/manual/features/vscode-extension.md` with current command list
- [ ] Remove references to dead commands from any user documentation

