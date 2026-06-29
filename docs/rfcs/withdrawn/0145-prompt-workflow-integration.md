<!-- exo:145 ulid:01kg5kp2j6kx3c76j7wxfehzyq -->

# RFC 145: Prompt Workflow Integration


# RFC 0145: Prompt Workflow Integration

## Summary

Transform `prompts.toml` from passive infrastructure into an active workflow component by wiring prompts to phase gates, mode activation, and tool invocations.

## Motivation

### The Problem

`prompts.toml` currently serves as a prompt catalog—a central registry of templates that *could* be used. However:

1. **Prompts exist but aren't invoked**: Workflow prompts like `fresh_eyes` and `phase_transition` are defined but not systematically triggered
2. **Modes are referenced but not activated**: Prompts mention modes, but there's no runtime mode switching
3. **Axioms are listed but not enforced**: Axiom files exist in the context index but aren't checked by prompts
4. **Gap between definition and practice**: The repo history analysis identified this as a key disconnect

### The Shipping Alignment

The shipping focus emphasizes:
- **Coherent projections across surfaces**: prompts.toml enables this but doesn't guarantee it
- **Contextual steering as a thread**: Workflow prompts could encode rituals, but only if invoked
- **Repeatable rituals**: Prompts describe workflows that should be automatic

### The Opportunity

By wiring prompts to lifecycle events, we transform prompts.toml from "available guidance" to "operational infrastructure":

| Event | Prompt Triggered |
|-------|------------------|
| Phase start | `commands.phase_checkin` (new) |
| Phase finish | `commands.phase_transition` |
| Task completion | `commands.assess` |
| RFC promotion | `commands.rfc_review` (new) |
| Idea triage | `commands.idea_vetting` (new) |

## Detailed Design

### Terminology

- **Workflow Prompt**: A prompt in `[workflows]` or `[commands]` that encodes a multi-step ritual
- **Gate Prompt**: A prompt that runs automatically at a lifecycle transition
- **Active Mode**: The current collaboration style (Thinking Partner, Chief of Staff, Maker, etc.)
- **Prompt Interpolation**: Replacing `{tokens}` with runtime values

### User Experience (UX)

#### Automatic Prompt Invocation

When a user runs `exo-phase-finish`:
1. System automatically invokes `commands.phase_transition` prompt
2. Review agent verifies work
3. Execute agent commits (if approved)
4. Prepare agent audits next phase
5. User sees structured output

#### Mode-Aware Prompts

When a mode is active, prompts receive mode context:
```toml
[system]
chat = """
...
Current Mode: {activeMode}
Mode Guidelines: {modeGuidelines}
...
"""
```

The system prompt adapts to the collaboration style.

#### Axiom Injection

Key prompts include axiom checks:
```toml
[commands.rfc_review]
prompt = """
Before promoting this RFC, verify against axioms:

Workflow Axioms ({workflowAxioms}):
- [ ] Does this align with phase-based collaboration?

System Axioms ({systemAxioms}):
- [ ] Is this within scope of the project?
...
"""
```

### Architecture

#### Prompt Categories

```toml
[system]
chat = "..."              # Always-on system prompt

[workflows]
fresh_eyes = "..."        # User-invoked rituals
persona_build = "..."

[commands]
assess = "..."            # Tool-invocable prompts
phase_transition = "..."
phase_checkin = "..."     # NEW: Phase start
rfc_review = "..."        # NEW: RFC promotion
idea_vetting = "..."      # NEW: Idea triage

[gates]                   # NEW: Auto-triggered prompts
on_phase_start = "phase_checkin"
on_phase_finish = "phase_transition"
on_task_complete = "assess"
on_rfc_promote = "rfc_review"
```

#### Integration Points

```
┌──────────────────────────────────────────────────────────┐
│                    LM Tool Invocation                     │
│                  (exo-phase-finish, etc.)                 │
└──────────────────────────────────────────────────────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────────┐
│                    Gate Check                             │
│            Is there a gate prompt for this event?         │
└──────────────────────────────────────────────────────────┘
                            │ yes
                            ▼
┌──────────────────────────────────────────────────────────┐
│                 Prompt Interpolation                      │
│   - {activeMode} from modes.toml                          │
│   - {workflowAxioms} from axioms.workflow.toml            │
│   - {phaseContext} from current phase                     │
└──────────────────────────────────────────────────────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────────┐
│                   Agent Invocation                        │
│          (or inline prompt if no agent specified)         │
└──────────────────────────────────────────────────────────┘
```

