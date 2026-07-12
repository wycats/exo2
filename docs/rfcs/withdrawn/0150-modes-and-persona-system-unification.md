<!-- exo:150 ulid:01kg5kp2jfgpxrzjszswg03m27 -->

# RFC 150: Modes and Persona System Unification

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 0150: Modes and Persona System Unification

## Problem Statement

The "modes" system evolved from an earlier "persona" concept but the transition was incomplete, leaving:

- **Naming confusion**: Code references both "persona" and "mode" inconsistently
- **No runtime activation**: Modes are defined but never loaded into agent prompts
- **Overlap with steering**: `ProgressMode` (7-state machine) exists separately from collaboration modes
- **Drift between files**: Template modes differ from live `modes.toml`
- **Orphaned persona artifacts**: `EXOSUIT.md` is called "The Persona", persona-build prompts reference legacy paths

## Historical Evolution

### Original Vision: Personas

Personas were user-type evaluators for testing/validation:

- "The New User" - tests onboarding clarity
- "The Skeptic" - challenges assumptions
- Used in "Fresh Eyes" reviews

### Transition: Personas → Modes

Plan tasks show intentional transition:

1. "Develop user personas" (completed)
2. "Refactor Personas into Modes" (completed)

Modes became **collaboration styles** aligned to workflow phases:

- **Thinking Partner** (Architect) → Planning phase
- **Chief of Staff** (Manager) → Transitions, context restoration
- **Maker** (Implementer) → Execution phase

### Current Reality: Static Definitions

- `docs/agent-context/modes.toml` defines 3 modes (structured)
- `docs/design/modes.md` mirrors same content (narrative, redundant)
- `docs/manual/features/modes.md` documents them (authoritative)
- **No code path reads modes at runtime**
- Steering uses `ProgressMode` separately

## Gap Analysis

| Aspect               | Vision                             | Reality                    |
| -------------------- | ---------------------------------- | -------------------------- |
| Runtime activation   | Modes injected into prompts        | Static files, never loaded |
| Steering integration | Mode derived from ProgressMode     | Separate systems           |
| Phase alignment      | Mode switches at phase transitions | No switching logic         |
| Agent configs        | Modes influence tool availability  | No connection              |

## Proposed Unification

### 1. Map ProgressMode → Collaboration Mode

RFC 0107 already defines this mapping:

```
ProgressMode::Planning     → Thinking Partner
ProgressMode::Executing    → Maker
ProgressMode::Transitioning → Chief of Staff
```

**Implementation**: Derive active mode from steering state.

### 2. Inject Mode into Prompts

The prompt-gates RFC (10139) specifies:

```
{activeMode} → "Thinking Partner"
{modeGuidelines} → Mode-specific instructions
```

**Implementation**: Wire mode context into `buildSystemPrompt()`.

### 3. Reconcile Personas vs. Modes

**Personas**: User-type perspectives for evaluation (keep for Fresh Eyes)
**Modes**: Agent behavior states for collaboration (integrate with steering)

These are **orthogonal concepts**:

- Personas = "Who am I pretending to be?" (evaluation)
- Modes = "How should I behave?" (operation)

### 4. Consolidate Mode Sources

| Current                         | Proposed             |
| ------------------------------- | -------------------- |
| `docs/agent-context/modes.toml` | Keep (canonical)     |
| `docs/design/modes.md`          | Delete (redundant)   |
| `docs/manual/features/modes.md` | Keep (documentation) |
| Template in `bootstrap.sh`      | Align with canonical |

### 5. Mode-Aware Steering Output

```json
{
  "progress_mode": "executing",
  "collaboration_mode": {
    "id": "maker",
    "title": "The Maker (Implementer Mode)",
    "focus": "Execution, Efficiency, 'How'"
  },
  "next_actions": [...]
}
```

## Relationship to Agent Configs

`.github/agents/*.md` define specialized agents (execute, review, prepare, recon).

**Question**: Are agents and modes complementary or competing?

**Proposal**: Modes are **behavioral states** that any agent can adopt. Agents are **capability configurations** (tools, expertise). An "execute" agent in "Maker" mode focuses on implementation; in "Thinking Partner" mode, it might pause to surface tensions.

## Migration Path

1. **Delete `docs/design/modes.md`** (cleanup phase)
2. **Update persona-build prompt** to reference `modes.toml`
3. **Add mode derivation** to steering output
4. **Wire mode injection** into prompt rendering
5. **Add mode to status commands** (`exo status` shows current mode)

## Open Questions

1. Should mode switching be automatic (from steering) or explicit (`exo mode set`)?
2. Should agents have mode preferences/restrictions?
3. How do modes interact with the axiom system? (Modes as behavioral axioms?)

## Related

- RFC 0107: Coherent Workflow Model (ProgressMode definition)
- RFC 0137: Prompt Gates (mode injection spec)
- RFC 0149: Axiom System Integration (similar integration problem)
- RFC 0142/10146: Agent Ecosystem (agents vs. modes question)

