<!-- exo:111 ulid:01kg5kp2ghjp1t4x3y78ed5fwk -->

# RFC 111: Agent Guidance Architecture

- **Supersedes**: RFC 0091



# RFC 0111: Agent Guidance Architecture

| Field        | Value                                                       |
| ------------ | ----------------------------------------------------------- |
| **Stage**    | 4 (Stable)                                                  |
| **Created**  | 2026-01-15                                                  |
| **Champion** | —                                                           |
| **Related**  | RFC 0091 (Protected Files), RFC 0136 (LM Tool Architecture) |

## Summary

Establish a coherent agent guidance architecture that:

1. Treats agents as collaborators to be taught, not adversaries to be blocked
2. Uses LM tools as THE agent interface (not CLI)
3. Provides file-scoped contextual instructions via VS Code's `.instructions.md` files
4. Rehabilitates AGENTS.md with clear scope and maintenance strategy
5. Removes the ProtectedFileWatcher entirely (reactive blocking is anti-pattern)

## Motivation

### The Core Problem

Agents frequently edit managed files directly instead of using the appropriate tools. Previous mitigations were reactive and adversarial:

```
Agent touches managed file → ProtectedFileWatcher detects → Revert + warning
```

This failed because:

1. **Adversarial framing**: Treats agents as threats to be blocked
2. **Race conditions**: CLI lock timing issues
3. **No learning**: Agent doesn't understand what to do instead
4. **Instruction drift**: AGENTS.md becomes stale and ignored

### Root Causes

Why do agents edit directly instead of using tools?

1. **Knowledge gap**: Agent doesn't know tools exist
2. **Capability gap**: Tool is missing needed functionality
3. **Friction gap**: Direct edit feels faster
4. **Discovery gap**: Instructions are in AGENTS.md but not surfaced at point of need

### The Insight

Instead of blocking edits after they happen, **teach agents at the moment they access managed files**. VS Code's file-scoped instructions do exactly this.

## Design Philosophy

### Command Spec as Source of Truth

```
┌─────────────────────────────────────────────┐
│           command-spec.json                 │
│   (Canonical definition of operations)      │
└─────────┬───────────────────┬───────────────┘
          │                   │
          ▼                   ▼
┌─────────────────┐   ┌─────────────────┐
│   LM Tools      │   │      CLI        │
│   (VS Code)     │   │   (Universal)   │
└─────────────────┘   └─────────────────┘
```

Both LM tools and CLI are **projections** of the same command spec. Neither is "more real" than the other. They serve different audiences.

> **Implementation Note (2026-02-02)**: The `command-spec.json` artifact shown above is generated at **compile time** from Clap annotations extended with `#[exo(...)]` custom attributes. This "Inline Spec Definition" approach (RFC 0201) ensures the schema artifact is always in sync with the CLI implementation. See [RFC 0201](../stage-1/0201-exospec-derive-macro-inline-commandspec-definition.md) for details.

### LM Tools are THE Agent Interface

**For agents working in VS Code, LM tools are the only interface that matters.**

Agents should not:

- Think "should I use tool or CLI?" (cognitive overhead → oscillation)
- Write scripts that shell out to CLI (unnecessary indirection)
- Fall back to CLI when tools work correctly

Agent-facing documentation should:

- Show only LM tool usage
- Not mention CLI as an alternative
- Frame tools as the natural way to interact

### CLI is for Humans and Automation

The CLI exists for:

- **Humans in terminals** who prefer command-line workflows
- **Other editors** (Neovim, Emacs, Zed) without VS Code's LM Tool API
- **CI/CD pipelines** running in headless environments
- **Debugging** when an engineer asks to trace a specific command

The CLI is **not** for:

- AI agents in VS Code writing orchestration scripts
- "Fallback" when an agent should use tools
- Mixing with tool usage in the same workflow

Human documentation should explicitly state:

