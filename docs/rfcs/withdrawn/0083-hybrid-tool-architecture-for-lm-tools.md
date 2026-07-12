<!-- exo:83 ulid:01kg5kp2f3efgyarbxdt0scshx -->

# RFC 83: Hybrid Tool Architecture for LM Tools

- **Status**: Withdrawn
- **Stage**: 3
- **Reason**:

# RFC 0083: Hybrid Tool Architecture for LM Tools

- **Superseded by**: RFC 0136

> **Additional Note (2026-02-02)**: The Clap Bridge pattern mentioned in this RFC (mirror enums in `clap_bridge.rs`) is being removed as part of the Inline Spec Definition migration (RFC 0201). The bridge was needed to maintain parity between Clap definitions and `Command::args()` trait implementations; with inline spec definition, this dual-definition architecture is eliminated.

## Summary

Define a three-tier hybrid architecture for exposing Exosuit capabilities to Language Models that balances discoverability, token economy, and decision quality. The architecture provides:

1. **Zero-arg orientation tools** (7 tools) for safe, repeatable context gathering
2. **Method-based dispatch tools** (5 tools) for type-safe mutations with confirmation
3. **Convenience zero-arg mutations** (2 tools) for high-frequency operations

Total: 14 tools, staying well under OpenAI's recommended limit of 20 functions.

## Relationship to RFC 0095 (Intent Taxonomy)

**This RFC and RFC 0095 address orthogonal concerns:**

- **RFC 0083 (this document)**: Defines **implementation architecture** based on _how tools work_
  - Zero-arg orientation (read-only, no parameters)
  - Method-based dispatch (mutations with method enums)
  - Convenience zero-arg mutations (high-frequency operations)

- **RFC 0095**: Defines **intent taxonomy** based on _when users need tools_
  - Orientation (where am I?)
  - Navigation (where should I go?)
  - Lifecycle (how do I transition states?)
  - Capture (how do I record information?)
  - Session (how do I bootstrap/recover?)
  - Advanced (complex operations)

**Both dimensions apply to every tool.** A tool like `exo-status` is:

- **Implementation tier**: Zero-arg orientation (no parameters, read-only)
- **Intent category**: Orientation (answers "where am I?")

A tool like `exo-phase-ops` is:

- **Implementation tier**: Method-based dispatch (takes method enum, mutations)
- **Intent category**: Lifecycle (transitions phase states)

### Cross-Dimensional Mapping

| Tool            | Implementation Tier (10061)   | Intent Category (10081) | Purpose                   |
| --------------- | ----------------------------- | ----------------------- | ------------------------- |
| `exo-status`    | Zero-arg orientation          | Orientation             | Current state snapshot    |
| `exo-plan`      | Zero-arg orientation          | Orientation             | Roadmap view              |
| `exo-phase`     | Zero-arg orientation          | Orientation             | Phase details             |
| `exo-steering`  | Zero-arg orientation          | Navigation              | Multiple next actions     |
| `exo-context`   | Zero-arg orientation          | Session                 | Full context dump         |
| `exo-epoch`     | Zero-arg orientation          | Orientation             | Milestone scope           |
| `exo-inbox`     | Zero-arg orientation          | Session                 | Pending intents           |
| `exo-phase-ops` | Method-based dispatch         | Lifecycle               | Phase state transitions   |
| `exo-task-ops`  | Method-based dispatch         | Lifecycle               | Task state transitions    |
| `exo-plan-ops`  | Method-based dispatch         | Lifecycle               | Plan modifications        |
| `exo-idea`      | Convenience zero-arg mutation | Capture                 | Quick idea capture        |
| `exo-add-task`  | Convenience zero-arg mutation | Capture                 | Quick task addition       |
| `exosuit`       | Mega-tool                     | Advanced                | Complex multi-op dispatch |

**Design Principle**: The implementation tier (how it works) is chosen based on technical constraints (parameter count, mutation safety). The intent category (when to use it) is chosen based on user mental models and language model steering needs.

**For Tool Implementers**: Choose the implementation tier first based on technical requirements, then assign the intent category based on user workflow position.

**For Model Descriptions**: Intent categories should drive the "Use this when" guidance in `modelDescription` fields.

## Motivation

### Empirical Constraints

Research into LLM tool design (OpenAI, Anthropic, MCP) reveals critical constraints:

1. **Tool count soft limit**: OpenAI recommends "fewer than 20 functions at any one time" for optimal accuracy
2. **Token budget**: Tool schemas consume input tokens; 50+ tools can consume 5,000-15,000 tokens before any actual work
3. **Decision quality**: More tools = harder for the model to select the correct one; accuracy degrades with choice overload

### The "Anchor Command" Problem

Agents need reliable entry points when:

