<!-- exo:95 ulid:01kg5kp2fqhvg20fdzngha4zmn -->

# RFC 95: Intent Mapping & Tool Ergonomics


# RFC 0095: Intent Mapping & Tool Ergonomics

## Summary

Define a principled approach to naming and organizing Language Model tools so that:

1. **Users** can naturally reference tools in conversation ("start the next exo-phase")
2. **Agents** can unambiguously choose the correct tool for any given intent
3. **Tool names** map directly to how humans express project management intents

This RFC establishes an "Intent Catalog" that maps natural language expressions to specific tools, naming principles that ensure tools are speakable in English, and success criteria for validating the design.

## Motivation

### The Two Audiences Problem

Exosuit's LM tools serve two distinct audiences with different needs:

| Audience        | How They Use Tools                                            | What They Need                                               |
| --------------- | ------------------------------------------------------------- | ------------------------------------------------------------ |
| **Human Users** | Reference by name in chat: "use exo-phase to start phase-110" | Memorable, pronounceable, unambiguous names                  |
| **AI Agents**   | Select from tool list based on `modelDescription`             | Clear disambiguation, precise scope, actionable descriptions |

The current tool design optimizes for agent precision but neglects user ergonomics.

### Current Problems

1. **Semantic Collision**: `exo-status` vs `exo-map` both claim to answer "what should I do next?"

   - `exo-status`: "Returns current project phase, active tasks, and **next steps**"
   - `exo-map`: "Returns structured steering with **next action suggestions**"

2. **Missing Lifecycle Verbs**: Users naturally say "start phase 110" or "complete task X", but no tools exist for these actions.

3. **Mega-Tool Confusion**: The `exosuit` tool handles 5 operations (list, run, locate, edit, use) but users don't know when to use it vs specialized tools.

4. **Unpronounceability**: Some tool names don't flow in natural sentences ("I'll exosuit locate the artifact" sounds wrong).

### The Goal

A user should be able to express any common project management intent, and:

- If speaking to an agent: the agent chooses the correct tool unambiguously
- If referencing a tool directly: the name fits naturally in the sentence

## Design

### Intent Catalog

The following catalog maps natural language expressions to tool invocations.

#### Orientation Intents (Read-Only, Zero-Arg)

| User Says                    | Intent Category | Correct Tool | Notes                                    |
| ---------------------------- | --------------- | ------------ | ---------------------------------------- |
| "What's the status?"         | Orientation     | `exo-status` | Current phase, tasks, singular next step |
| "Where am I in the project?" | Orientation     | `exo-status` | Same as above                            |
| "What phase am I in?"        | Orientation     | `exo-phase`  | Current phase details and task breakdown |
| "Show me the current phase"  | Orientation     | `exo-phase`  | Same as above                            |
| "What's the plan?"           | Orientation     | `exo-plan`   | High-level roadmap, epoch structure      |
| "What epoch are we in?"      | Orientation     | `exo-epoch`  | Milestone scope (NEW TOOL NEEDED)        |

#### Navigation Intents (Read-Only, AI-Assisted)

| User Says                     | Intent Category | Correct Tool   | Notes                                   |
| ----------------------------- | --------------- | -------------- | --------------------------------------- |
| "What should I do next?"      | Navigation      | `exo-steering` | Multiple options with confidence scores |
| "I'm stuck, help me navigate" | Navigation      | `exo-steering` | Repair actions and suggestions          |
| "What's blocking me?"         | Navigation      | `exo-steering` | Surfaces blockers and repair paths      |
| "Show me the GPS view"        | Navigation      | `exo-steering` | Explicit request for steering           |

**Key Distinction**: `exo-status` answers "where am I?" while `exo-steering` answers "where should I go?"

#### Lifecycle Intents (Mutating, Method-Dispatch)

