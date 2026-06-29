<!-- exo:182 ulid:01kmzxeffky7vm9pqa6b0srmtf -->

# RFC 182: Zero-Arg Tool Completeness


# RFC 00182: Zero-Arg Tool Completeness

## Summary

Complete the zero-arg tool inventory by implementing missing tools referenced in workflow documentation and promoting key discovery tools to agent-fundamental tier. This ensures the agent workflow has all necessary orientation and lifecycle tools.

## Motivation

### The Anchor Command Problem (from RFC 0083)

Agents need reliable entry points when:

- Starting a fresh session
- Recovering from errors
- Reorienting after context loss
- Performing handoffs between agents

These "anchor commands" should be:

- **Zero-parameter** (no failure modes from malformed input)
- **Pure/read-only** (safe to call repeatedly)
- **High-signal** (return actionable context, not noise)

### Current State

RFC 0136 defines the Core Navigation set (`exo-status`, `exo-plan`, `exo-phase`, `exo-context`, `exo-steering`), but several workflow-critical tools are missing:

- Strike workflow references `exo-strike-status` but no such tool exists
- Phase finish ceremony needs `exo-verify` and `exo-commit-status`
- Quick-context tools (`exo-rfc`, `exo-goal`) don't exist
- `exo-epoch` for current epoch context (vs `exo-epoch-list` for discovery)

### The Problem

Agents must work around missing tools by:

1. Using list tools when they need single-item context
2. Inferring verification status from other signals
3. Missing workflow steps that require non-existent tools

### The Opportunity

Completing the zero-arg tool inventory enables:

1. Cleaner agent workflows with purpose-built tools
2. Better steering (each tool can provide focused recommendations)
3. Full coverage of the agent workflow lifecycle

## Alignment with Existing Architecture

### Related RFCs

| RFC   | Stage | Relevance                                                                         |
| ----- | ----- | --------------------------------------------------------------------------------- |
| 0083  | 3     | **Superseded by 0136** for tool surface; anchor command principles preserved here |
| 0136  | 3     | **Canonical** LM Tool Architecture; Core Navigation + ToolSets model              |
| 00181 | 0     | Multipart responses; these tools will use that pattern                            |

### Preserved Principles from RFC 0083

1. **Orientation Tools Never Error**: Zero-arg tools always return current state, even if empty — guarantees agents always have a recovery path
2. **Zero-Arg Qualification**: A tool qualifies for zero-arg if: (a) effect is `pure`, (b) all parameters are optional, (c) operation is idempotent

### Tool Classification (from RFC 00181)

| Tool Type             | Steering Purpose           | Examples                   |
| --------------------- | -------------------------- | -------------------------- |
| **Agent-fundamental** | What action to take next   | `exo-status`, `exo-verify` |
| **User-convenience**  | How to communicate results | `exo-rfc`, `exo-goal`      |

### Mapping to RFC 0136 Layers

| New Tool            | RFC 0136 Layer  | Rationale                  |
| ------------------- | --------------- | -------------------------- |
| `exo-verify`        | Core Navigation | Essential for VERIFY stage |
| `exo-commit-status` | Core Navigation | Essential for phase finish |
| `exo-epoch`         | Core Navigation | Current epoch context      |
| `exo-strike-status` | Core Navigation | Strike workflow status     |
| `exo-rfc`           | High-Frequency  | RFC context on demand      |
| `exo-goal`          | High-Frequency  | Goal context on demand     |

## Detailed Design

### New Tools

#### `exo-verify` (Agent-fundamental, Action steering)

Run verification checks for the current phase.

```
exo verify [--fix]
```

**Zero-arg behavior**: Run all verifiers, report pass/fail status.

**Steering**: On failure, recommend specific fix actions. On success, recommend proceeding to next phase step.

#### `exo-commit-status` (Agent-fundamental, Action steering)

Check git working tree status for phase finish ceremony.

```
exo commit status
```

**Zero-arg behavior**: Report clean/dirty status with uncommitted file list.

