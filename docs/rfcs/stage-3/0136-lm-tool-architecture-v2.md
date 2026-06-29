<!-- exo:136 ulid:01kg5kp2hr3b7dneekkkekyvh4 -->

# RFC 136: LM Tool Architecture v2


# RFC 0136: LM Tool Architecture v2

- **Supersedes**: RFC 0083

---

## Summary

This RFC proposes a **layered architecture** for Exosuit's Language Model tools that separates _declarative metadata_ (what models see) from _implementation_ (code loaded on invoke). The architecture organizes 60+ operations into three layers:

1. **Core Navigation**: 5-6 always-available orientation tools
2. **High-Frequency Mutations**: 3-4 context-gated convenience tools
3. **ToolSets**: Grouped operations using VS Code's `languageModelToolSets` API

**Notable omission**: The current `exosuit` mega-tool is **removed**, not relegated to fallback. Its complexity confused models, and ToolSets provide complete coverage. If a router pattern proves necessary in practice, it can be restored.

This design reduces visible tool noise, improves model selection accuracy, and enables progressive disclosure of capabilities.

## Motivation

### The Current State

Exosuit's Language Model tooling has grown organically into an inconsistent state:

| Metric                            | Current State                   | Problem                                 |
| --------------------------------- | ------------------------------- | --------------------------------------- |
| **Declared tools** (package.json) | 9                               | Models can see and reason about these   |
| **Runtime-registered tools**      | 60+                             | Invisible to models, completely useless |
| **Tool selection accuracy**       | ~60%                            | Models frequently choose wrong tools    |
| **Mega-tool schema**              | Complex `oneOf` with 5 variants | Models struggle with schema complexity  |

### Why This Matters

**Models can only use what they can see.** VS Code's Language Model Tool API splits into two parts:

1. **Declarative Registration** (package.json `contributes.languageModelTools`): Metadata visible to models during tool selection
2. **Imperative Registration** (`vscode.lm.registerTool`): Code loaded at runtime

The current architecture registers most tools imperatively, making them invisible to models. When a user says "start the next phase", the model has no idea that `exo-phase-start` exists—it only sees the 9 declared tools.

### The Cost of Invisibility

Consider what happens when a user asks "mark task X complete":

1. Model scans 9 visible tools
2. None match exactly—`exo-add-task` adds tasks, `exosuit` has an `edit` operation
3. Model either guesses wrong or falls back to chat without tool use
4. The actual `exo-task-complete` tool exists at runtime but is invisible

This invisibility wastes 60+ carefully implemented operations.

### The `exosuit` Mega-Tool Problem

The `exosuit` tool was designed as a "universal router" with a complex schema:

```json
{
  "oneOf": [
    { "required": ["list"], ... },
    { "required": ["run"], ... },
    { "required": ["locate"], ... },
    { "required": ["edit"], ... },
    { "required": ["use"], ... }
  ]
}
```

Models struggle with `oneOf` schemas. They must:

1. Parse 5 variant shapes
2. Determine which variant matches the user's intent
3. Construct the correct nested object

The result: frequent schema violations and incorrect variant selection.

## Design

### Core Insight: Metadata vs. Implementation

VS Code's LM Tool API supports **lazy activation**:

```json
{
  "activationEvents": ["onLanguageModelTool:exo-phase-start"]
}
```

This means we can:

- **Declare** 30 tools in package.json (models see rich metadata)
- **Load** implementation only when a tool is actually invoked
- **Share** implementation across tools (one handler, multiple entry points)

The key is treating tool declarations as _contracts_ that inform model selection, not as 1:1 mappings to implementation classes.

### Layer 1: Core Navigation (Always Available)

Five read-only tools for project orientation. These are always visible because orientation is always relevant.

| Tool           | Purpose               | Model Description (Key Phrase)                                          |
| -------------- | --------------------- | ----------------------------------------------------------------------- |
| `exo-status`   | Quick status check    | "Returns current phase, active tasks, and singular next step"           |
| `exo-plan`     | Project roadmap       | "Returns high-level plan, epoch structure, and health metrics"          |
| `exo-phase`    | Current phase details | "Returns task breakdown, walkthrough, and artifacts for current phase"  |
| `exo-context`  | Full context dump     | "Returns canonical context for session handoff or recovery"             |
| `exo-steering` | GPS-style navigation  | "Returns multiple next actions with confidence scores and repair paths" |

