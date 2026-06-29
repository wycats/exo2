<!-- exo:146 ulid:01kmzxeff1jpprpdhap2eehcw5 -->

# RFC 146: Vercel AI Gateway: Stream Feature Mapping & Integration Fixes


# RFC 0146: Vercel AI Gateway: Stream Feature Mapping & Integration Fixes

**Stage**: 0 (Idea)  
**Feature**: sferadev-extension  
**Created**: 2026-01-26  
**Updated**: 2026-01-26  
**Audit Status**: ✅ Verified by prepare agent

## Summary

This RFC documents the mapping between the Vercel AI SDK's streaming APIs and the VS Code Language Model API, identifies critical bugs in the current implementation, and provides a comprehensive fix plan.

**Status: READY FOR IMPLEMENTATION** - Bugs identified, fixes verified by prepare agent audit.

## Critical Issues Identified

### 1. Token Limit Miscommunication (BLOCKER)

The extension sets `maxInputTokens` directly from the model's `context_window` without reserving output tokens or accounting for tool/system overhead. This causes VS Code's compaction algorithm to fail catastrophically because it believes more input budget is available than actually exists.

### 2. Stream Chunk Schema Mismatch (BLOCKER)

The Vercel AI SDK has inconsistent chunk schemas between documentation and runtime:

- Error chunks use `error` field, not `errorText`
- The `reasoning-delta` handler expects `text` field but SDK sends `delta` field

> **Note (2026-01-26 Audit)**: The `text-delta` chunk handling is correct—`fullStream` uses `textDelta` consistently. The original claim about `text` field variants was incorrect. However, the `reasoning-delta` handler has a type signature bug that drops all reasoning content.

### 3. Tool Result Message Format (BLOCKER)

Tool results had multiple schema issues:

1. Missing `toolName` field (required by Vercel AI SDK `ToolResultPart`)
2. Wrong output structure: used `result: string` instead of `output: { type: 'text', value: string }`
3. Cross-message `toolCallId → toolName` mapping was not maintained

> **Note (2026-01-26 Fix)**: The `convertMessages` function now builds a `toolNameMap` in a first pass by scanning all `LanguageModelToolCallPart` entries, then uses this map when constructing `tool-result` messages in the second pass. The output format now correctly uses `{ type: 'text', value: ... }` per the SDK schema.

### 4. System Role Handling (BLOCKER)

System messages are not explicitly mapped - they're coerced to `assistant` and only retroactively fixed if they appear before the first user message.

> **Note (2026-01-26 Audit)**: VS Code's `LanguageModelChatMessageRole` enum only has `User` (1) and `Assistant` (2)—there is **no `System` role**. The fix is more complex than simply adding a case to the enum check; it requires detecting system messages through other means (e.g., content heuristics or metadata markers).

## Motivation

The Vercel AI Gateway extension provides VS Code Language Model providers backed by the Vercel AI SDK. When streaming responses, the AI SDK emits a rich set of chunk types through `fullStream`, but VS Code's Language Model API has a more constrained set of response parts. Understanding this mapping is crucial for:

1. **Bug prevention**: Knowing which chunks are ignored helps avoid bugs like the whitespace pollution issue
2. **Feature planning**: Identifying which AI SDK features could be surfaced through existing VS Code APIs
3. **API evolution tracking**: As VS Code adds new response part types, we can map them to existing AI SDK capabilities
4. **Correct integration**: Ensuring we use the right streaming API (`fullStream` vs `toUIMessageStream()`) and handle all chunk variants

## Vercel AI SDK Streaming API Documentation

### Stream API Comparison

| Stream                | Payload                                                                        | Tool Calls                                  | Metadata                                             | Intended Use                   |
| --------------------- | ------------------------------------------------------------------------------ | ------------------------------------------- | ---------------------------------------------------- | ------------------------------ |
| `fullStream`          | All SDK events (lifecycle + text/reasoning + tools + sources + files + errors) | Yes (`tool-call*`, `tool-result`)           | Yes (start/finish/step/usage)                        | Custom UIs, tool orchestration |
| `textStream`          | Text deltas only                                                               | No                                          | No                                                   | Simple text streaming          |
| `toUIMessageStream()` | AI SDK UI protocol (SSE data stream parts)                                     | Tool input/output parts only in UI protocol | Some (start/finish/step, optional reasoning/sources) | Frontend useChat/useCompletion |

**Current Implementation**: Uses `fullStream` ✅ (correct choice for VS Code integration)

### fullStream Chunk Types (Complete Reference)

Source: https://ai-sdk.dev/docs/reference/ai-sdk-core/stream-text

#### Core Lifecycle Events

| Chunk Type    | Purpose               | Fields                              |
| ------------- | --------------------- | ----------------------------------- |
| `start`       | Stream start          | -                                   |
| `start-step`  | Step start (LLM call) | -                                   |
| `finish-step` | Step end              | `usage`, `response`, `finishReason` |
| `finish`      | Stream end            | `usage`, `finishReason`             |
| `abort`       | Abort signal observed | -                                   |
| `error`       | Streaming error       | `error` (NOT `errorText`)           |

#### Text/Reasoning Events

| Chunk Type              | Purpose                 | Fields      |
| ----------------------- | ----------------------- | ----------- |
| `text-delta`            | Text content            | `textDelta` |
| `text-start`            | Text block start        | `id`        |
| `text-end`              | Text block end          | `id`        |
| `reasoning-start`       | Reasoning block start   | `id`        |
| `reasoning-delta`       | Reasoning content       | `delta`     |
| `reasoning-end`         | Reasoning block end     | `id`        |
| `reasoning-part-finish` | Reasoning part complete | -           |

> **Schema Warning**: The API reference also lists `text` and `reasoning` chunk types with a `text` field. The implementation must handle BOTH variants defensively.

#### Tool Events

| Chunk Type                  | Purpose                         | Fields                           |
| --------------------------- | ------------------------------- | -------------------------------- |
| `tool-call`                 | Complete tool call              | `toolCallId`, `toolName`, `args` |
| `tool-call-streaming-start` | Tool call beginning             | `toolCallId`, `toolName`         |
| `tool-call-delta`           | Incremental tool args           | `toolCallId`, `argsTextDelta`    |
| `tool-result`               | Tool result (only with execute) | `toolCallId`, `result`           |