> If you're an AI agent working in VS Code, use the LM tools directly. Do not write scripts that shell out to the CLI - that's adding unnecessary indirection when you have native tool access.

### Tool Approval Recommendation

Exosuit's LM tools are safe operations on local project files:

- Effects are predictable and documented
- Changes are reversible via git
- Pre-commit hooks validate invariants

Users should consider **blanket approval** for Exosuit tools to reduce friction. The confirmation dialog provides visibility, not security gatekeeping.

## Guidance Hierarchy

Instructions flow from general to specific:

```
┌────────────────────────────────────────────────┐
│ 1. AGENTS.md (workspace-wide philosophy)       │
│    - Project mission                           │
│    - Workflow principles                       │
│    - "Use LM tools, not direct edits"          │
└────────────────────────────────────────────────┘
                      │
                      ▼
┌────────────────────────────────────────────────┐
│ 2. .instructions.md (file-scoped guidance)     │
│    - Triggered when accessing specific files   │
│    - "For project state, use exo-phase-start..." │
│    - Contextual, just-in-time                  │
└────────────────────────────────────────────────┘
                      │
                      ▼
┌────────────────────────────────────────────────┐
│ 3. Tool descriptions (modelDescription)        │
│    - What each tool does                       │
│    - When to use it                            │
│    - Parameter schemas                         │
└────────────────────────────────────────────────┘
```

Each layer adds specificity without repeating content from higher layers.

## File-Scoped Instructions

### Mechanism

VS Code's `.instructions.md` files with `applyTo` patterns:

```yaml
---
applyTo: "**/docs/agent-context/tasks.sql"
---
# Working with project state
...
```

When an agent accesses managed project-state artifacts, these instructions are automatically injected into context.

### Authoring Principles

Instruction files are consumed by LM agents. Author them accordingly:

1. **Reference tools, not CLI**: Use LM tool names (`exo-add-idea`, `exo-phase-start`) rather than CLI commands. The agent's interface is tools; CLI is a separate projection.

2. **Affirmative guidance only**: State what TO do, not what NOT to do. "Use `exo-add-task` to add tasks" rather than "Do not edit this file directly." Affirmative framing teaches the right behavior.

3. **Abstract over storage**: Never mention file paths as "the thing being managed." The abstraction is the concept (ideas, tasks, phases), not the storage location.

See RFC 0177 (Instruction Localization Convention) for detailed conventions.

### Instructions Directory Structure

```
.github/
└── instructions/
    ├── plan-toml.instructions.md
    ├── ideas-toml.instructions.md
    ├── implementation-plan-toml.instructions.md
    ├── inbox-toml.instructions.md
    └── agent-context-readonly.instructions.md
```

### Example: project-state.instructions.md

```markdown
---
applyTo: "**/docs/agent-context/tasks.sql"
---

# Working with project state

This managed state contains the project roadmap: epochs, phases, and tasks.

## Do not edit this state directly

Use these tools instead:

| Action               | Tool                                      |
| -------------------- | ----------------------------------------- |
| Check status         | `exo-status` or `exo-steering`            |
| Start a phase        | `exo-phase-start` with phase ID           |
| Finish current phase | `exo-phase-finish`                        |
| Add a task           | `exo-add-task` with title and description |
| Complete a task      | `exo-task-complete` with task ID          |

## Why tools?

These tools maintain invariants:

- Unique IDs and ULIDs
- Status transitions (pending → active → completed)
- Cross-references between epochs, phases, and tasks
- Proper schema and relational invariants

Direct edits may corrupt these invariants.
```

Note: **No CLI mentioned**. Tools only.

### Example: agent-context-readonly.instructions.md

```markdown
---
applyTo: "**/docs/agent-context/{axioms,modes,council,prompts}*.toml"
---

# Read-Only Configuration

These files define system behavior and are edited by users, not agents.

- **axioms.\*.toml**: Core constraints and principles
- **modes.toml**: Agent operating modes
- **council.toml**: Multi-agent coordination
- **prompts.toml**: Prompt templates

## Reading is encouraged

Read these files to understand system configuration.

## Do not edit

If you think a change is needed, suggest it to the user.
```