**Disambiguation Rule**: Each tool answers exactly one question:

- `exo-status`: "Where am I?" (snapshot)
- `exo-plan`: "What's the big picture?" (zoom out)
- `exo-phase`: "What's in this phase?" (zoom in)
- `exo-context`: "What happened before?" (historical)
- `exo-steering`: "What should I do next?" (navigation)

### Layer 2: High-Frequency Mutations (Context-Gated)

Convenience tools for common mutations. Gated by context keys to reduce noise when irrelevant.

| Tool           | Purpose                   | When Clause                  |
| -------------- | ------------------------- | ---------------------------- |
| `exo-idea`     | Add to backlog            | `exosuit.projectInitialized` |
| `exo-add-task` | Add task to current phase | `exosuit.hasActivePhase`     |
| `exo-inbox`    | Check pending intents     | `exosuit.projectInitialized` |

**Context Key Benefits**:

- Tools hidden when project not initialized (no noise in non-Exosuit workspaces)
- `exo-add-task` hidden when no active phase (can't add tasks to nothing)
- Reduces cognitive load for models during tool selection

### Layer 3: ToolSets (Grouped Operations)

VS Code's proposed `languageModelToolSets` API groups related tools semantically. This is the key to exposing 60+ operations without overwhelming models.

```json
{
  "contributes": {
    "languageModelToolSets": [
      {
        "name": "exo-project",
        "displayName": "Project Lifecycle",
        "description": "Phase and epoch management: start/finish phases, create/close epochs",
        "tools": [
          "exo-phase-start",
          "exo-phase-finish",
          "exo-epoch-add",
          "exo-epoch-close",
          "exo-epoch-list"
        ]
      },
      {
        "name": "exo-governance",
        "displayName": "Project Governance",
        "description": "Axioms, council decisions, and operational modes",
        "tools": [
          "exo-axiom-check",
          "exo-axiom-add",
          "exo-mode-set",
          "exo-council-record"
        ]
      },
      {
        "name": "exo-tasks",
        "displayName": "Task Management",
        "description": "Manage tasks within the current phase: mark complete, remove, reorder",
        "tools": [
          "exo-task-complete",
          "exo-task-remove",
          "exo-task-reorder",
          "exo-task-update"
        ]
      },
      {
        "name": "exo-context-ops",
        "displayName": "Context Operations",
        "description": "Read and write context artifacts: walkthrough, artifacts, logs",
        "tools": [
          "exo-walkthrough-append",
          "exo-artifact-add",
          "exo-log-append"
        ]
      },
      {
        "name": "exo-rfc",
        "displayName": "RFC Lifecycle",
        "description": "Manage RFCs: create, promote stages, withdraw, update status",
        "tools": [
          "exo-rfc-create",
          "exo-rfc-promote",
          "exo-rfc-withdraw",
          "exo-rfc-update"
        ]
      },
      {
        "name": "exo-plan-ops",
        "displayName": "Plan Modifications",
        "description": "Modify the project plan: add phases, reorder, update metadata",
        "tools": ["exo-plan-add-phase", "exo-plan-reorder", "exo-plan-update"]
      },
      {
        "name": "exo-discovery",
        "displayName": "Discovery & Listing",
        "description": "List and locate project artifacts: tasks, recipes, ports, RFCs",
        "tools": ["exo-list-tasks", "exo-list-artifacts", "exo-locate"]
      }
    ]
  }
}
```

**How ToolSets Work**:

1. Model sees ToolSet description: "Task Management: Manage tasks within the current phase"
2. Model determines user intent matches the ToolSet
3. Model selects specific tool from the set: `exo-task-complete`

This provides progressive disclosure—models scan 6-7 ToolSet descriptions, not 30 individual tools.

### Why No Router Fallback

The current `exosuit` mega-tool is **removed entirely**, not relegated to fallback status.

**Rationale**:

1. **Complexity hurts more than it helps**: The `oneOf` schema with 5 variants consistently confuses models
2. **ToolSets provide complete coverage**: Every operation has a home in a ToolSet
3. **YAGNI**: The "ticket-based execution" and "dynamic operations" use cases are speculative
4. **Simpler mental model**: Users and models learn "use ToolSets", not "use ToolSets, except sometimes use exosuit"

**What about operations that don't fit?**

- **Listing operations** → `exo-discovery` ToolSet with `exo-list-*` tools
- **Locate operations** → `exo-locate` tool in `exo-discovery`
- **Rare admin tasks** → Individual tools, even if rarely used

**Restoration path**: If a router pattern proves genuinely necessary after deployment, it can be added back with a simpler schema (single `operation` string + `input` object). But we start without it.

### Model Description Standards

Every tool must follow this description template:

```
[One sentence: What this tool returns/does]

**Use this when**: [specific trigger conditions]

**Do NOT use when**: [disambiguation from similar tools]

**Zero arguments required.** (if applicable)
```

Example for `exo-steering`:

```
Returns AI-scored steering with multiple next action options, confidence scores, repair paths, and blockers.

**Use this when**: User is stuck, asks "what should I do next?" with uncertainty, or you need to evaluate multiple workflow options.

**Do NOT use when**: A simple status check suffices (use exo-status) or you need phase structure (use exo-phase).

**Zero arguments required.**
```

## Implementation Plan

### P0: Model Description Rewrite (Immediate)

| Task                                             | Effort   | Impact                                     |
| ------------------------------------------------ | -------- | ------------------------------------------ |
| Rewrite all 9 tool `modelDescription` fields     | 2 hours  | High: immediate disambiguation improvement |
| Add "Use this when" / "Do NOT use when" sections | Included | High: explicit steering                    |
| Rename `exo-map` → `exo-steering`                | 30 min   | Medium: clearer metaphor                   |

### P1: ToolSets Implementation

| Task                                        | Effort  | Impact                          |
| ------------------------------------------- | ------- | ------------------------------- |
| Define 5 ToolSets in package.json           | 2 hours | High: exposes hidden operations |
| Declare individual tools within ToolSets    | 4 hours | High: makes tools visible       |
| Implement lazy activation handlers          | 4 hours | Medium: code organization       |
| Add context keys for conditional visibility | 2 hours | Medium: reduces noise           |

### P2: Cleanup

| Task                                            | Effort  | Impact                   |
| ----------------------------------------------- | ------- | ------------------------ |
| Remove runtime tool spam (keep core + toolsets) | 2 hours | Medium: cleaner codebase |
| Consolidate tool handlers                       | 4 hours | Medium: maintainability  |
| Simplify `exosuit` schema to fallback-only      | 2 hours | Low: better but optional |

### P3: Validation

| Task                                | Effort  | Impact                      |
| ----------------------------------- | ------- | --------------------------- |
| Create tool selection test suite    | 4 hours | High: regression prevention |
| Measure selection accuracy pre/post | 2 hours | High: validates success     |
| Document tool taxonomy in manual    | 2 hours | Medium: team alignment      |

## Success Metrics

| Metric                           | Current  | Target | How to Measure                   |
| -------------------------------- | -------- | ------ | -------------------------------- |
| **Visible tools**                | 9        | 25-30  | Count package.json declarations  |
| **Tool selection accuracy**      | ~60%     | >90%   | Sample 50 intent→tool mappings   |
| **Schema errors**                | Frequent | Rare   | Monitor tool invocation failures |
| **Runtime-only tools**           | 60+      | <10    | Count imperative registrations   |
| **User disambiguation requests** | Common   | Rare   | Track "which tool?" questions    |

## Migration Path

### Phase 1: Additive (No Breaking Changes)

1. Add ToolSets alongside existing tools
2. Add new individual tool declarations
3. Update model descriptions
4. Existing tools continue working

### Phase 2: Deprecation

1. Add deprecation notices to redundant tools
2. Update documentation to prefer new patterns
3. Monitor usage metrics

### Phase 3: Removal

1. Remove deprecated runtime registrations
2. Simplify `exosuit` schema
3. Archive migration documentation

## Drawbacks

1. **Package.json Bloat**: Declaring 30 tools increases manifest size. Mitigated by VS Code's lazy loading.

2. **ToolSet API Stability**: `languageModelToolSets` is proposed, not stable. Mitigated by feature detection and fallback.

3. **Coordination Overhead**: More tools means more names to coordinate. Mitigated by strict naming conventions.

4. **Context Key Maintenance**: `when` clauses require context key updates. Mitigated by integration tests.

## Alternatives Considered

### Alternative 1: Pure Router Architecture

Keep only the `exosuit` mega-tool and improve its schema.

**Rejected because**: Models fundamentally struggle with complex `oneOf` schemas. No amount of description improvement fixes the schema parsing problem.

### Alternative 2: Flat Tool List (No ToolSets)

Declare all 60 tools individually without grouping.

**Rejected because**: Too much noise. Models would scan 60 descriptions, reducing selection accuracy through cognitive overload.

### Alternative 3: AI-Generated Tool Selection

Use a pre-processing step to select tools before model invocation.

**Rejected because**: Adds latency, complexity, and potential for misalignment between selector and executor.

## Unresolved Questions

1. **ToolSet UX**: How do models actually interact with ToolSets? Do they see the set description or individual tool descriptions first?

2. **Context Key Granularity**: How fine-grained should visibility gating be? Per-phase-type? Per-user-role?

3. **Fallback Behavior**: When a ToolSet tool fails, should we automatically suggest the `exosuit` router?

4. **Cross-Workspace Tools**: Some tools (like `exo-idea`) might be useful even without a full Exosuit project. How do we handle partial initialization?

## Design Consideration: User-Facing Groups vs. ToolSets (2026-02-03)

### Orthogonal Concerns

There are two separate grouping mechanisms that should not be conflated:

1. **User-Facing Groups**: How tools are organized in the VS Code tool selector UI for user enablement/disablement
2. **ToolSets (Model-Facing)**: How tools are grouped for progressive disclosure to the LLM

These are orthogonal! A user could enable the "Planning" group in the UI, and within that group, some tools might be lazy-loaded into the model's context based on workflow state.

### Proposed User-Facing Groups (SOAR Model)

The tool groups align with the **SOAR loop** (Status → Orient → Act → Review), a workflow cycle inspired by Boyd's OODA loop but adapted for human-AI collaboration. See `.github/copilot-instructions.md` for the full SOAR specification.

| Group      | SOAR Phase   | Purpose                               | Tools                                                   |
| ---------- | ------------ | ------------------------------------- | ------------------------------------------------------- |
| **Status** | Status       | Detect current state, drift from plan | `exo-status`, `exo-phase`, `exo-list-tasks`             |
| **Orient** | Orient       | Synthesize options, steering          | `exo-steering`, `exo-context`, `exo-goal-list`          |
| **Act**    | Act          | Execute tasks, TDD, implementation    | `exo-task-*`, `exo-add-task`, `exo-tdd-*`, `exo-impl-*` |
| **Plan**   | (orthogonal) | Strategic: RFCs, epochs, roadmap      | `exo-rfc-*`, `exo-epoch-*`, `exo-plan`                  |

**Note**: **Review** is a phase in the SOAR loop but not a tool group—it's powered by verification tooling (`exo-verify`) and human judgment, feeding back into the next Status check.

Within each user-facing group, individual tools may still be lazy-loaded based on:

- Current mode (e.g., TDD mode activates `exo-tdd-*`)
- Workflow state (e.g., RFC tools only when in RFC workflow)
- Context keys (e.g., `exosuit.hasActivePhase`)

### Tool Dependency Coherence

**Problem**: If `exo-steering` suggests "use `exo-tdd-start` to begin the TDD cycle" but the user has disabled the Execute group, the agent gets confused and may hallucinate or fail.

**Solution**: Declare tool dependencies and validate coherence at configuration time.

#### Dependency Types

| Type              | Example                                 | Behavior                                 |
| ----------------- | --------------------------------------- | ---------------------------------------- |
| **Hard**          | `exo-phase-finish` requires `exo-phase` | Error if violated; prevent configuration |
| **Soft/Steering** | `exo-steering` references `exo-tdd-*`   | Warning + steering message adjustment    |
| **Mode-coupled**  | TDD mode activates `exo-tdd-*`          | Auto-enable when mode active             |

#### Coherence Validation

On startup and configuration change:

1. **Build dependency graph** from tool metadata
2. **Check enabled set** forms a coherent subset
3. **Surface violations**:
   - **Toast notification**: "⚠️ Steering tools enabled but Execute tools disabled—steering suggestions may reference unavailable tools"
   - **High-priority steering message**: "Configuration issue: I can suggest next steps but cannot execute them. Consider enabling the Execute tool group."

#### Metadata Schema

```json
{
  "languageModelTools": [
    {
      "name": "exo-steering",
      "dependencies": {
        "soft": ["exo-tdd-start", "exo-task-complete", "exo-phase-finish"],
        "description": "Steering may suggest these tools as next actions"
      }
    },
    {
      "name": "exo-phase-finish",
      "dependencies": {
        "hard": ["exo-phase"],
        "description": "Cannot finish a phase without reading phase state"
      }
    }
  ]
}
```

This metadata enables deterministic identification of user configuration errors and makes fixing them a high priority through both UI feedback (toasts) and agent feedback (steering messages).

### SOAR → GitHub Alignment

The SOAR loop maps naturally onto a GitHub-centric workflow:

```
SOAR Phase    │  GitHub Artifact
──────────────┼───────────────────
Status        │  Branch state, PR status, CI results
Orient        │  PR description, review comments, steering
Act           │  Commits, code changes
Review        │  PR review, CI verification
──────────────┼───────────────────
(Loop closes) │  PR merge → new Status on main
```

**Key insight**: A complete SOAR cycle often culminates in a PR. The PR itself is a Review artifact—it captures what was done and invites verification. Merging the PR closes the loop and triggers a new Status check on the updated main branch.

**Implications for tooling**:

- `exo-status` should surface PR state when on a feature branch
- `exo-steering` should suggest "open PR" when Act phase is complete
- Review phase tools should integrate with GitHub review workflows
- Phase completion could auto-suggest PR creation

This alignment means the SOAR loop isn't just an abstract workflow—it has concrete GitHub artifacts at each stage, making progress visible and auditable.

## Future Possibilities

1. **Dynamic ToolSets**: Generate ToolSets based on project state (e.g., RFC tools only visible when in RFC workflow)

2. **Tool Composition**: Allow chaining tools declaratively (e.g., `exo-phase-start` automatically calls `exo-context` after)

3. **Learning Loop**: Track tool selection patterns to improve descriptions over time

4. **External Tools**: Allow external extensions to contribute to Exosuit ToolSets

---

## Appendix A: Complete Tool Inventory

### Layer 1: Core Navigation

| Tool           | Args | Description                     |
| -------------- | ---- | ------------------------------- |
| `exo-status`   | none | Current phase, tasks, next step |
| `exo-plan`     | none | Roadmap, epochs, health         |
| `exo-phase`    | none | Phase details, task breakdown   |
| `exo-context`  | none | Full context dump               |
| `exo-steering` | none | Navigation options, confidence  |

### Layer 2: High-Frequency Mutations

| Tool           | Args                       | Description          |
| -------------- | -------------------------- | -------------------- |
| `exo-idea`     | title, description?, tags? | Add to backlog       |
| `exo-add-task` | id, label?                 | Add task to phase    |
| `exo-inbox`    | none                       | Pending user intents |

### Layer 3: ToolSet Members

**exo-project** (Phase/Epoch Lifecycle):

- `exo-phase-start`: Start a phase by ID
- `exo-phase-finish`: Finish current phase with message
- `exo-epoch-add`: Create new epoch
- `exo-epoch-close`: Close current epoch
- `exo-epoch-list`: List all epochs

**exo-governance** (Project Governance):

- `exo-axiom-check`: Verify axioms are satisfied
- `exo-axiom-add`: Add new axiom
- `exo-mode-set`: Set operational mode
- `exo-council-record`: Record council decision

**exo-tasks** (Task Management):

- `exo-task-complete`: Mark task complete
- `exo-task-remove`: Remove task from phase
- `exo-task-reorder`: Change task order
- `exo-task-update`: Update task metadata

**exo-context-ops** (Context Read/Write):

- `exo-walkthrough-append`: Add to walkthrough
- `exo-artifact-add`: Register artifact
- `exo-log-append`: Add to phase log

**exo-rfc** (RFC Lifecycle):

- `exo-rfc-create`: Create new RFC
- `exo-rfc-promote`: Promote RFC stage
- `exo-rfc-withdraw`: Withdraw RFC
- `exo-rfc-update`: Update RFC metadata

**exo-plan-ops** (Plan Modification):

- `exo-plan-add-phase`: Add phase to plan
- `exo-plan-reorder`: Reorder phases
- `exo-plan-update`: Update plan metadata

**exo-discovery** (Listing & Location):

- `exo-list-tasks`: List tasks in current phase
- `exo-list-artifacts`: List project artifacts
- `exo-locate`: Locate canonical files/directories

---

## Appendix B: Context Keys

| Key                          | Type    | Set When                         |
| ---------------------------- | ------- | -------------------------------- |
| `exosuit.projectInitialized` | boolean | exosuit.toml exists and is valid |
| `exosuit.hasActivePhase`     | boolean | plan.toml has active phase       |
| `exosuit.hasPendingInbox`    | boolean | inbox.toml has unread items      |
| `exosuit.inRfcWorkflow`      | boolean | Current phase is RFC-related     |
| `exosuit.hasRunningTask`     | boolean | Background task is executing     |

---

## Appendix C: ToolSets API Stability Assessment

The `languageModelToolSets` API is marked as "proposed" but has strong stability indicators:

| Indicator         | Status       | Notes                                          |
| ----------------- | ------------ | ---------------------------------------------- |
| MCP Integration   | ✅ Active    | MCP servers use ToolSets for all their tools   |
| Schema stability  | ✅ Stable    | No breaking changes in recent VS Code releases |
| Core team usage   | ✅ Yes       | VS Code's own tools use ToolSets internally    |
| Feature detection | ✅ Available | `isProposedApiEnabled()` check                 |

**Risk Mitigation**: If the API changes or is removed:

1. Feature detection allows graceful fallback to individual tools
2. Individual tools remain functional (just ungrouped)
3. The migration path is additive, not destructive

**Recommendation**: Proceed with ToolSets as P1. The stability evidence outweighs the "proposed" label risk.

---

## Implementation Note: CommandSpec Source (2026-02-02, updated 2026-02-05)

This RFC assumes CommandSpec is available for tool schema generation. The source of CommandSpec is being unified:

### Current State

CommandSpec is currently generated at runtime via `exo schema generate`, which reads `Command::args()` trait implementations. This works but creates a dual-source problem (Clap definitions + `args()` implementations can drift).

### Target State (RFC 00233)

CommandSpec will be defined **inline** using Clap annotations extended with `#[exo(...)]` custom attributes. A proc-macro (`ExoSpec`) will extract the complete CommandSpec at compile time, eliminating the dual-source problem.

**Implications for this RFC**:

- **Tool schema generation**: Schemas will be derived from the compile-time `command-spec.json` artifact
- **Parity guarantees**: Single-source definition eliminates drift between CLI and LM tool schemas
- **Manual mapping lists eliminated**: `LIFECYCLE_OPERATIONS` and similar TypeScript lists will be replaced by artifact-driven registration
- **No runtime introspection needed**: CommandSpec will be available as a static artifact

**See Also**: [RFC 00233: ExoSpec — Unified Command Definition](../stage-1/00233-exospec-unified-command-definition-and-the-end-of-dual-source-drift.md) for the consolidated design and migration plan.