#### Other Events

| Chunk Type | Purpose             | Fields                                    |
| ---------- | ------------------- | ----------------------------------------- |
| `source`   | Source reference    | `sourceId`, `url`, `title`                |
| `file`     | Generated file      | `file: { base64, uint8Array, mediaType }` |
| `raw`      | Raw provider chunks | Only with `includeRawChunks: true`        |

## Current Implementation Status

### Handled Chunk Types

| AI SDK Chunk Type | VS Code Response Part       | Status    | Notes                                                              |
| ----------------- | --------------------------- | --------- | ------------------------------------------------------------------ |
| `text-delta`      | `LanguageModelTextPart`     | ✅ OK     | Correctly uses `textDelta` field                                   |
| `reasoning-delta` | `LanguageModelThinkingPart` | ❌ BROKEN | Handler expects `text` but SDK sends `delta` (all content dropped) |
| `error`           | `LanguageModelTextPart`     | ❌ BROKEN | Expects `errorText` but SDK sends `error`                          |
| `tool-call`       | `LanguageModelToolCallPart` | ✅ OK     | Correctly forwards to VS Code                                      |
| `file`            | `LanguageModelDataPart`     | ✅ OK     | Handles images, JSON, text                                         |

### Silently Ignored Chunk Types

These chunks are emitted by the AI SDK but intentionally not processed. They should **never** emit content to the response stream.

#### Lifecycle/Framing Chunks (OK to ignore)

| AI SDK Chunk Type       | Purpose                        | Potential VS Code Mapping            |
| ----------------------- | ------------------------------ | ------------------------------------ |
| `start`                 | Message start                  | None - could use for debugging       |
| `text-start`            | Text block boundary start      | None - purely structural             |
| `text-end`              | Text block boundary end        | None - purely structural             |
| `reasoning-start`       | Reasoning block boundary start | None - purely structural             |
| `reasoning-end`         | Reasoning block boundary end   | None - purely structural             |
| `reasoning-part-finish` | Reasoning part complete        | None - purely structural             |
| `start-step`            | LLM step boundary start        | None - internal orchestration        |
| `finish-step`           | LLM step boundary end          | Could extract usage metadata         |
| `finish`                | Message complete signal        | None - implicit in stream end        |
| `abort`                 | Stream abort signal            | None - handled by cancellation token |

#### Tool Streaming Chunks (⚠️ May need handling)

| AI SDK Chunk Type           | Purpose                    | Status                                              |
| --------------------------- | -------------------------- | --------------------------------------------------- |
| `tool-call-streaming-start` | Tool call beginning        | ⚠️ Ignored - may miss calls if no final `tool-call` |
| `tool-call-delta`           | Incremental tool input     | ⚠️ Ignored - should buffer for incomplete calls     |
| `tool-result`               | Tool result (SDK-executed) | OK to ignore - VS Code handles execution            |

#### Source/Reference Chunks

| AI SDK Chunk Type | Purpose            | Potential VS Code Mapping                         |
| ----------------- | ------------------ | ------------------------------------------------- |
| `source`          | External reference | `LanguageModelDataPart.json()` with citation data |

#### Data Chunks

| AI SDK Chunk Type | Purpose                | Potential VS Code Mapping      |
| ----------------- | ---------------------- | ------------------------------ |
| `data-*`          | Custom structured data | `LanguageModelDataPart.json()` |

## VS Code API Inventory

### Available Response Parts (`LanguageModelResponsePart`)

```typescript
type LanguageModelResponsePart =
  | LanguageModelTextPart // Text content
  | LanguageModelToolResultPart // Tool execution results
  | LanguageModelToolCallPart // Tool invocations
  | LanguageModelDataPart; // Binary/structured data (images, JSON, etc.)
```

### `LanguageModelDataPart` Static Constructors

```typescript
LanguageModelDataPart.image(data: Uint8Array, mime: string)  // Images
LanguageModelDataPart.json(value: any, mime?: string)        // JSON data
LanguageModelDataPart.text(value: string, mime?: string)     // Text with MIME
```

### `LanguageModelThinkingPart` (Proposed API)

The extension currently checks for `LanguageModelThinkingPart` at runtime because it may not be available in all VS Code versions:

```typescript
const vsAny = vscode as unknown as Record<string, unknown>;
const ThinkingCtor = vsAny.LanguageModelThinkingPart;
if (ThinkingCtor && chunkObj.delta) {
  progress.report(new ThinkingCtor(chunkObj.delta));
}
```

## Proposed Enhancements

### Priority 1: Better Error Handling

**Current**: Errors are emitted as markdown text with `**Error:**` prefix  
**Proposed**: When VS Code adds a dedicated error response part, use it

### Priority 2: Source/Citation Support

**AI SDK provides**: `source-url`, `source-document` chunks with URLs and document references  
**VS Code opportunity**: Use `LanguageModelDataPart.json()` to emit structured citation data that chat extensions could render

```typescript
case "source-url": {
  const sourceChunk = chunk as { url: string; sourceId: string };
  progress.report(
    LanguageModelDataPart.json({
      type: "citation",
      url: sourceChunk.url,
      sourceId: sourceChunk.sourceId
    }, "application/vnd.vscode.citation+json")
  );
  break;
}
```

### Priority 3: File/Image Support

**AI SDK provides**: `file` chunks with URLs and media types  
**VS Code provides**: `LanguageModelDataPart.image()` for binary image data

```typescript
case "file": {
  const fileChunk = chunk as { url: string; mediaType: string };
  if (fileChunk.mediaType.startsWith("image/")) {
    // Fetch and emit as image
    const imageData = await fetchImageAsUint8Array(fileChunk.url);
    progress.report(
      LanguageModelDataPart.image(imageData, fileChunk.mediaType)
    );
  }
  break;
}
```

### Priority 4: Tool Input Streaming

**AI SDK provides**: `tool-input-start`, `tool-input-delta`, `tool-input-available` for progressive tool arg display  
**VS Code opportunity**: Could show tool invocation UI before execution completes