### Managed Files Inventory

| File                 | Purpose             | LM Tools                                                                   |
| -------------------- | ------------------- | -------------------------------------------------------------------------- |
| SQLite project state | Project roadmap     | `exo-phase-start`, `exo-phase-finish`, `exo-add-task`, `exo-task-complete` |
| SQLite idea state    | Backlog             | `exo-idea`                                                                 |
| SQLite task state    | Current phase state | `exo-add-task`, `exo-task-complete`                                        |
| SQLite inbox state   | Pending intents     | `exo-inbox`                                                                |
| Feedback state       | User feedback       | — (user-managed)                                                           |
| Decision state       | Decision log        | — (user-managed)                                                           |
| `axioms.*.toml`      | Constraints         | — (read-only)                                                              |
| `modes.toml`         | Mode definitions    | — (read-only)                                                              |
| `council.toml`       | Coordination rules  | — (read-only)                                                              |
| `prompts.toml`       | Templates           | — (read-only)                                                              |

## AGENTS.md Rehabilitation

### Current Issues

1. **Too detailed**: Contains implementation specifics that drift
2. **Mixed concerns**: Philosophy and mechanics interleaved
3. **Missing quick start**: No immediate actionable guidance
4. **CLI/Tool confusion**: Presents both without clear preference

### Proposed Structure

```markdown
# Agent Workflow & Philosophy

[30-second elevator pitch - what is this project, what's your role]

## Quick Start

1. Check status: Use `exo-status`
2. See current phase: Use `exo-phase`
3. Check for pending work: Use `exo-steering`

## The Exosuit Way

[Keep existing mental model - Brain/Hands/Memory/Conscience]
[This is philosophy, rarely changes]

## Core Principles

### Use Tools, Not Direct Edits

Files in `docs/agent-context/` are managed by LM tools.
When you access these files, you'll receive contextual instructions.

### Phased Execution

Work happens in phases. One phase active at a time.
Use `exo-phase-start` and `exo-phase-finish` to manage lifecycle.

### Context is King

Read current project state through `exo-status` to understand current state.
Use `exo-status` for a quick summary.

## File Structure

[Keep existing reference, move earlier in doc]

## For Humans

[Section explicitly for human readers, not agents]

The CLI (`exo`) provides the same operations for:

- Terminal workflows
- Other editors (Neovim, Emacs, Zed)
- CI/CD pipelines
- Debugging

See the relevant Stage 3/4 RFCs for CLI documentation.
```

### Maintenance Strategy

1. **Keep under 300 lines**: Philosophy only, no implementation details
2. **Link don't embed**: Point to file-scoped instructions, don't duplicate
3. **Version stamp**: Track when AGENTS.md was last updated
4. **CI check**: Verify file references still exist

## ProtectedFileWatcher Removal

**Status**: Removed (2026-01-15)

The ProtectedFileWatcher has been completely removed from the codebase. This includes:

- The `ProtectedFileWatcher` service class
- CLI lock mechanism (`CliLockGuard`, `.exo-cli.lock`)
- Configuration in `exosuit.toml`
- RFC 0091 (moved to `withdrawn/`)

### Why Removal Over Demotion

The original plan was to demote the watcher to a "safety net" that logs instead of reverts. However:

1. It never worked reliably (race conditions, lock timing)
2. Proactive guidance is sufficient
3. Read-only permissions + save rejection provide adequate protection
4. Keeping dead code increases maintenance burden

### Remaining Protection Mechanisms

1. **Read-only file permissions** (`0o444` mode)
2. **VS Code save rejection** (`onWillSaveTextDocument` handler)
3. **File-scoped instructions** (teaches agents proactively)
4. **Pre-commit validation** (exohook catches any escaped edits)