| User Says              | Intent Category | Correct Tool    | Method     |
| ---------------------- | --------------- | --------------- | ---------- |
| "Start phase 110"      | Phase Lifecycle | `exo-phase-ops` | `start`    |
| "Finish this phase"    | Phase Lifecycle | `exo-phase-ops` | `finish`   |
| "Complete task X"      | Task Lifecycle  | `exo-task-ops`  | `complete` |
| "Mark task X done"     | Task Lifecycle  | `exo-task-ops`  | `complete` |
| "List all tasks"       | Task Lifecycle  | `exo-task-ops`  | `list`     |
| "Update task priority" | Task Lifecycle  | `exo-task-ops`  | `update`   |

#### Planning Intents (Mutating, Convenience)

| User Says                      | Intent Category   | Correct Tool   | Notes                     |
| ------------------------------ | ----------------- | -------------- | ------------------------- |
| "Add a task to this phase"     | Task Capture      | `exo-add-task` | Convenience wrapper       |
| "Schedule task for next phase" | Plan Modification | `exo-plan-ops` | NEW TOOL NEEDED           |
| "Add a task to phase 12"       | Plan Modification | `exo-plan-ops` | Cross-phase task addition |

#### Capture Intents (Mutating, Convenience)

| User Says                 | Intent Category | Correct Tool | Notes                |
| ------------------------- | --------------- | ------------ | -------------------- |
| "I have an idea"          | Idea Capture    | `exo-idea`   | Adds to ideas.toml   |
| "Add this to the backlog" | Idea Capture    | `exo-idea`   | Same as above        |
| "Check the inbox"         | Intent Review   | `exo-inbox`  | Pending user intents |
| "Any feedback for me?"    | Intent Review   | `exo-inbox`  | Same as above        |

#### Context Intents (Session Management)

| User Says                    | Intent Category   | Correct Tool  | Notes                |
| ---------------------------- | ----------------- | ------------- | -------------------- |
| "Give me full context"       | Session Bootstrap | `exo-context` | For handoff/recovery |
| "I'm starting a new session" | Session Bootstrap | `exo-context` | Same as above        |
| "What happened before?"      | Session Recovery  | `exo-context` | Historical context   |

### Tool Taxonomy

Based on the intent catalog, tools fall into these categories:

#### Tier 1: Orientation Tools (Zero-Arg, Read-Only)

| Tool         | Purpose                                | When to Use                           |
| ------------ | -------------------------------------- | ------------------------------------- |
| `exo-status` | Current state + singular next step     | Default orientation; "Where am I?"    |
| `exo-phase`  | Current phase details + task breakdown | Drilling into phase work              |
| `exo-plan`   | High-level roadmap + epoch structure   | Understanding the big picture         |
| `exo-epoch`  | Current epoch context + milestones     | Understanding current milestone (NEW) |

#### Tier 2: Navigation Tools (Zero-Arg, AI-Assisted)

| Tool           | Purpose                                   | When to Use                                         |
| -------------- | ----------------------------------------- | --------------------------------------------------- |
| `exo-steering` | Multiple next actions + confidence scores | When stuck or need options (RENAMED from `exo-map`) |

#### Tier 3: Lifecycle Tools (Method-Dispatch, Mutating)

| Tool            | Methods                             | When to Use                       |
| --------------- | ----------------------------------- | --------------------------------- |
| `exo-phase-ops` | `start`, `finish`, `status`         | Phase lifecycle management (NEW)  |
| `exo-task-ops`  | `add`, `complete`, `list`, `update` | Task lifecycle management (NEW)   |
| `exo-plan-ops`  | `add-phase`, `add-task`, `reorder`  | Plan structure modification (NEW) |

#### Tier 4: Capture Tools (Convenience, Mutating)

| Tool           | Purpose                   | When to Use                                |
| -------------- | ------------------------- | ------------------------------------------ |
| `exo-add-task` | Add task to current phase | Quick task capture without method dispatch |
| `exo-idea`     | Add idea to backlog       | Quick idea capture                         |

#### Tier 5: Session Tools (Context Management)

| Tool          | Purpose              | When to Use                             |
| ------------- | -------------------- | --------------------------------------- |
| `exo-context` | Full context dump    | Session handoff, recovery, bootstrap    |
| `exo-inbox`   | Pending user intents | Check for guidance before starting work |