This would require accumulating deltas and emitting the `LanguageModelToolCallPart` when `tool-input-available` arrives with the complete input.

### Priority 5: Custom Data Passthrough

**AI SDK provides**: `data-*` custom type pattern  
**VS Code provides**: `LanguageModelDataPart.json()`

```typescript
if (chunk.type.startsWith("data-")) {
  const dataChunk = chunk as { type: string; data: unknown };
  const customType = dataChunk.type.slice(5); // Remove "data-" prefix
  progress.report(
    LanguageModelDataPart.json(
      dataChunk.data,
      `application/vnd.vercel.${customType}+json`,
    ),
  );
}
```

## Implementation Notes

### The Whitespace Bug (Fixed)

The original bug was caused by emitting whitespace for every unhandled chunk:

```typescript
// BAD - caused 50K+ tokens of whitespace pollution
private handleUnknownChunk(chunk: unknown, progress: Progress<...>): void {
  progress.report(new LanguageModelTextPart(" "));  // DON'T DO THIS
}
```

This was problematic because:

- `tool-input-delta` emits for every character of tool input (potentially hundreds per tool call)
- `start-step`/`finish-step` emit for every LLM call in multi-step scenarios
- `text-start`/`text-end` emit for every text block boundary

**Correct approach**: Silent ignore with debug logging:

```typescript
// GOOD - no output pollution
private handleUnknownChunk(chunk: unknown, _progress: Progress<...>): void {
  console.debug("[VercelAI] Ignored:", chunk.type);
}
```

### Version Compatibility

When using new VS Code API types, always check for their existence at runtime:

```typescript
const ThinkingPart = (vscode as any).LanguageModelThinkingPart;
if (ThinkingPart) {
  progress.report(new ThinkingPart(text));
} else {
  // Fallback: emit as regular text with prefix
  progress.report(new LanguageModelTextPart(`[Thinking] ${text}`));
}
```

## Chunk Type Reference (Complete)

### AI SDK `toUIMessageStream()` Chunks