## Implementation Plan

### P0: ~~Fix LM Tool Registration~~ (Done)

✅ Completed (2026-01-15):

1. `buildToolFromSpec` fix committed (`1c29437`)
2. Tool alias registration order fixed (`1ce67a6`)
3. Package.json names (exo-phase, exo-plan, exo-inbox, exo-context) registered first

### P1: ~~Create File-Scoped Instructions~~ (Done)

✅ Completed (2026-01-15):

- `plan-toml.instructions.md`
- `ideas-toml.instructions.md`
- `implementation-plan-toml.instructions.md`
- `inbox-toml.instructions.md`
- `agent-context-readonly.instructions.md`
- `rfcs.instructions.md` (bonus: RFC editing guidance)

### P2: ~~Rehabilitate AGENTS.md~~ (Done)

✅ Completed (2026-01-15):

- Added version stamp (Last Updated header)
- Removed all CLI references from agent-facing sections
- Protocol: Phase Loop uses tool names
- Protocol: Context Check uses exo-status, exo-phase, exo-inbox
- Protocol: Managed Files table with tool mappings
- Reference to `.github/instructions/` for file-scoped guidance
- Commit: `c870e73`

### P3: ~~Demote ProtectedFileWatcher~~ (Done)

✅ Removed entirely (2026-01-15):

- Deleted `ProtectedFileWatcher.ts`
- Removed CLI lock mechanism from `utils.rs`
- Moved RFC 0091 to `withdrawn/`
- Updated `exosuit.toml`

### P4: Update Manual (Human Documentation)

1. Document CLI usage for humans
2. Explain when CLI is appropriate
3. Explicitly state agents should use tools

## Success Metrics

1. **Reduced watcher interventions**: Approaches zero
2. **Agent tool usage**: Agents consistently use LM tools
3. **No scripting attempts**: Agents don't write CLI orchestration scripts
4. **Faster onboarding**: New sessions productive immediately
5. **Instruction freshness**: No "stale docs" complaints

## Open Questions

### Q1: Nested AGENTS.md in agent-context?

Should we create `docs/agent-context/AGENTS.md` for folder-specific guidance?

**Pro**: Additional layer of defense, catches agents exploring the folder
**Con**: Might be redundant with file-scoped instructions

### Q2: Dynamic instructions via extension API?

VS Code may support extension-contributed instructions in the future.

**Pro**: Could inject current phase, active tasks dynamically
**Con**: Not yet stable, `.instructions.md` works today

### Q3: What about agents in other contexts?

Agents running outside VS Code (Cursor, Claude Code, etc.) won't see VS Code instructions.

**Options**:

- Those tools have their own instruction mechanisms
- AGENTS.md serves as universal fallback
- CLI documentation in Manual covers non-VS Code usage

### Q4: Tool approval UX

Should we provide VS Code settings recommendations for blanket tool approval?

**Pro**: Reduces friction significantly
**Con**: Some users may want per-operation control

## Alternatives Considered

### Keep Reactive Watcher Only

Continue with ProtectedFileWatcher as primary enforcement.

**Rejected & Removed**: Race conditions, adversarial framing, no teaching. Watcher was removed entirely.

### CLI as Equal Alternative

Document CLI alongside tools in agent instructions.

**Rejected**: Creates decision overhead, leads to oscillation, enables scripting anti-pattern.

### Chat Participant (@exosuit)

Create explicit chat participant for exosuit interactions.

**Deferred**: Requires explicit invocation. File-scoped instructions are automatic.

### Extension-Contributed Instructions

Wait for VS Code API to support dynamic instruction contribution.

**Deferred**: API not stable. `.instructions.md` works today.

## References

- [VS Code Custom Instructions](https://code.visualstudio.com/docs/copilot/customization/custom-instructions)
- RFC 0091: Protected Files Architecture
- RFC 0136: LM Tool Architecture v2
- RFC 0132: CLI Patterns and Command Spec