#### Tier 6: Advanced Operations (Mega-Tool)

| Tool      | Purpose                                    | When to Use                                                             |
| --------- | ------------------------------------------ | ----------------------------------------------------------------------- |
| `exosuit` | Run recipes, locate artifacts, use tickets | Only when no specialized tool exists, or following `steering.next_call` |

### Naming Principles

1. **Pronounceability**: Names must flow in natural English sentences

   - ✅ "Check the exo-status"
   - ✅ "Use exo-phase-ops to start phase 110"
   - ❌ "Exosuit locate the artifact" (awkward)

2. **Noun-Based**: Tool names should be nouns (the thing) not verbs (the action)

   - ✅ `exo-status` (the status)
   - ✅ `exo-phase` (the phase)
   - ❌ `exo-get-status` (verb-heavy)

3. **Method Dispatch Suffix**: Lifecycle tools use `-ops` suffix to indicate they take a method parameter

   - `exo-phase-ops` not `exo-phase-lifecycle`
   - `exo-task-ops` not `exo-task-actions`

4. **Disambiguation by Scope**: Related tools differentiate by scope

   - `exo-status` = snapshot (current state)
   - `exo-steering` = navigation (where to go)
   - `exo-phase` = zoom in (phase details)
   - `exo-plan` = zoom out (roadmap)

5. **No Semantic Overlap**: Each tool answers exactly one question
   - Two tools should never both claim to provide "next steps"

### Tool Descriptions

Every tool's `modelDescription` must follow this template:

```
[One sentence: What this tool returns]

**Use this when**: [Specific trigger conditions]

**Do NOT use when**: [Common mistakes / when to use another tool instead]

**Zero arguments required.**
```

Example for `exo-status`:

```
Returns the current project phase, active tasks, and the single highest-priority next step.

**Use this when**: The user asks "what's the status?" or "where am I?" or you need a quick orientation.

**Do NOT use when**: You need multiple options with confidence scores (use exo-steering) or detailed task breakdown (use exo-phase).

**Zero arguments required.**
```

Example for `exo-steering`:

```
Returns AI-scored steering with multiple next actions, confidence scores, and repair paths.

**Use this when**: The user is stuck, asks "what should I do next?" with uncertainty, or you need to evaluate multiple options.

**Do NOT use when**: A simple status check suffices (use exo-status) or you need phase details (use exo-phase).

**Zero arguments required.**
```

## Relationship to RFC 0083 (Implementation Architecture)

**This RFC and RFC 0083 address orthogonal concerns:**

- **RFC 0095 (this document)**: Defines **intent taxonomy** based on _when users need tools_

  - Maps natural language expressions to correct tool choices
  - Provides mental model for user interaction
  - Drives model descriptions and disambiguation

- **RFC 0083**: Defines **implementation architecture** based on _how tools work_
  - Zero-arg orientation (no parameters)
  - Method-based dispatch (method enums)
  - Convenience zero-arg mutations (shortcuts)

**Both dimensions apply to every tool.** The intent category determines _when_ a tool should be used (and what goes in its `modelDescription`). The implementation tier determines _how_ it's structured (parameters, confirmation requirements).

**Example**: `exo-steering` is:

- **Intent category** (10081): Navigation → "Use when stuck or need multiple options"
- **Implementation tier** (10061): Zero-arg orientation → No parameters, read-only

**Design Principle**: This RFC's intent categories should drive the "Use this when" and "Do NOT use when" sections of every tool's `modelDescription`. The implementation tier is an internal detail that affects parameter schemas.

## Tool Registration Details

This section provides exact registration metadata for each tool. These details would appear in `package.json` for VS Code LM tools.

### Orientation Tools

#### exo-status