From the [Vercel AI SDK Stream Protocol](https://ai-sdk.dev/docs/ai-sdk-ui/stream-protocol):

| Chunk Type              | Description          | Fields                            |
| ----------------------- | -------------------- | --------------------------------- |
| `start`                 | Message start        | `messageId`                       |
| `text-start`            | Text block start     | `id`                              |
| `text-delta`            | Text content         | `id`, `delta`                     |
| `text-end`              | Text block end       | `id`                              |
| `reasoning-start`       | Reasoning start      | `id`                              |
| `reasoning-delta`       | Reasoning content    | `id`, `delta`                     |
| `reasoning-end`         | Reasoning end        | `id`                              |
| `source-url`            | URL citation         | `sourceId`, `url`                 |
| `source-document`       | Document citation    | `sourceId`, `mediaType`, `title`  |
| `file`                  | File reference       | `url`, `mediaType`                |
| `data-*`                | Custom data          | `data`                            |
| `error`                 | Error                | `errorText`                       |
| `tool-input-start`      | Tool call start      | `toolCallId`, `toolName`          |
| `tool-input-delta`      | Tool input streaming | `toolCallId`, `inputTextDelta`    |
| `tool-input-available`  | Tool input complete  | `toolCallId`, `toolName`, `input` |
| `tool-output-available` | Tool result          | `toolCallId`, `output`            |
| `start-step`            | Step boundary start  | -                                 |
| `finish-step`           | Step boundary end    | -                                 |
| `finish`                | Message complete     | -                                 |
| `abort`                 | Stream aborted       | `reason`                          |

### VS Code `LanguageModelResponsePart` Types

| Type                          | Constructor                                          | Purpose                  |
| ----------------------------- | ---------------------------------------------------- | ------------------------ |
| `LanguageModelTextPart`       | `new LanguageModelTextPart(value: string)`           | Text content             |
| `LanguageModelToolCallPart`   | `new LanguageModelToolCallPart(callId, name, input)` | Tool invocation          |
| `LanguageModelToolResultPart` | `new LanguageModelToolResultPart(callId, content[])` | Tool result              |
| `LanguageModelDataPart`       | `.image()`, `.json()`, `.text()`                     | Binary/structured data   |
| `LanguageModelThinkingPart`   | `new LanguageModelThinkingPart(text)`                | Reasoning (proposed API) |

## Bug Analysis (2026-01-26 Review)

> **Audit Note**: Line numbers verified against actual codebase on 2026-01-26. Files located at:
>
> - `provider.ts`: `.reference/SferaDev/apps/vscode-ai-gateway/src/provider.ts`
> - `models.ts`: `.reference/SferaDev/apps/vscode-ai-gateway/src/models.ts`

### BLOCKER: Token Limit Miscommunication

**Location**: `models.ts` line 77

**Problem**: `maxInputTokens` is set directly to `context_window` from the models API without accounting for:

1. Output token reservation (model needs room to generate response)
2. Tool schema overhead (tool definitions consume tokens)
3. System message overhead

**Impact**: VS Code's compaction algorithm believes more input budget is available than actually exists. When the model rejects the request for being too long, VS Code cannot recover because it already thought it was under budget.

**Fix Required**:

```typescript
// models.ts - transformToVSCodeModels
maxInputTokens: Math.floor(model.context_window * 0.85), // Reserve 15% for output + overhead
```

Additionally, `streamText()` should pass `maxTokens` to limit output:

```typescript
// provider.ts - streamText call
const response = streamText({
  model: gateway(model.id),
  messages: convertMessages(chatMessages),
  maxTokens:
    options.modelOptions?.maxOutputTokens ?? Math.floor(model.max_tokens * 0.5),
  // ... other options
});
```

### BLOCKER: Stream Error Chunk Schema Mismatch

**Location**: `provider.ts` lines 548-555 (`handleErrorChunk`)

**Problem**: The code expects `errorText` field but the SDK sends `error`:

```typescript
// CURRENT (broken)
private handleErrorChunk(
  chunk: { type: "error"; errorText: string },  // ❌ Wrong field name
  progress: Progress<LanguageModelResponsePart>,
): void {
  const errorMessage = chunk.errorText || "Unknown error occurred";
```

**Fix Required**:

```typescript
// FIXED
private handleErrorChunk(
  chunk: { type: "error"; error: unknown },
  progress: Progress<LanguageModelResponsePart>,
): void {
  const errorMessage = chunk.error instanceof Error
    ? chunk.error.message
    : String(chunk.error) || "Unknown error occurred";
```

### BLOCKER: Reasoning Handler Type Signature Mismatch

**Location**: `provider.ts` lines 432-447 (`handleReasoningChunk`)

**Problem**: The `handleReasoningChunk` method has an incorrect type signature. It expects a chunk with a `text` field, but `reasoning-delta` chunks from `fullStream` have a `delta` field:

```typescript
// CURRENT (broken) - line 432
private handleReasoningChunk(
  chunk: { type: "reasoning"; text: string },  // ❌ Wrong - expects 'text'
  progress: Progress<LanguageModelResponsePart>,
): void {
  // ...
  if (ThinkingCtor && chunk.text) {  // ← Always undefined, reasoning dropped
    progress.report(new (ThinkingCtor...)(chunk.text));
  }
}
```

**Fix Required**:

```typescript
// FIXED
private handleReasoningChunk(
  chunk: { type: "reasoning-delta"; delta: string },
  progress: Progress<LanguageModelResponsePart>,
): void {
  // ...
  if (ThinkingCtor && chunk.delta) {
    progress.report(new (ThinkingCtor as any)(chunk.delta));
  }
}
```

> **Note**: The `text-delta` handling (lines 362-369) is correct—it uses `textDelta` which matches the SDK. Only the reasoning handler is broken.

### BLOCKER: Tool Result Message Format (FIXED)

**Location**: `provider.ts` lines 729-747 (convertMessages) and 796-828 (convertSingleMessage)

**Problems Identified**:

1. **Missing `toolName`**: VS Code's `LanguageModelToolResultPart` only has `callId`, but Vercel AI SDK requires `toolName`
2. **Wrong output structure**: Used `result: string` directly instead of typed `output: ToolResultOutput`
3. **No cross-message tracking**: Tool call parts and tool result parts can be in different messages

**Original (broken)**:

```typescript
results.push({
  role: "tool",
  content: [
    {
      type: "tool-result",
      toolCallId: part.callId,
      result: resultTexts.join(" "), // ❌ Wrong field and structure
      // ❌ Missing toolName!
    },
  ],
});
```

**Fix Applied**: Two-pass conversion that builds a `toolCallId → toolName` map first:

```typescript
function convertMessages(
  messages: readonly LanguageModelChatMessage[],
): ModelMessage[] {
  // First pass: build toolCallId -> toolName mapping
  const toolNameMap: Record<string, string> = {};
  for (const msg of messages) {
    for (const part of msg.content) {
      if (part instanceof LanguageModelToolCallPart) {
        toolNameMap[part.callId] = part.name;
      }
    }
  }

  // Second pass: convert with map available
  const result = messages
    .flatMap((msg) => convertSingleMessage(msg, toolNameMap))
    .filter(isValidMessage);
  // ...
}

// In convertSingleMessage:
results.push({
  role: "tool",
  content: [
    {
      type: "tool-result" as const,
      toolCallId: part.callId,
      toolName: toolNameMap[part.callId] || "unknown_tool", // ✅ Lookup from map
      output: {
        type: "text" as const,
        value: resultTexts.join(" "), // ✅ Correct ToolResultOutput structure
      },
    },
  ],
});
```

### BLOCKER: System Role Handling

**Location**: `provider.ts` lines 609-612 (role assignment) and 769-776 (fixSystemMessages)

**Problem**: System messages are coerced to `assistant` and only retroactively fixed if they appear before the first user message. Any system message after the first user message is incorrectly sent as `assistant`.

**Current Logic**:

```typescript
function fixSystemMessages(result: ModelMessage[]): void {
  const firstUserIndex = result.findIndex((msg) => msg.role === "user");
  for (let i = 0; i < firstUserIndex; i++) {
    if (result[i].role === "assistant") {
      result[i].role = "system";
    }
  }
}
```

**Fix Required**: This is more complex than originally thought. VS Code's `LanguageModelChatMessageRole` enum only has:

- `User` (1)
- `Assistant` (2)

There is **no `System` role** in the VS Code API. The current workaround of retroactively fixing messages before the first user message is the best available approach without additional metadata.

**Options**:

1. **Accept current behavior** - System messages before first user message work; later ones become assistant
2. **Content heuristics** - Detect system-like content patterns
3. **Custom metadata** - Use a VS Code-specific marker in message content to indicate system role
4. **API proposal** - Request VS Code add a `System` role to `LanguageModelChatMessageRole`

For now, document this limitation and ensure the `fixSystemMessages` function is working correctly for the common case (system prompt at start of conversation).

### IMPORTANT: Tool Schema Token Estimation

**Location**: `provider.ts` lines 225-264 (provideTokenCount)

**Problem**: Token estimation completely ignores tool definitions, which can be substantial (hundreds to thousands of tokens for complex schemas).

**Fix Required**:

```typescript
async provideTokenCount(
  model: LanguageModelChatInformation,
  text: string | LanguageModelChatMessage,
  _token: CancellationToken,
): Promise<number> {
  // ... existing logic ...

  // Add tool schema estimation if tools are defined
  // This requires access to the tools, which may need API changes
}
```

### IMPORTANT: Tool Call Streaming

**Location**: `provider.ts` (chunk handling section)

**Problem**: `tool-call-streaming-start` and `tool-call-delta` are ignored. If the provider emits streamed tool calls without a final `tool-call` chunk, VS Code will never receive the tool call.

**Fix Required**: Buffer streaming tool calls:

```typescript
private pendingToolCalls = new Map<string, { name: string; argsText: string }>();

case "tool-call-streaming-start":
  this.pendingToolCalls.set(chunk.toolCallId, {
    name: chunk.toolName,
    argsText: ""
  });
  break;

case "tool-call-delta":
  const pending = this.pendingToolCalls.get(chunk.toolCallId);
  if (pending) {
    pending.argsText += chunk.argsTextDelta;
  }
  break;

case "tool-call":
  // Complete tool call received, clear any pending state
  this.pendingToolCalls.delete(chunk.toolCallId);
  // ... existing handling ...
  break;
```

## Fix Priority

### Critical (Must Fix Before Use)

1. **Fix stream error mapping** (`provider.ts:548-555`) - Errors are completely silent
2. **Fix reasoning-delta handler** (`provider.ts:432-447`) - All reasoning content is dropped
3. **Correct tool result payload** (`provider.ts:649-661`) - Tool execution results are lost
4. **Fix maxInputTokens semantics** (`models.ts:77`) - Compaction will fail catastrophically
5. **Document system role limitation** - VS Code API has no System role; current workaround is best available

### Important (Should Fix Soon)

1. **Account for tool schemas in token estimation** (`provider.ts:225-264`) - Underestimation causes failures
2. **Handle tool-call streaming deltas** - May miss tool calls from some providers
3. **Pass maxTokens into streamText** (`provider.ts:163`) - Prevent output from exceeding budget
4. **Fix tests** (`.reference/SferaDev/apps/vscode-ai-gateway/test/provider.test.ts`) - Tests use wrong field names and mask bugs

### Nice to Have

1. **Surface reasoning parts more completely** - Handle start/end events
2. **Expose source chunks** - Could enhance citations
3. **Extract usage metadata from finish-step** - Better observability

## Code Changes Required

### `provider.ts`

1. **Lines 548-555**: Fix `handleErrorChunk` to use `error` field instead of `errorText`
2. **Lines 432-447**: Fix `handleReasoningChunk` type signature to use `delta` instead of `text`
3. **Lines 649-661**: Fix tool result message format (use `result` directly, not `output` wrapper)
4. **Line 163**: Add `maxTokens` parameter to `streamText()` call
5. **(Optional)** Add buffering for `tool-call-streaming-start` and `tool-call-delta`

### `models.ts`

1. **Line 77**: Reduce `maxInputTokens` to reserve output budget: `Math.floor(model.context_window * 0.85)`

### `provider.test.ts` (Test Fixes)

1. **Line 145**: Change `delta` to `textDelta` for text-delta chunk tests (test has wrong field)
2. **Line 225**: Change `errorText` to `error` for error chunk tests (test mirrors production bug)
3. **Line 173**: Test correctly uses `delta` for reasoning - no change needed (production code is wrong, not test)

## Decision Log

| Date       | Decision                                                  | Rationale                                                    |
| ---------- | --------------------------------------------------------- | ------------------------------------------------------------ |
| 2026-01-26 | Remove whitespace emission from `handleUnknownChunk`      | Fixes 50K+ token pollution bug                               |
| 2026-01-26 | Add `detail: "Vercel AI Gateway"` to model info           | Shows provider in VS Code model picker                       |
| 2026-01-26 | Runtime check for `LanguageModelThinkingPart`             | Graceful degradation for older VS Code                       |
| 2026-01-26 | Comprehensive review identified 5 blocking bugs           | Previous review missed critical issues                       |
| 2026-01-26 | **Audit correction**: `text-delta` handling is OK         | Prepare agent verified `textDelta` field is correct          |
| 2026-01-26 | **Audit correction**: System role has no API fix          | VS Code `LanguageModelChatMessageRole` has no `System` value |
| 2026-01-26 | **New bug found**: reasoning-delta type signature         | Handler expects `text` but SDK sends `delta`                 |
| 2026-01-26 | **Test bug found**: Tests use wrong field names           | Tests mask real bugs by using incorrect schemas              |
| 2026-01-27 | Remove hard-fail preflight check                          | Let API handle actual errors; estimates are imprecise        |
| 2026-01-27 | Add error logging utility                                 | Surface full error details for debugging                     |
| 2026-01-27 | Implement Anthropic context_management                    | Automatically clear old tool uses when approaching limits    |
| 2026-01-27 | **Research confirmed**: Gateway supports provider options | `providerOptions.anthropic` passes through to Anthropic API  |

## Context Length Handling Architecture (2026-01-27)

### Background Research Findings

After extensive investigation into how VS Code handles context length:

1. **VS Code has NO built-in compaction** - Extensions are expected to manage their own context. The `provideTokenCount` API is purely informational; VS Code does not automatically truncate or compact messages.

2. **The current extension hard-fails** when estimated tokens exceed the model limit (lines 158-167 in `provider.ts`). This prevents the API from ever being called, even when:
   - The estimate is wrong (estimation is inherently imprecise)
   - The API might accept the request anyway
   - The user could benefit from seeing the actual API error

3. **Anthropic's `context_management` API CAN be used through the gateway** - Despite initial concerns, the Vercel AI Gateway documentation confirms that provider-specific options are passed through. The `providerOptions` object supports both gateway-specific options AND provider-specific options under different keys.

### How VS Code Expects Context Length to Work

The VS Code Language Model API expects context management to work as follows:

1. **Extension provides token estimates** via `provideTokenCount()`
2. **Consumers (chat extensions, Copilot)** use these estimates to decide whether to compact
3. **If compaction is needed**, the consumer is responsible for implementing it
4. **The extension should not hard-fail** based on estimates—let the API determine actual limits

### Anthropic Context Management Support

**Research confirmed (2026-01-27)**: Provider-specific options ARE supported through the gateway.

- AI SDK's AI Gateway provider explicitly documents Provider-Specific Options with provider keys like `anthropic`
- Anthropic provider docs include a Context Management section with `providerOptions.anthropic.contextManagement`
- Issue #10485 "Anthropic: implement context_management" was completed
- PR #10540 "feat(anthropic): context management" was merged

**Correct usage pattern**:

```typescript
streamText({
  model: gateway("anthropic/claude-sonnet-4"),
  messages,
  providerOptions: {
    gateway: {
      /* gateway-specific options */
    },
    anthropic: {
      contextManagement: {
        // Automatically clear old tool uses when approaching limits
        enabled: true,
        strategy: "sliding-window",
      },
    },
  },
});
```

### Implementation Plan: Context Length Fix

#### Task 1: Remove Hard-Fail Preflight Check

**File**: `src/provider.ts` (lines 158-167)

Convert the hard error to a warning. Let the API handle the actual error if context is truly too long.

**Rationale**:

- Token estimation is inherently imprecise
- The API may accept requests that exceed estimated limits
- Users should see actual API errors, not estimation-based rejections
- This aligns with VS Code's architecture where the extension provides estimates, not enforcement

#### Task 2: Implement Error Logging

**New File**: `src/logger.ts`

Create a logger utility that extracts full error details from `GatewayError` and `APICallError` for debugging. This helps diagnose actual context length errors when they occur.

#### Task 3: Implement Anthropic Context Management

For Anthropic/Claude models, add `providerOptions.anthropic.contextManagement` to automatically clear old tool uses and thinking turns when approaching context limits.

### References

- [AI Gateway Provider Options](https://ai-sdk.dev/providers/ai-sdk-providers/ai-gateway)
- [Anthropic Context Management](https://ai-sdk.dev/providers/ai-sdk-providers/anthropic)
- [GitHub PR #10540](https://github.com/vercel/ai/pull/10540)

## Audit Summary (2026-01-26)

A prepare agent audited this RFC against the actual codebase. Key findings:

### Verified Bugs (4 of 5 confirmed)

| Bug                | Status                          | Actual Location                |
| ------------------ | ------------------------------- | ------------------------------ |
| Token Limit        | ✅ CONFIRMED                    | `models.ts:77`                 |
| Error Schema       | ✅ CONFIRMED                    | `provider.ts:548-555`          |
| Tool Result Format | ✅ CONFIRMED                    | `provider.ts:649-661`          |
| System Role        | ✅ CONFIRMED (no fix available) | `provider.ts:609-612, 769-776` |

### Corrected Claims

| Original Claim                           | Correction                                                      |
| ---------------------------------------- | --------------------------------------------------------------- |
| `text-delta` has schema variants         | ❌ INCORRECT - `textDelta` is used consistently in `fullStream` |
| System role can be fixed with enum check | ❌ INCORRECT - VS Code API has no `System` role value           |

### New Bugs Found

| Bug                                      | Location              | Impact                                  |
| ---------------------------------------- | --------------------- | --------------------------------------- |
| `reasoning-delta` handler type signature | `provider.ts:432-447` | All reasoning content silently dropped  |
| Test file uses wrong field names         | `provider.test.ts`    | Tests pass but don't validate real code |

### Dependencies Verified

- `@ai-sdk/gateway`: 3.0.23
- `ai`: 6.0.50
- VS Code engine: `^1.108.0`

## Accurate Token Tracking Design

### The Problem

The Vercel AI SDK provides **actual** token usage in `finish` and `finish-step` chunks, but VS Code's `provideTokenCount` API only supports **estimation**. This mismatch means:

1. First request uses estimation (potentially inaccurate)
2. Subsequent requests continue using estimation even though we have real data
3. Compaction algorithm works with inaccurate counts

### Key Insight

We can bridge this gap by capturing actual usage data and using it to improve subsequent estimations. After each request completes, we know the **real** token cost. We should use that for future calculations.

### Data Available from Vercel AI SDK

**`finish-step` chunk** (emitted at end of each LLM call step):

```typescript
{
  type: "finish-step";
  usage: {
    inputTokens: number | undefined;
    outputTokens: number | undefined;
    totalTokens: number | undefined;
  }
}
```

**`finish` chunk** (emitted at very end of stream):

```typescript
{
  type: "finish";
  totalUsage: {
    inputTokens: number | undefined;
    outputTokens: number | undefined;
    totalTokens: number | undefined;
  }
}
```

### Implementation Approach

#### 1. Add State to `VercelAIChatModelProvider`

```typescript
// Track actual token usage from completed requests
private lastRequestInputTokens: number | null = null;
private lastRequestOutputTokens: number | null = null;
private lastRequestMessageCount: number = 0;
```

#### 2. Capture Usage from `finish`/`finish-step` Chunks

Modify the chunk handler to extract and store usage data:

```typescript
case "finish-step": {
  const finishChunk = chunk as {
    type: "finish-step";
    usage?: { inputTokens?: number; outputTokens?: number };
  };
  if (finishChunk.usage?.inputTokens !== undefined) {
    this.lastRequestInputTokens = finishChunk.usage.inputTokens;
  }
  if (finishChunk.usage?.outputTokens !== undefined) {
    this.lastRequestOutputTokens = finishChunk.usage.outputTokens;
  }
  break;
}

case "finish": {
  const finishChunk = chunk as {
    type: "finish";
    totalUsage?: { inputTokens?: number; outputTokens?: number };
  };
  if (finishChunk.totalUsage?.inputTokens !== undefined) {
    this.lastRequestInputTokens = finishChunk.totalUsage.inputTokens;
  }
  if (finishChunk.totalUsage?.outputTokens !== undefined) {
    this.lastRequestOutputTokens = finishChunk.totalUsage.outputTokens;
  }
  break;
}
```

#### 3. Hybrid Token Estimation

For `estimateTotalInputTokens`, use a hybrid approach:

- **Known cost (actual)**: Tokens from last completed request for messages we've already sent
- **Estimated new content**: Only estimate the delta (new messages since last request)

```typescript
private async estimateTotalInputTokens(
  model: LanguageModelChatInformation,
  messages: readonly LanguageModelChatMessage[],
  token: CancellationToken,
): Promise<number> {
  // If we have actual data from a previous request in this conversation
  if (this.lastRequestInputTokens !== null &&
      messages.length > this.lastRequestMessageCount) {
    // Use actual tokens for known messages, estimate only new ones
    const newMessages = messages.slice(this.lastRequestMessageCount);
    let newTokenEstimate = 0;
    for (const message of newMessages) {
      newTokenEstimate += await this.provideTokenCount(model, message, token);
    }
    return this.lastRequestInputTokens + newTokenEstimate;
  }

  // Fall back to pure estimation for first request
  let total = 0;
  for (const message of messages) {
    total += await this.provideTokenCount(model, message, token);
  }
  total += messages.length * 4; // Message structure overhead
  return total;
}
```

#### 4. Track Message Count

Store the message count after each successful request to enable conversation change detection:

```typescript
// In provideLanguageModelChatResponse, after successful stream completion:
this.lastRequestMessageCount = chatMessages.length;
```

### Benefits

| Benefit                          | Description                                         |
| -------------------------------- | --------------------------------------------------- |
| **Accuracy after first request** | Token counts become real data, not estimates        |
| **Better compaction**            | VS Code's compaction algorithm gets accurate input  |
| **Reduced failures**             | Fewer "context too long" errors from miscalculation |
| **Observable**                   | Can log actual vs estimated for debugging           |

### Limitations

| Limitation                             | Impact                        | Mitigation                                       |
| -------------------------------------- | ----------------------------- | ------------------------------------------------ |
| First request uses estimation          | May be inaccurate             | Conservative estimation buffer (0.85 multiplier) |
| Edited/removed messages reset tracking | Falls back to estimation      | **Per-message hash caching (see below)**         |
| Provider may not send usage            | Some providers omit this data | Graceful fallback to estimation                  |

### Per-Message Hash-Based Caching

#### The Problem

The message-count-based approach has a critical flaw: if any message is edited, the count changes and we fall back to pure estimation for the entire conversation. This loses valuable token data we've already collected.

#### The Solution

Hash each message's content to create a stable cache key. Store `hash → tokenCount` mappings in `ExtensionContext.workspaceState`. This gives us:

1. **Per-message granularity**: Editing one message only invalidates that message's cache
2. **Persistence**: Cache survives VS Code restarts
3. **Correction factor**: After actual usage data, we can improve estimates

#### Implementation

**1. Hash Function**

```typescript
import { createHash } from "crypto";

function hashMessage(msg: LanguageModelChatRequestMessage): string {
  const payload = {
    role: msg.role,
    name: msg.name ?? null,
    content: msg.content.map((part) => {
      if (part instanceof LanguageModelTextPart) {
        return { type: "text", value: part.value };
      }
      if (part instanceof LanguageModelDataPart) {
        return {
          type: "data",
          mimeType: part.mimeType,
          dataLen: part.data.length,
        };
      }
      if (part instanceof LanguageModelToolCallPart) {
        return { type: "toolCall", name: part.name, callId: part.callId };
      }
      if (part instanceof LanguageModelToolResultPart) {
        return { type: "toolResult", callId: part.callId };
      }
      return { type: "unknown" };
    }),
  };
  return createHash("sha256").update(JSON.stringify(payload)).digest("hex");
}
```

**2. Cache Operations**

```typescript
class VercelAIChatModelProvider {
  private context: ExtensionContext;
  private correctionFactor: number = 1.0; // Actual/Estimated ratio

  private getCachedTokenCount(
    msg: LanguageModelChatRequestMessage,
  ): number | undefined {
    const key = `lm.tokens.${hashMessage(msg)}`;
    return this.context.workspaceState.get<number>(key);
  }

  private async setCachedTokenCount(
    msg: LanguageModelChatRequestMessage,
    tokens: number,
  ): Promise<void> {
    const key = `lm.tokens.${hashMessage(msg)}`;
    await this.context.workspaceState.update(key, tokens);
  }
}
```

**3. Updated Token Counting**

```typescript
async provideTokenCount(
  model: LanguageModelChatInformation,
  text: string | LanguageModelChatRequestMessage,
  _token: CancellationToken,
): Promise<number> {
  if (typeof text === "string") {
    return Math.ceil(text.length / 3.5 * this.correctionFactor);
  }

  // Check cache first
  const cached = this.getCachedTokenCount(text);
  if (cached !== undefined) {
    return cached;
  }

  // Estimate and cache
  const estimate = this.estimateMessageTokens(text);
  const corrected = Math.ceil(estimate * this.correctionFactor);
  await this.setCachedTokenCount(text, corrected);
  return corrected;
}
```

**4. Correction Factor Update**

After receiving actual usage from `finish` chunk:

```typescript
// In handleStreamChunk, after capturing actual usage:
if (this.lastEstimatedInputTokens > 0 && this.lastRequestInputTokens !== null) {
  const newFactor = this.lastRequestInputTokens / this.lastEstimatedInputTokens;
  // Smooth the correction factor (moving average)
  this.correctionFactor = this.correctionFactor * 0.7 + newFactor * 0.3;
  console.debug(
    `[VercelAI] Correction factor updated: ${this.correctionFactor.toFixed(3)}`,
  );
}
```

#### Benefits Over Message-Count Approach

| Scenario            | Message-Count Approach    | Hash-Based Approach                 |
| ------------------- | ------------------------- | ----------------------------------- |
| New message added   | ✅ Uses actual + estimate | ✅ Uses cached + estimate           |
| Message edited      | ❌ Full re-estimation     | ✅ Only edited message re-estimated |
| Message deleted     | ❌ Full re-estimation     | ✅ Remaining messages use cache     |
| VS Code restart     | ❌ Loses all data         | ✅ Cache persists                   |
| Correction learning | ❌ None                   | ✅ Improves over time               |

## Implementation Plan for Execute Agent

**Target Directory**: `.reference/SferaDev/apps/vscode-ai-gateway/`

### Phase 1: Critical Fixes (Must Complete)

Execute these changes in order:

#### 1.1 Fix Token Limit (`models.ts`)

**File**: `src/models.ts`  
**Line**: 78  
**Change**: Replace `maxInputTokens: model.context_window` with `maxInputTokens: Math.floor(model.context_window * 0.85)`

#### 1.2 Fix Reasoning Handler (`provider.ts`)

**File**: `src/provider.ts`  
**Method**: `handleReasoningChunk` (line 431)  
**Changes**:

1. Change type signature from `{ type: "reasoning"; text: string }` to `{ type: "reasoning-delta"; delta: string }`
2. Change all references to `chunk.text` to `chunk.delta`

#### 1.3 Fix Error Handler (`provider.ts`)

**File**: `src/provider.ts`  
**Method**: `handleErrorChunk` (line 548)  
**Changes**:

1. Change type signature from `{ type: "error"; errorText: string }` to `{ type: "error"; error: unknown }`
2. Update error extraction: `chunk.error instanceof Error ? chunk.error.message : String(chunk.error)`

#### 1.4 Fix Tool Result Format (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: `convertSingleMessage` function (line 665)  
**Change**: Replace `output: { type: "text", value: resultTexts.join(" ") }` with `result: resultTexts.join(" ")`

#### 1.5 Add maxTokens to streamText (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: `streamText()` call (line 164)  
**Change**: Add `maxTokens` parameter to the options object

### Phase 2: Test Fixes

#### 2.1 Fix text-delta test (`provider.test.ts`)

**File**: `src/provider.test.ts`  
**Lines**: 145, 148, 160, 163  
**Change**: Change mock chunk field from `delta` to `textDelta` in text-delta test cases

#### 2.2 Fix error test (`provider.test.ts`)

**File**: `src/provider.test.ts`  
**Lines**: 225, 230, 318, 323  
**Change**: Change mock chunk field from `errorText` to `error` in error test cases

### Phase 3: Accurate Token Tracking

#### 3.1 Add Token Tracking State (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: Class properties section (after `private modelsClient: ModelsClient;`)  
**Add**:

```typescript
// Track actual token usage from completed requests
private lastRequestInputTokens: number | null = null;
private lastRequestOutputTokens: number | null = null;
private lastRequestMessageCount: number = 0;
```

#### 3.2 Capture Usage from Finish Chunks (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: `handleStreamChunk` method, in the switch statement  
**Change**: Replace the empty `case "finish":` and `case "finish-step":` handlers with code that extracts usage data

#### 3.3 Update Message Count After Request (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: `provideLanguageModelChatResponse` method, after the stream loop completes  
**Add**: `this.lastRequestMessageCount = chatMessages.length;`

#### 3.4 Update `estimateTotalInputTokens` (`provider.ts`)

**File**: `src/provider.ts`  
**Method**: `estimateTotalInputTokens`  
**Change**: Implement hybrid estimation - use actual tokens for known messages, estimate only new ones

### Phase 3.5: Per-Message Hash-Based Caching

#### 3.5.1 Add hashMessage Utility Function (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: Top of file, after imports  
**Add**: `hashMessage()` function using SHA-256 on serialized message content

```typescript
import { createHash } from "crypto";

function hashMessage(msg: LanguageModelChatRequestMessage): string {
  const payload = {
    role: msg.role,
    name: msg.name ?? null,
    content: msg.content.map((part) => {
      if (part instanceof LanguageModelTextPart) {
        return { type: "text", value: part.value };
      }
      if (part instanceof LanguageModelDataPart) {
        return {
          type: "data",
          mimeType: part.mimeType,
          dataLen: part.data.length,
        };
      }
      if (part instanceof LanguageModelToolCallPart) {
        return { type: "toolCall", name: part.name, callId: part.callId };
      }
      if (part instanceof LanguageModelToolResultPart) {
        return { type: "toolResult", callId: part.callId };
      }
      return { type: "unknown" };
    }),
  };
  return createHash("sha256").update(JSON.stringify(payload)).digest("hex");
}
```

#### 3.5.2 Store ExtensionContext Reference (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: Constructor  
**Change**: Store `context` parameter for later use with `workspaceState`

```typescript
private context: ExtensionContext;

constructor(context: ExtensionContext) {
  this.context = context;
  this.modelsClient = new ModelsClient();
}
```

#### 3.5.3 Add Cache Lookup/Store Methods (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: Class methods section  
**Add**:

```typescript
private getCachedTokenCount(msg: LanguageModelChatRequestMessage): number | undefined {
  const key = `lm.tokens.${hashMessage(msg)}`;
  return this.context.workspaceState.get<number>(key);
}

private async setCachedTokenCount(
  msg: LanguageModelChatRequestMessage,
  tokens: number,
): Promise<void> {
  const key = `lm.tokens.${hashMessage(msg)}`;
  await this.context.workspaceState.update(key, tokens);
}
```

#### 3.5.4 Add Correction Factor State (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: Class properties  
**Add**:

```typescript
private correctionFactor: number = 1.0;
private lastEstimatedInputTokens: number = 0;
```

#### 3.5.5 Update provideTokenCount to Use Cache (`provider.ts`)

**File**: `src/provider.ts`  
**Method**: `provideTokenCount`  
**Change**: Check cache first, apply correction factor, cache new estimates

#### 3.5.6 Track Estimated Tokens Before Request (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: `provideLanguageModelChatResponse`, before stream loop  
**Add**: Store the estimated total for correction factor calculation

```typescript
this.lastEstimatedInputTokens = estimatedTokens;
```

#### 3.5.7 Update Correction Factor After Actual Usage (`provider.ts`)

**File**: `src/provider.ts`  
**Location**: `finish` chunk handler  
**Add**: Calculate and smooth correction factor

```typescript
if (this.lastEstimatedInputTokens > 0 && this.lastRequestInputTokens !== null) {
  const newFactor = this.lastRequestInputTokens / this.lastEstimatedInputTokens;
  this.correctionFactor = this.correctionFactor * 0.7 + newFactor * 0.3;
  console.debug(
    `[VercelAI] Correction factor: ${this.correctionFactor.toFixed(3)}`,
  );
}
```

### Phase 4: Documentation

Add a comment to `fixSystemMessages` function explaining the VS Code API limitation (no System role available).

### Verification Steps

After implementation:

1. Run `pnpm test` to verify tests pass
2. Build the extension with `pnpm build`
3. Manual test with a model that supports reasoning (e.g., Claude) to verify reasoning content appears
4. **New**: Verify token tracking by making two requests and checking that the second uses actual token data from the first

## References

- [Vercel AI SDK Stream Protocol](https://ai-sdk.dev/docs/ai-sdk-ui/stream-protocol)
- [Vercel AI SDK streamText Reference](https://ai-sdk.dev/docs/reference/ai-sdk-core/stream-text)
- [VS Code Language Model API](https://code.visualstudio.com/api/references/vscode-api#LanguageModelResponsePart)
- [VS Code Language Model Chat Provider](https://code.visualstudio.com/api/references/vscode-api#LanguageModelChatProvider)