- Starting a fresh session
- Recovering from errors
- Reorienting after context loss
- Performing handoffs between agents

These "anchor commands" should be:

- **Zero-parameter** (no failure modes from malformed input)
- **Pure/read-only** (safe to call repeatedly)
- **High-signal** (return actionable context, not noise)

### Real-World Analogies

The best CLI tools provide zero-arg orientation commands:

| Tool    | Command       | Purpose                    |
| ------- | ------------- | -------------------------- |
| Git     | `git status`  | What changed? What's next? |
| Docker  | `docker ps`   | What's running?            |
| Git     | `git log`     | Recent history             |
| Kubectl | `kubectl get` | List resources             |

These commands are memorable because they:

- Require no parameters (low cognitive load)
- Always succeed (deterministic state snapshot)
- Guide next actions (implicit steering)

## Philosophy: Orientation vs. Mutation

The hybrid architecture distinguishes two fundamental operation types:

### Orientation Operations (Query/Read)

**Characteristics**:

- Zero or minimal parameters
- Pure/deterministic
- Safe to repeat
- Never require confirmation
- Return current state + guidance

**Purpose**: Help agents build a mental model of the workspace state before acting.

**Real-world analogy**: Looking at a map before driving, checking `git status` before committing.

### Mutation Operations (Command/Write)

**Characteristics**:

- Require explicit parameters (what to change, how)
- Side effects (write/exec)
- May require user confirmation
- Include rollback/undo information in responses

**Purpose**: Execute discrete state transitions with safety guardrails.

**Real-world analogy**: Turning the wheel, running `git commit`, pressing "submit".

### The Hybrid Approach

Rather than choosing between:

- **Many small tools** (discovery overhead, token bloat, decision paralysis)
- **One mega-tool** (complex dispatch, poor error messages, steep learning curve)

We use **three tool categories**:

1. **Zero-arg orientation** → Discovery, context, steering
2. **Method-based dispatch** → Type-safe mutations with confirmation
3. **Convenience zero-arg** → Wrappers for high-frequency operations

## The Three Tool Categories

### Category 1: Zero-Arg Orientation Tools (7 tools)

Pure, stateless tools that agents can call freely for context.

| Tool        | CLI Mapping     | Returns                | Why High-Leverage                     |
| ----------- | --------------- | ---------------------- | ------------------------------------- |
| exo-status  | exo status      | Project health summary | Like `git status` - always start here |
| exo-plan    | exo plan review | Current roadmap        | Big picture view                      |
| exo-phase   | exo phase show  | Active phase details   | What work is happening now            |
| exo-map     | exo map --json  | Steering + suggestions | GPS for workspace navigation          |
| exo-context | exo ai context  | Full context dump      | Session handoff/continuity            |
| exo-epoch   | exo epoch show  | Epoch summary          | Milestone scope                       |
| exo-ideas   | exo idea list   | Backlog/triage         | Quick backlog review                  |

**Design invariants**:

- No parameters means no type errors
- Pure functions mean safe to call repeatedly
- Each returns structured JSON with embedded steering
- Responses include `next_call` suggestions

### Category 2: Method-Based Dispatch Tools (5 tools)

Mutations grouped by resource domain, using enums for type safety.

| Tool          | Methods                         | Purpose                 |
| ------------- | ------------------------------- | ----------------------- |
| exo-phase-ops | start, finish, status           | Phase lifecycle         |
| exo-task-ops  | add, complete, list, update     | Task management         |
| exo-plan-ops  | review, add-task, update-status | Plan modifications      |
| exo-rfc-ops   | list, show, create, promote     | RFC management          |
| exo-impl-ops  | add-step, log, update           | Implementation tracking |

**Design pattern**:

```json
{
  "name": "exo_phase_ops",
  "parameters": {
    "method": {
      "type": "string",
      "enum": ["start", "finish", "status"]
    },
    "phase_id": { "type": "string" },
    "confirm": { "type": "boolean", "default": false }
  }
}
```

**Why method-based dispatch?**

- Keeps related operations together (easier to discover)
- Type-safe via enum constraints
- Mirrors patterns used by GitHub MCP tools
- Reduces tool count 10x while preserving full functionality

### Category 3: Convenience Zero-Arg Mutations (2 tools)

High-frequency operations that deserve direct shortcuts.

| Tool         | Wraps    | Confirmation | Use Case                  |
| ------------ | -------- | ------------ | ------------------------- |
| exo-idea     | idea.add | Required     | Quick idea capture        |
| exo-add-task | task.add | Required     | Add task to current phase |

**Why convenience wrappers?**

- These operations happen 10x more often than others
- Zero-arg reduces cognitive load for common tasks
- Confirmation requirement provides safety without friction
- Still map to underlying method-based tools