```json
{
  "name": "exo-status",
  "tags": ["exosuit", "zero-arg", "orientation"],
  "modelDescription": "Returns the current project phase, active tasks, and the single highest-priority next step.\n\n**Use this when**: User asks 'what's the status?' or 'where am I?' or you need quick orientation at session start.\n\n**Do NOT use when**: You need multiple options with confidence scores (use exo-steering) or detailed task breakdown (use exo-phase) or the full roadmap (use exo-plan).\n\n**Zero arguments required.**",
  "inputSchema": {
    "type": "object",
    "properties": {},
    "required": []
  }
}
```

#### exo-plan

```json
{
  "name": "exo-plan",
  "tags": ["exosuit", "zero-arg", "orientation"],
  "modelDescription": "Returns the high-level project roadmap, epoch structure, and phase sequence.\n\n**Use this when**: User asks 'what's the plan?' or 'show me the roadmap' or you need to understand the project's big-picture structure.\n\n**Do NOT use when**: You need current phase details (use exo-phase) or current state (use exo-status) or navigation guidance (use exo-steering).\n\n**Zero arguments required.**",
  "inputSchema": {
    "type": "object",
    "properties": {},
    "required": []
  }
}
```

#### exo-phase

```json
{
  "name": "exo-phase",
  "tags": ["exosuit", "zero-arg", "orientation"],
  "modelDescription": "Returns current phase details including task breakdown, progress, and completion criteria.\n\n**Use this when**: User asks 'what phase am I in?' or 'show me phase details' or you need to see the full task list for the current phase.\n\n**Do NOT use when**: You just need a quick status snapshot (use exo-status) or need to start/finish phases (use exo-phase-ops) or need the full roadmap (use exo-plan).\n\n**Zero arguments required.**",
  "inputSchema": {
    "type": "object",
    "properties": {},
    "required": []
  }
}
```

### Navigation Tools

#### exo-steering

```json
{
  "name": "exo-steering",
  "tags": ["exosuit", "zero-arg", "navigation"],
  "modelDescription": "Returns AI-scored steering with multiple next action options, confidence scores, repair paths, and blockers.\n\n**Use this when**: User is stuck, asks 'what should I do next?' with uncertainty, or you need to evaluate multiple workflow options. This is the GPS view.\n\n**Do NOT use when**: A simple status check suffices (use exo-status) or you need phase structure (use exo-phase) or the user has already decided what to do next.\n\n**Zero arguments required.**",
  "inputSchema": {
    "type": "object",
    "properties": {},
    "required": []
  }
}
```

### Session Tools

#### exo-context

```json
{
  "name": "exo-context",
  "tags": ["exosuit", "zero-arg", "session"],
  "modelDescription": "Returns full context dump including project state, recent history, axioms, and bootstrap information for session handoff or recovery.\n\n**Use this when**: Starting a new session, recovering from errors, or performing agent handoffs. This is the complete workspace snapshot.\n\n**Do NOT use when**: You just need current status (use exo-status) or specific phase details (use exo-phase). This is verbose and should only be used for full context bootstrap.\n\n**Zero arguments required.**",
  "inputSchema": {
    "type": "object",
    "properties": {},
    "required": []
  }
}
```

#### exo-inbox

```json
{
  "name": "exo-inbox",
  "tags": ["exosuit", "zero-arg", "session"],
  "modelDescription": "Returns pending user intents, feedback items, and guidance that should be checked before starting work.\n\n**Use this when**: Starting a work session or before beginning a new task, to check for pending user guidance or feedback.\n\n**Do NOT use when**: You're already mid-task or need current project status (use exo-status) or navigation guidance (use exo-steering).\n\n**Zero arguments required.**",
  "inputSchema": {
    "type": "object",
    "properties": {},
    "required": []
  }
}
```

### Capture Tools

#### exo-idea

```json
{
  "name": "exo-idea",
  "tags": ["exosuit", "zero-arg-mutation", "capture"],
  "modelDescription": "Captures a new idea to the backlog (ideas.toml) without requiring context about phases or structure.\n\n**Use this when**: User says 'I have an idea' or 'add this to the backlog' or you need to record a thought for later triage.\n\n**Do NOT use when**: The user wants to add a task to a specific phase (use exo-add-task or exo-task-ops) or modify the plan structure (use exo-plan-ops).\n\n**Requires title parameter.**",
  "inputSchema": {
    "type": "object",
    "properties": {
      "title": { "type": "string", "description": "Brief title for the idea" },
      "description": {
        "type": "string",
        "description": "Detailed description (optional)"
      },
      "tags": {
        "type": "string",
        "description": "Comma-separated tags (optional)"
      }
    },
    "required": ["title"]
  }
}
```

