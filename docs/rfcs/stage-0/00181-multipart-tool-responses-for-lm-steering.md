<!-- exo:181 ulid:01kmzxefftacgn8wgadt6hrr80 -->

# RFC 181: Multipart Tool Responses for LM Steering


# RFC 00181: Multipart Tool Responses for LM Steering

## Summary

Refactor the machine channel's LM tool adapter to return **multipart responses** using VS Code's `LanguageModelToolResult` API—separating steering narrative (text) from structured data (typed JSON). This improves LM reasoning by providing natural language context alongside machine-readable payloads.

## Motivation

### Current State

The machine channel currently returns tool results as a single `LanguageModelTextPart` containing stringified JSON:

```typescript
return new vscode.LanguageModelToolResult([
  new vscode.LanguageModelTextPart(JSON.stringify(parsed, null, 2)),
]);
```

Steering hints (`tool`, `tool_args`) are embedded _inside_ the JSON structure, forcing the LM to:

1. Parse the JSON mentally
2. Extract the steering recommendation
3. Understand _why_ that recommendation was made

### The Problem

LLMs process natural language more effectively than nested JSON navigation. Embedding steering intent inside structured data conflates two concerns:

- **What to do next** (intent/reasoning) — best expressed in prose
- **The data to act on** (state/facts) — best expressed in typed JSON

### Two Types of Steering

Not all tools steer the same way:

| Tool Type             | Steering Purpose                   | Example Narrative                                                             |
| --------------------- | ---------------------------------- | ----------------------------------------------------------------------------- |
| **Agent-fundamental** | What action to take next           | "Run `exo-phase-start` to begin implementation"                               |
| **User-convenience**  | How to communicate results to user | "User asked about RFCs. Summarize the 3 most relevant to their current work." |

Both benefit from multipart responses — the narrative just serves different purposes.

### The Opportunity

VS Code's API already supports exactly what we need:

```typescript
class LanguageModelToolResult {
  content: Array<LanguageModelTextPart | LanguageModelDataPart | ...>;
}

class LanguageModelDataPart {
  static json(value: any, mime?: string): LanguageModelDataPart;  // defaults to 'application/json'
  static text(value: string, mime?: string): LanguageModelDataPart;
}
```

We're leaving capability on the table.

## Alignment with Existing Architecture

### Related RFCs

| RFC  | Stage | Relevance                                                                                                          |
| ---- | ----- | ------------------------------------------------------------------------------------------------------------------ |
| 0063 | 4     | Error + steering invariants; "non-ok implies steering" — multipart must honor this                                 |
| 0083 | 3     | Hybrid LM tool architecture; **superseded by 0136** for tool surface, but steering-first principles preserved here |
| 0125 | 3     | Machine Channel v1 protocol; `ResponseEnvelope` with `steering` field                                              |
| 0136 | 3     | **Canonical** LM Tool Architecture; Core Navigation + ToolSets model                                               |
| 0146 | 0     | LanguageModelToolResult / LanguageModelDataPart API mapping                                                        |
| 0154 | 0     | Steering confidence model; confidence scoring and multipliers                                                      |

### Preserved Principles from RFC 0083

While RFC 0136 supersedes 0083 for tool surface, these principles from 0083 remain foundational:

1. **Steering-First Design**: Every tool response includes guidance for what to do next via `steering.next_call`
2. **Orientation vs Mutation**: Fundamental distinction between read (safe, repeatable) and write (requires confirmation)
3. **Response Envelope**: Consistent `{ status, data, steering }` structure across all tools
4. **Orientation Tools Never Error**: Zero-arg tools always return current state, even if empty — guarantees agents always have a recovery path

### Tool Classification

Tools are classified by their role in the agent workflow:

**Agent-Fundamental (Core Loop + Orientation)**
| Tool | Role | Steering Type |
|------|------|---------------|
| `exo-status` | Primary orientation snapshot | Action |
| `exo-steering` | Navigation/next-action | Action |
| `exo-phase` | Active phase details | Action |
| `exo-plan` | Roadmap/epoch goals | Action |
| `exo-context` | Full context dump | Action |
| `exo-inbox` | Session intent check | Action |
| `exo-rfc-list` | RFC discovery and context | Communication |
| `exo-goal-list` | Goal status and relevance | Communication |
| `exo-phase-start` | Phase lifecycle | Action |
| `exo-strike-abort/finish` | Strike workflow exit | Action |

**User-Convenience (Discovery/Triage)**
| Tool | Role |
|------|------|
| `exo-list-tasks` | Task discovery (covered by `exo-phase`) |
| `exo-idea-list` | Idea discovery |
| `exo-epoch-list` | Epoch discovery (covered by `exo-plan`) |
| `exo-tdd-red/green` | TDD workflow aid |