**Design constraint**: Only add convenience wrappers when usage data justifies the tool slot.

## Tool Surface Enumeration

### Complete Tool List (14 total)

#### Zero-Arg Orientation (7)

1. `exo-status` → project health
2. `exo-plan` → roadmap
3. `exo-phase` → current phase
4. `exo-map` → workspace navigation
5. `exo-context` → full context dump
6. `exo-epoch` → milestone scope
7. `exo-ideas` → backlog

#### Method-Based Dispatch (5)

8. `exo-phase-ops` → phase lifecycle
9. `exo-task-ops` → task management
10. `exo-plan-ops` → plan modifications
11. `exo-rfc-ops` → RFC management
12. `exo-impl-ops` → implementation tracking

#### Convenience Zero-Arg (2)

13. `exo-idea` → quick capture
14. `exo-add-task` → quick task creation

## Steering-First Design

Every tool response includes guidance for what to do next.

### Response Structure

```json
{
  "status": "ok" | "needs_input" | "confirm_required" | "error",
  "data": { /* operation-specific payload */ },
  "steering": {
    "next_call": {
      "tool": "exo-task-ops",
      "params": { "method": "list", "phase_id": "current" }
    },
    "message": "Consider reviewing current tasks before adding new ones"
  }
}
```

### Steering Patterns

**Success responses**:

- Suggest logical next step
- Example: After `phase.start`, suggest `task.list` to see work items

**Error responses**:

- Include recovery path
- Example: Unknown phase ID → suggest `exo-phase` to see valid phases

**Confirmation-required**:

- Explain what will happen
- Provide exact retry with `confirm: true`

### Design Invariant: Orientation Tools Never Error

Zero-arg orientation tools always return current state, even if that state is "empty" or "unconfigured":

- `exo-status` with no active phase → returns "no active phase" (not error)
- `exo-ideas` with empty backlog → returns empty list (not error)
- `exo-map` in new workspace → returns minimal structure (not error)

This guarantees agents always have a recovery path.

## CommandSpec Integration

The tool surface is derived from `CommandSpec` (RFC 0132), not maintained separately.

### Mapping Rules

**Namespaces → Tool Categories**:

| CommandSpec Namespace | Tool Category        | Example Tool    |
| --------------------- | -------------------- | --------------- |
| `phase.*`             | Method dispatch      | `exo-phase-ops` |
| `task.*`              | Method dispatch      | `exo-task-ops`  |
| `status` (leaf)       | Zero-arg orientation | `exo-status`    |
| `idea.add` (leaf)     | Zero-arg convenience | `exo-idea`      |

**Effect Annotations → Confirmation Requirements**:

| Effect  | Confirmation | Tool Category                           |
| ------- | ------------ | --------------------------------------- |
| `pure`  | Never        | Zero-arg orientation                    |
| `write` | Required     | Method dispatch or zero-arg convenience |
| `exec`  | Required     | Method dispatch                         |

### Zero-Arg Operation Detection

A CommandSpec operation qualifies for zero-arg orientation if:

1. Effect is `pure`
2. All parameters are optional (or have defaults)
3. Operation is semantically idempotent

Example from CommandSpec:

```toml
[[commands]]
name = "status"
effect = "pure"
about = "Show project health summary"
# No required arguments → qualifies for zero-arg tool
```

## Implementation Phases

### Phase 1: Critical Orientation (Priority 1)

- [ ] `exo-map` - workspace navigation/steering
- [ ] `exo-context` - full context dump

**Why first?**: These are session bootstrap tools; every agent needs them.

### Phase 2: Lifecycle Operations (Priority 2)

- [ ] `exo-phase-ops` - phase lifecycle
- [ ] `exo-task-ops` - task management

**Why second?**: Core workflow operations for active development.

### Phase 3: Extended Context (Priority 3)

- [ ] `exo-epoch` - milestone scope
- [ ] `exo-ideas` - backlog

**Why third?**: Enhanced discovery; nice-to-have but not blocking.

### Phase 4: Convenience Tools (Priority 4)

- [ ] `exo-idea` - quick capture
- [ ] `exo-add-task` - quick task creation

**Why last?**: Convenience tools with narrower use cases.

## Migration Path

### From RFC 0082 Single-Tool Model

**Original proposal**: One `exosuit` tool with ports (`run`, `locate`, `edit`)

**What changes**:

- Replace single tool with 14 specialized tools
- Preserve steering-first error design
- Preserve capability tickets for confirmation

**What stays**:

- No shell strings (structured inputs only)
- Signed tickets for mutations
- `next_call` guidance in responses

**Migration**: RFC 0082's steering principles remain valid; the tool surface design is superseded by this RFC.