#### exo-add-task

```json
{
  "name": "exo-add-task",
  "tags": ["exosuit", "zero-arg-mutation", "capture"],
  "modelDescription": "Adds a new task to the current active phase. Convenience wrapper for the most common task operation.\n\n**Use this when**: User says 'add a task' or 'add this to the current phase' and there is an active phase.\n\n**Do NOT use when**: You need to add tasks to a different phase (use exo-plan-ops) or perform other task operations like complete/update (use exo-task-ops) or there is no active phase.\n\n**Requires id parameter.**",
  "inputSchema": {
    "type": "object",
    "properties": {
      "id": { "type": "string", "description": "Task ID (kebab-case)" },
      "label": {
        "type": "string",
        "description": "Human-readable label (optional)"
      }
    },
    "required": ["id"]
  }
}
```

### Lifecycle Tools (Proposed)

#### exo-phase-ops

```json
{
  "name": "exo-phase-ops",
  "tags": ["exosuit", "method-dispatch", "lifecycle"],
  "modelDescription": "Manages phase lifecycle transitions: start new phases, finish current phase, or get phase status.\n\n**Use this when**: User says 'start phase X' or 'finish this phase' or you need to transition phase states.\n\n**Do NOT use when**: You just need to view phase details (use exo-phase) or check status (use exo-status) or add tasks (use exo-add-task).\n\n**Requires method parameter to specify operation.**\n\n[PROPOSED - NOT YET IMPLEMENTED]",
  "inputSchema": {
    "type": "object",
    "properties": {
      "method": {
        "type": "string",
        "enum": ["start", "finish", "status"],
        "description": "Phase operation"
      },
      "phase_id": {
        "type": "string",
        "description": "Phase identifier (required for start)"
      },
      "message": {
        "type": "string",
        "description": "Completion message (optional for finish)"
      }
    },
    "required": ["method"]
  }
}
```

#### exo-task-ops

```json
{
  "name": "exo-task-ops",
  "tags": ["exosuit", "method-dispatch", "lifecycle"],
  "modelDescription": "Manages task lifecycle: add tasks, mark complete, list tasks, or update task properties.\n\n**Use this when**: User says 'complete task X' or 'list all tasks' or 'update task priority' or you need full task management operations.\n\n**Do NOT use when**: You just want to add a task to the current phase (use exo-add-task for convenience) or view phase details (use exo-phase).\n\n**Requires method parameter to specify operation.**\n\n[PROPOSED - NOT YET IMPLEMENTED]",
  "inputSchema": {
    "type": "object",
    "properties": {
      "method": {
        "type": "string",
        "enum": ["add", "complete", "list", "update"],
        "description": "Task operation"
      },
      "task_id": {
        "type": "string",
        "description": "Task identifier (required for complete/update)"
      },
      "phase_id": {
        "type": "string",
        "description": "Phase identifier (optional, defaults to current)"
      }
    },
    "required": ["method"]
  }
}
```

### Advanced Tools

#### exosuit

```json
{
  "name": "exosuit",
  "tags": ["exosuit", "mega-tool", "advanced"],
  "modelDescription": "Multi-purpose tool for advanced operations: run recipes, locate artifacts, edit configuration, or execute complex workflows not covered by specialized tools.\n\n**Use this when**: No specialized tool exists for the operation, or following a `steering.next_call` suggestion that specifies this tool, or executing recipes.\n\n**Do NOT use when**: A specialized tool exists (prefer exo-status, exo-phase-ops, etc. for clarity), or you're unsure what operation to perform (use exo-steering first).\n\n**Requires operation parameter for dispatch.**",
  "inputSchema": {
    "type": "object",
    "properties": {
      "operation": {
        "type": "string",
        "enum": ["run", "locate", "edit", "use", "list"],
        "description": "Operation type"
      },
      "target": { "type": "string", "description": "Target for operation" },
      "params": {
        "type": "object",
        "description": "Operation-specific parameters"
      }
    },
    "required": ["operation"]
  }
}
```