### Out of Scope: Missing Zero-Arg Tools

The following tools are referenced in workflow documentation but don't exist as zero-arg tools. These are **out of scope** for this RFC and should be addressed in a separate RFC (00182):

| Missing Tool        | Justification                                                         | Steering Type |
| ------------------- | --------------------------------------------------------------------- | ------------- |
| `exo-verify`        | Core VERIFY stage needs canonical check runner                        | Action        |
| `exo-commit-status` | Phase finish needs clean working tree check                           | Action        |
| `exo-epoch`         | RFC 0083 defines it as orientation tool; only `exo-epoch-list` exists | Action        |
| `exo-strike-status` | Strike workflow needs status command                                  | Action        |
| `exo-rfc`           | Quick current-RFC context (vs list)                                   | Communication |
| `exo-goal`          | Quick current-goal view (vs list)                                     | Communication |

**Rationale for split**: Multipart responses are independently valuable and shouldn't be blocked on new tool implementation.

### Existing Steering Types (Rust)

The codebase uses these types (not `SteeringOption` as originally drafted):

```rust
// Machine-channel steering (tools/exo/src/machine_channel.rs)
pub struct Steering {
    pub next_call: NextCall,
    pub priority: Option<Priority>,
    pub confidence: Option<f32>,
    pub context_note: Option<String>,  // ← Narrative source for TS composition
}

// Suggested actions with rationale
pub struct SuggestedAction {
    pub label: String,
    pub command: String,
    pub rationale: String,  // ← Existing narrative field
    pub intent: WorkIntent,
    pub confidence: Option<f32>,
}

// Full steering block
pub struct SteeringBlock {
    pub primary_intent: WorkIntent,
    pub progress_mode: ProgressMode,
    pub next_actions: Vec<SuggestedAction>,
    pub repair_actions: Vec<SuggestedAction>,
    pub pending_intents: Vec<SurfacedIntent>,
}
```

**Key insight**: `context_note` on `Steering` and `rationale` on `SuggestedAction` are existing narrative fields we can leverage.

### TypeScript Protocol Mismatch

The TS `MachineChannelResponseEnvelope` type is **incomplete** compared to Rust:

```typescript
// Current TS type (packages/exosuit-vscode/src/machine-channel/types.ts)
steering?: {
  next_call: { kind: MachineChannelOpKind; params: unknown; };
};
// Missing: priority, confidence, context_note
```

**Prerequisite**: Update TS types to include `priority`, `confidence`, and `context_note` before implementing multipart responses.

## Detailed Design

### Terminology

| Term                   | Definition                                                                     |
| ---------------------- | ------------------------------------------------------------------------------ |
| **Steering Narrative** | Natural language text explaining the current state and recommended next action |
| **Structured Payload** | Typed JSON data with proper MIME type annotation                               |
| **Multipart Response** | A `LanguageModelToolResult` containing multiple content parts                  |

### Response Structure

Every tool response SHOULD return a multipart result with this structure:

```typescript
return new vscode.LanguageModelToolResult([
  // Part 1: Steering narrative (plain text)
  new vscode.LanguageModelTextPart(steeringNarrative),

  // Part 2: Structured data (typed JSON)
  vscode.LanguageModelDataPart.json(structuredPayload),
]);
```

### Steering Narrative Format

The text part follows a consistent template:

```
[Status Summary]
Current phase: {phase}. Status: {status}.

[Recommendation]
Recommended action: {action_description}.
Tool: `{tool_name}` with args: {brief_args_summary}.

[Context]
{1-2 sentences of relevant context explaining why this recommendation}.
```

Example:

```
Current phase: Implementation. Status: 2/5 steps complete.

Recommended action: Run the test suite to verify the refactor.
Tool: `exo-tdd-green` with args: none required.

The previous step modified the SteeringOption serializer. Tests will confirm the change didn't break existing behavior.
```

### Structured Payload Schema

The JSON part contains the full machine-readable state, matching the existing Rust protocol:

```typescript
interface ToolResponsePayload {
  // Core result data (varies by tool)
  result: unknown;

  // Steering metadata (when applicable) — matches Rust Steering struct
  steering?: {
    next_call: { kind: string; params: unknown };
    priority?: "critical" | "high" | "normal" | "low";
    confidence?: number; // 0.0-1.0, matches Option<f32>
    context_note?: string;
  };

  // Diagnostic metadata
  meta?: {
    duration_ms: number;
    warnings?: string[];
  };
}
```