#### New Interpolation Tokens

| Token | Source |
|-------|--------|
| `{activeMode}` | modes.toml → currently active mode |
| `{modeGuidelines}` | modes.toml → mode.mindset |
| `{workflowAxioms}` | axioms.workflow.toml |
| `{systemAxioms}` | axioms.system.toml |
| `{designAxioms}` | axioms.design.toml |
| `{phaseGoals}` | implementation-plan.toml → phase.goal |
| `{phaseTasks}` | implementation-plan.toml → phase.steps |
| `{linkedRfcs}` | implementation-plan.toml → phase.rfc |

### Implementation Details

#### Gate Registry

```typescript
interface GateConfig {
  event: 'phase_start' | 'phase_finish' | 'task_complete' | 'rfc_promote' | 'idea_triage';
  promptKey: string;
  agent?: string;  // Optional: invoke specific agent
  required: boolean;  // If true, blocks the action until prompt completes
}
```

#### Mode Activation

```typescript
interface ModeContext {
  name: string;
  focus: string;
  mindset: string[];
  keyDocuments: string[];
}

function getActiveMode(): ModeContext | null {
  // Read from modes.toml or session state
}
```

#### Prompt Rendering Pipeline

```
1. Load prompt template from prompts.toml
2. Determine active mode (if any)
3. Load relevant axioms
4. Load phase context
5. Interpolate all tokens
6. If agent specified, invoke agent with rendered prompt
7. Otherwise, inject into chat participant
```

## Implementation Plan (Stage 2)

- [ ] Add `[gates]` section to prompts.toml schema
- [ ] Implement gate registry in PromptService
- [ ] Add new interpolation tokens (mode, axioms, phase)
- [ ] Wire gates to LM tool lifecycle (phase-finish, etc.)
- [ ] Create gate prompts: `phase_checkin`, `rfc_review`, `idea_vetting`
- [ ] Add mode activation mechanism
- [ ] Update prompt loader to handle gates

## Context Updates (Stage 3)

- [ ] Create `docs/manual/features/prompt-gates.md`
- [ ] Update `docs/manual/features/prompts.md` with gate section
- [ ] Document new interpolation tokens
- [ ] Add gate configuration to prompts.toml template

## Drawbacks

1. **Friction**: Automatic prompts could slow down simple operations
2. **Complexity**: More moving parts in the prompt system
3. **Override Difficulty**: Users may want to skip gates
4. **Token Cost**: Interpolating axioms/modes adds tokens to every prompt

### Mitigations

- Gates can be marked `required: false` for optional checks
- Add `--skip-gates` flag to CLI commands
- Lazy-load axioms only when tokens are present in prompt

## Alternatives

### A. Manual Prompt Invocation Only
Keep prompts as opt-in. Rejected because:
- Workflow discipline depends on user memory
- Prompts remain underutilized
- Gap between definition and practice persists

### B. Hardcoded Workflow Logic
Embed workflow checks in tool code. Rejected because:
- Not customizable by users
- Violates prompt externalization principle
- Harder to iterate

### C. Separate Workflow Engine
Build a dedicated workflow orchestrator. Deferred because:
- Overkill for current needs
- Adds significant complexity
- prompts.toml can evolve toward this

## Unresolved Questions

1. **Gate Granularity**: Which lifecycle events warrant gates?
2. **Override UX**: How do users skip or customize gates?
3. **Mode Persistence**: Is the active mode session-scoped or persistent?
4. **Axiom Scope**: Which axioms apply to which prompts?

## Future Possibilities

1. **Conditional Gates**: Gate prompts run only if certain conditions are met
2. **Gate Metrics**: Track gate invocations and outcomes
3. **User-Defined Gates**: Users add custom gates to their prompts.toml
4. **Gate Templates**: Shareable gate configurations
5. **Visual Gate Editor**: UI for configuring prompt gates