**Steering**: If dirty, recommend `git add` + `git commit`. If clean, recommend `exo-phase-finish`.

#### `exo-epoch` (Agent-fundamental, Action steering)

Get current epoch context (not the list of all epochs).

```
exo epoch [--id <id>]
```

**Zero-arg behavior**: Return active epoch with goals, phases, and progress.

**Steering**: Recommend next phase or goal based on epoch state.

#### `exo-strike-status` (Agent-fundamental, Action steering)

Get current strike workflow status.

```
exo strike status
```

**Zero-arg behavior**: Return active strike with goal, progress, and blockers.

**Steering**: Recommend `exo-strike-finish` or `exo-strike-abort` based on state.

#### `exo-rfc` (Agent-fundamental, Communication steering)

Get context for a specific RFC or the most relevant RFC.

```
exo rfc [<id>] [--related]
```

**Zero-arg behavior**: Return the RFC most relevant to current work (based on active phase/goal).

**Steering**: Suggest how to present RFC context to user.

#### `exo-goal` (Agent-fundamental, Communication steering)

Get current goal context (not the list of all goals).

```
exo goal [--id <id>]
```

**Zero-arg behavior**: Return active goal with tasks and progress.

**Steering**: Suggest how to present goal progress to user.

### Tier Promotions

Promote these existing tools from user-convenience to agent-fundamental:

| Tool            | Current Tier     | New Tier          | Rationale                         |
| --------------- | ---------------- | ----------------- | --------------------------------- |
| `exo-rfc-list`  | User-convenience | Agent-fundamental | Frequently needed for RFC context |
| `exo-goal-list` | User-convenience | Agent-fundamental | Central to current workflow       |

## Implementation Plan (Stage 2)

### Phase 1: Core Workflow Tools

- [ ] Implement `exo-verify` (wraps existing `exo verify` command)
- [ ] Implement `exo-commit-status` (wraps `git status`)
- [ ] Add zero-arg tool registration for both

### Phase 2: Context Tools

- [ ] Implement `exo-epoch` (current epoch, not list)
- [ ] Implement `exo-strike-status` (current strike state)
- [ ] Add zero-arg tool registration for both

### Phase 3: Discovery Tools

- [ ] Implement `exo-rfc` (relevant RFC context)
- [ ] Implement `exo-goal` (current goal context)
- [ ] Promote `exo-rfc-list` and `exo-goal-list` to agent-fundamental

### Phase 4: Integration

- [ ] Update tool-factory.ts to handle new tools
- [ ] Add multipart response support (per RFC 00181)
- [ ] Update documentation

## Context Updates (Stage 3)

- [ ] Update `docs/manual/features/lm-tools.md` with new tools
- [ ] Update RFC 0083 to mark tools as implemented
- [ ] Add examples to tool documentation

## Drawbacks

1. **More tools to maintain** — Each tool needs implementation, tests, and documentation.
2. **Potential overlap** — `exo-epoch` vs `exo-epoch-list`, `exo-goal` vs `exo-goal-list` may confuse users.
3. **Steering complexity** — Each tool needs appropriate steering logic.

## Alternatives

### Alternative A: Extend Existing Tools

Add optional parameters to existing tools instead of new tools. Rejected because:

- Violates zero-arg principle for user convenience
- Makes tool behavior less predictable

### Alternative B: Single "Context" Tool

Create one `exo-context` tool with subcommands. Rejected because:

- Already exists (`exo-context` dumps full context)
- Loses focused steering per tool

## Unresolved Questions

1. **RFC relevance algorithm** — How does `exo-rfc` determine "most relevant" RFC?
2. **Strike detection** — How does `exo-strike-status` know if a strike is active?

## Future Possibilities

1. **Tool aliases** — Allow `exo epoch` to work as both command and zero-arg tool
2. **Contextual defaults** — Tools auto-detect relevant context from git branch, active phase, etc.
3. **Tool composition** — Combine multiple tool outputs for richer orientation