**Note**: The `confidence` field remains numeric (`Option<f32>`) to match the existing protocol. The narrative can translate this to human-readable terms.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Machine Channel                          │
│  ┌─────────────┐    ┌──────────────┐    ┌───────────────┐  │
│  │ Command     │───▶│ Response     │───▶│ Multipart     │  │
│  │ Execution   │    │ Builder      │    │ Formatter     │  │
│  └─────────────┘    └──────────────┘    └───────────────┘  │
│                            │                    │           │
│                            ▼                    ▼           │
│                     ┌──────────────┐    ┌───────────────┐  │
│                     │ Steering     │    │ Text + JSON   │  │
│                     │ Extractor    │    │ Parts         │  │
│                     └──────────────┘    └───────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

**Components to modify:**

1. `tool-factory.ts` — Response construction logic + narrative composition
2. `MachineChannelResponse` types — Align with Rust `Steering` fields

**No Rust changes required**: Narrative is composed in TypeScript from existing structured fields (`context_note`, `rationale`). This follows the established pattern where CLI and LM tool contexts render the same data differently.

### Implementation Details

#### TypeScript (tool-factory.ts)

```typescript
function buildToolResult(
  response: MachineChannelResponse,
): vscode.LanguageModelToolResult {
  const parts: Array<
    vscode.LanguageModelTextPart | vscode.LanguageModelDataPart
  > = [];

  // Part 1: Steering narrative (composed from structured fields)
  const narrative = composeNarrative(response);
  if (narrative) {
    parts.push(new vscode.LanguageModelTextPart(narrative));
  }

  // Part 2: Structured payload (always present, unchanged from current protocol)
  parts.push(
    vscode.LanguageModelDataPart.json({
      result: response.result,
      steering: response.steering, // Pass through unchanged
      meta: response.meta,
    }),
  );

  return new vscode.LanguageModelToolResult(parts);
}
```

#### Narrative Composition (TypeScript)

Narrative is composed in TypeScript from structured fields, following the established pattern where CLI (`status_human()`) and LM tool contexts render the same data differently:

```typescript
function composeNarrative(
  response: MachineChannelResponse,
): string | undefined {
  const { steering, result } = response;
  if (!steering) return undefined;

  const parts: string[] = [];

  // Status summary from result (tool-specific)
  if (result?.phase) {
    parts.push(`Phase "${result.phase.title}" is ${result.phase.status}.`);
  }

  // Recommendation from steering
  if (steering.next_call) {
    parts.push(`Recommended: Call \`${steering.next_call.kind}\`.`);
  }

  // Context from context_note or rationale
  if (steering.context_note) {
    parts.push(steering.context_note);
  }

  return parts.length > 0 ? parts.join("\n\n") : undefined;
}
```

**Rationale**: This follows the existing codebase pattern where Rust emits structured data and each output context (CLI, LM tool) renders it appropriately. No Rust changes are required.

## Implementation Plan (Stage 2)

### Phase 1: Type Alignment

- [ ] Update TS `MachineChannelResponseEnvelope.steering` to include `priority`, `confidence`, `context_note`
- [ ] Add TS types for `SuggestedAction` and `SteeringBlock` if needed

### Phase 2: Narrative Composition (TypeScript)

- [ ] Create `composeNarrative()` helper in `tool-factory.ts`
- [ ] Implement action narrative for agent-fundamental tools
- [ ] Implement communication narrative for user-convenience tools

### Phase 3: Multipart Construction (TypeScript)

- [ ] Create `buildMultipartResult()` helper in `tool-factory.ts`
- [ ] Refactor CommandSpec tool responses to use multipart
- [ ] Refactor zero-arg tool responses to use multipart
- [ ] Update error response path to use multipart (per RFC 0063)

### Phase 4: Testing & Validation

- [ ] Add unit tests for multipart response construction
- [ ] Add integration tests verifying LM receives both parts
- [ ] Empirical testing with Claude, GPT-4, and other target models

## Context Updates (Stage 3)

- [ ] Update `docs/manual/architecture/machine-channel.md`
- [ ] Update `docs/specs/machine-channel-protocol.md` (if exists)
- [ ] Add example responses to tool documentation

## Friction Points & Prerequisites

### Friction 1: `tool-factory.ts` Ignores Steering

**Current state**: `tool-factory.ts` extracts `result` from the response envelope but **ignores** the `steering` field entirely. The steering data is present in the Rust response but never surfaces to the LM.

**Required fix**: Update `tool-factory.ts` to read `response.steering` and compose the narrative.

### Friction 2: Error Steering is Embedded in Result

**Current state**: RFC 0063 established that error responses embed steering in the `result` field. The `tool-factory.ts` error path uses only `error.message` for stderr and may drop structured steering.

**Required fix**: Error responses must also use multipart format, with the error message as narrative and structured error + steering as JSON.

### Friction 3: Zero-Arg Tools Have Separate Handling

**Current state**: Zero-arg tools (`exo-status`, `exo-steering`, etc.) have tool-specific response handling in `tool-factory.ts` but still emit text-only results.

**Required fix**: Centralize multipart creation so both CommandSpec tools and zero-arg tools use the same pattern.

### Friction 4: No Active Plan Items

**Current state**: No active phases or goals in `plan.toml` relate to steering or machine channel improvements.

**Recommendation**: This RFC should be scheduled after current work completes, or added as a parallel track if deemed high priority.

## Drawbacks

1. **Increased response size** — Two representations of similar data. Mitigated by keeping narrative terse.
2. **Maintenance burden** — Narrative must stay in sync with structured data. Mitigated by generating narrative from structured data.
3. **Unknown LM behavior** — We don't know exactly how different models handle multipart responses. Requires empirical testing.
4. **TS/Rust type sync** — Requires updating TS types to match Rust `Steering` fields before implementation.

## Alternatives

### Alternative A: Structured JSON Only (Current)

Keep embedding steering in JSON. Rejected because it conflates intent with data.

### Alternative B: Text Only

Return only narrative text. Rejected because it loses machine-readability for tooling/logging.

### Alternative C: Markdown with Embedded JSON

Return a single text part with markdown containing a JSON code block. Rejected because it doesn't leverage the typed `LanguageModelDataPart` API.

## Unresolved Questions

1. **Narrative verbosity** — How much context is optimal? Too little loses value; too much wastes tokens.
2. **Model compatibility** — Do all target models (Claude, GPT-4, etc.) handle multipart responses equivalently?

## Resolved Questions

| Question             | Resolution                                                                                              |
| -------------------- | ------------------------------------------------------------------------------------------------------- |
| Error responses      | Yes, multipart per RFC 0063                                                                             |
| Scope                | All tools, but with two steering types: action (agent-fundamental) and communication (user-convenience) |
| Tool classification  | `exo-rfc-list` and `exo-goal-list` promoted to agent-fundamental with communication steering            |
| Narrative generation | TypeScript composes narrative from structured fields; follows existing CLI/LM rendering pattern         |
| Confidence schema    | Keep `Option<f32>` to match existing protocol; narrative translates to human-readable                   |
| Multipart structure  | JSON part always present; text narrative included when steering exists                                  |
| Zero-arg tools       | Out of scope; addressed in separate RFC (00182)                                                         |

## Future Possibilities

1. **Adaptive narratives** — Adjust verbosity based on model capabilities or user preferences.
2. **Structured steering schemas** — Define JSON Schema for steering payloads to enable validation.
3. **Prompt-tsx integration** — Use `LanguageModelPromptTsxPart` for richer steering UI in chat.

## Appendix: Concrete Before/After Example

### Before (Current): `exo-status` Response

```json
{
  "phase": {
    "id": "phase-42",
    "title": "Implement multipart responses",
    "status": "in-progress"
  },
  "active_task": {
    "id": "task-3",
    "title": "Refactor tool-factory.ts",
    "status": "in-progress"
  },
  "steering": {
    "next_call": { "kind": "tdd.green", "params": {} },
    "confidence": 0.9,
    "context_note": "Tests will confirm the change works."
  }
}
```

The LM must parse this JSON, find the `steering` object, and infer why `tdd.green` is recommended.

### After (Proposed): `exo-status` Response

**Part 1 (Text):** Composed by TypeScript from structured fields

```
Phase "Implement multipart responses" is in progress (task 3/5).

Recommended: Call `tdd.green` to verify the refactor.

Tests will confirm the change works.
```

**Part 2 (JSON with `application/json` mime type):** Unchanged protocol

```json
{
  "result": {
    "phase": {
      "id": "phase-42",
      "title": "Implement multipart responses",
      "status": "in-progress"
    },
    "active_task": {
      "id": "task-3",
      "title": "Refactor tool-factory.ts",
      "status": "in-progress"
    }
  },
  "steering": {
    "next_call": { "kind": "tdd.green", "params": {} },
    "confidence": 0.9,
    "context_note": "Tests will confirm the change works."
  }
}
```

The LM reads the narrative first (understanding intent), then has structured data available for precise tool invocation. The JSON protocol is unchanged from current implementation.

## Appendix: Backward Compatibility

**Risk**: Some LM backends may not fully support multipart responses or may concatenate parts unexpectedly.

**Mitigation**: The text narrative is self-contained and actionable even if the JSON part is ignored. The JSON part is also self-contained. Either part alone provides enough information to proceed.

**Fallback**: If empirical testing shows issues with specific models, we can add a configuration flag to fall back to single-part JSON-only responses.