### User Ergonomics

#### Direct Invocation

Users can reference tools by name:

- "Use `exo-phase` to show me the current phase"
- "Start the next phase with `exo-phase-ops`"
- "Check `exo-inbox` for any pending feedback"

**Design Implication**: Names must be memorable and speakable.

#### Implicit Invocation

Users express intent without naming a tool:

- "What's the plan?" → Agent chooses `exo-plan`
- "Start phase 110" → Agent chooses `exo-phase-ops` with method `start`

**Design Implication**: Intent catalog must be exhaustive; descriptions must be discriminating.

#### Disambiguation

When confused, users can clarify:

- "I meant status, not steering"
- "Show me the phase details, not the plan"

**Design Implication**: Tool names should be distinct when spoken aloud.

### Agent Ergonomics

#### Selection Heuristics

Agents should follow this decision tree:

1. **Is this a mutation?** → Use lifecycle/capture tools
2. **Is this a simple orientation?** → Use zero-arg tools (status, phase, plan)
3. **Does user need options/guidance?** → Use `exo-steering`
4. **Is this session bootstrap?** → Use `exo-context`
5. **None of the above?** → Use `exosuit` mega-tool

#### Description Quality

Every tool description must:

- State what the tool returns (not just what it does)
- Include "Use this when" guidance
- Include "Do NOT use when" anti-patterns
- Specify required vs optional parameters

## Success Criteria

### Measurable Outcomes

1. **Coverage**: Every intent in the catalog maps to exactly one tool
2. **Disambiguation**: No two tools have overlapping "Use this when" descriptions
3. **Pronounceability**: All tool names pass the "say it in a sentence" test
4. **Agent Accuracy**: In testing, agents choose correct tool >95% of the time

### Validation Process

1. Create test suite of user intents (50+ examples)
2. For each intent, verify exactly one tool is appropriate
3. Have agents select tools without prior context
4. Measure accuracy and identify confusion points

## Implementation Roadmap

### Phase 1: Renames and Descriptions

- Rename `exo-map` to `exo-steering`
- Update all tool descriptions to follow template
- Add "Do NOT use when" guidance to all tools

### Phase 2: New Lifecycle Tools

- Implement `exo-phase-ops` (start, finish, status)
- Implement `exo-task-ops` (add, complete, list, update)

### Phase 3: Extended Coverage

- Implement `exo-epoch` (epoch context)
- Implement `exo-plan-ops` (plan modification)

### Phase 4: Validation

- Create intent test suite
- Measure agent accuracy
- Iterate on descriptions

## Alternatives Considered

### Alternative: Verb-Based Names

Instead of `exo-phase-ops`, use `exo-start-phase`, `exo-finish-phase`, etc.

**Rejected because**:

- Proliferates tool count (each verb is a separate tool)
- Harder to discover all phase-related operations
- Method dispatch is more extensible

### Alternative: Keep `exo-map` Name

The "GPS" metaphor is intuitive.

**Rejected because**:

- "Map" overlaps with "status" semantically
- "Steering" better conveys the AI-assisted navigation aspect
- "Map" sounds static; "steering" sounds dynamic

### Alternative: Merge Lifecycle Tools into Mega-Tool

Use `exosuit` for all mutations.

**Rejected because**:

- Poor discoverability for common operations
- Users can't reference specific operations by name
- Agent confusion about which operation to choose

## Open Questions

1. Should `exo-add-task` be deprecated in favor of `exo-task-ops add`?
2. Should there be an `exo-rfc-ops` for RFC lifecycle management?
3. Is `exo-epoch` high-enough frequency to warrant a dedicated tool?