### From RFC 0125 Machine Channel

**Original proposal**: Method-based dispatch pattern

**What changes**:

- Add zero-arg orientation category
- Add convenience zero-arg mutations
- Formalize the three-tier taxonomy

**What stays**:

- Method-based dispatch for mutations
- Help ladder for discovery
- Effect annotations from CommandSpec

**Migration**: RFC 0125 is extended, not replaced. This RFC provides the higher-level philosophy.

## Drawbacks

**Tool count still higher than minimal**: 14 tools vs. 1-3 mega-tools.

**Mitigation**: Research shows models handle 14 tools well; the soft limit is ~20.

**Maintenance burden**: More tools = more schemas to keep in sync.

**Mitigation**: All tools derive from CommandSpec; no manual duplication.

**Discoverability**: Agents must learn 14 tool names.

**Mitigation**: Consistent naming (prefix + domain) + `exo-map` for discovery.

## Alternatives

### Alternative 1: Single Mega-Tool

One `exosuit` tool with complex dispatch.

**Pros**: Minimal tool count (1)
**Cons**:

- Complex parameter schemas (harder to learn)
- Poor error messages (which parameter was wrong?)
- No type safety for method dispatch

### Alternative 2: Tool-Per-Operation

Expose every operation as a separate tool (50+ tools).

**Pros**: Maximum granularity, simple schemas
**Cons**:

- Exceeds OpenAI's 20-tool recommendation
- Token budget consumed by schemas
- Decision paralysis from too many choices

### Alternative 3: Help Ladder Only

One `exosuit` tool with multi-step help ladder navigation.

**Pros**: Single entry point, progressive disclosure
**Cons**:

- Requires 2-3 calls for every operation (latency)
- No shortcuts for common operations
- Poor experience for frequent operations

### Why Hybrid Is Better

The three-tier approach:

- Stays under 20 tools (✓)
- Provides zero-call shortcuts for common queries (✓)
- Type-safe dispatch for mutations (✓)
- Consistent naming and patterns (✓)

## Unresolved Questions

1. **Naming conventions**: Should tools use `exo-` prefix or `exosuit_` prefix?
   - Decision: Use `exo-` for consistency with CLI naming

2. **Method parameter naming**: Should it be `method`, `operation`, or `action`?
   - Decision: Use `method` to match industry patterns (GitHub MCP, etc.)

3. **Confirmation UX**: Should confirmation be a boolean parameter or a separate ticket flow?
   - Decision: Use boolean parameter for simplicity; tickets for advanced flows

4. **Tool evolution**: How to deprecate/rename tools without breaking agents?
   - Proposed: Version tools (`exo-status-v2`) or use capability negotiation

## Future Possibilities

### Tool Analytics

Track which tools are actually used to inform:

- Which convenience wrappers to add
- Which method-based tools to split
- Which orientation tools are redundant

### Dynamic Tool Projection

Based on workspace state, expose different tool surfaces:

- New workspace → minimal orientation tools
- Active development → full tool surface
- Maintenance mode → read-only tools

### Cross-Agent Tool Sharing

Export tool schemas for use by other systems:

- MCP servers
- AI agent frameworks
- IDE extensions

### Tool Composition

Allow tools to call other tools internally:

- `exo-add-task` calls `exo-task-ops` under the hood
- Composition layer for complex workflows

## Appendix: Tool Schema Examples

### Zero-Arg Orientation Tool

```json
{
  "name": "exo_status",
  "description": "Show project health summary. Like `git status` - always start here when reorienting.",
  "parameters": {
    "type": "object",
    "properties": {},
    "required": []
  }
}
```

### Method-Based Dispatch Tool

```json
{
  "name": "exo_task_ops",
  "description": "Manage tasks in the current phase",
  "parameters": {
    "type": "object",
    "properties": {
      "method": {
        "type": "string",
        "enum": ["add", "complete", "list", "update"],
        "description": "Operation to perform"
      },
      "task_id": {
        "type": "string",
        "description": "Task identifier (required for complete/update)"
      },
      "description": {
        "type": "string",
        "description": "Task description (required for add)"
      },
      "confirm": {
        "type": "boolean",
        "default": false,
        "description": "Confirm mutations (required for write operations)"
      }
    },
    "required": ["method"]
  }
}
```

### Convenience Zero-Arg Mutation

```json
{
  "name": "exo_idea",
  "description": "Quickly capture an idea to the backlog. Wraps `exo idea add`.",
  "parameters": {
    "type": "object",
    "properties": {
      "idea": {
        "type": "string",
        "description": "The idea to capture"
      },
      "confirm": {
        "type": "boolean",
        "default": false,
        "description": "Confirm addition to backlog"
      }
    },
    "required": ["idea"]
  }
}
```
