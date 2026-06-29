# RTD Streaming Protocol

**Status**: Draft
**Parent**: [RTD Architecture](../architecture.md)

## 1. Introduction

The **RTD Streaming Protocol** defines the operational semantics for the **Resumable Parser** (Layer 2).

Unlike traditional parsers that assume a complete document, the RTD Parser is designed as a **State Machine** that can pause and resume execution at any point. This allows it to process incomplete chunks of data (e.g., from an LLM stream) without emitting premature or incorrect tokens ("flicker").

## 2. The Resumable State Machine

The Parser is not a function of `String -> AST`. It is a stateful processor:

$$
(State_{t}, Chunk) \rightarrow (State_{t+1}, Events)
$$

### 2.1 The Suspension Principle

**Rule**: The Parser MUST NOT emit a token until it has unambiguously identified it.

If the Parser reaches the end of a chunk (EOF) while in a state where the next character could change the token type (an **Ambiguous State**), it MUST **Suspend**.

- **Suspend**: The Parser saves its current state (e.g., `AsteriskState`) and any partially consumed characters. It returns control to the caller.
- **Resume**: When the next chunk arrives, the Parser restores its state and continues processing as if the stream were contiguous.

### 2.2 State Categories

To implement this, the State Machine states defined in the [Source Grammar](./syntax.md) are categorized by their behavior at EOF.

#### 2.2.1 Immediate States (Emit on EOF)

In these states, reaching EOF is unambiguous. The Parser emits the pending token and transitions to `EOF`.

- **Data State**: If EOF is reached, emit `EOF`.
- **Text State**: If EOF is reached, emit the buffered text.

#### 2.2.2 Suspensible States (Wait on EOF)

In these states, the token type depends on a future character. If EOF is reached, the Parser **Suspends**.

| State          | Ambiguity           | Action on EOF            |
| :------------- | :------------------ | :----------------------- |
| **Asterisk**   | `*` vs `**` vs `* ` | Suspend. Buffer `*`.     |
| **Underscore** | `_` vs `__`         | Suspend. Buffer `_`.     |
| **Backtick**   | `` ` `` vs ` ``` `  | Suspend. Buffer `` ` ``. |
| **LinkStart**  | `[` vs `![`         | Suspend. Buffer `[`.     |
| **ImageStart** | `!` vs `![`         | Suspend. Buffer `!`.     |
| **MathStart**  | `$` vs `$$`         | Suspend. Buffer `$`.     |
| **LessThan**   | `<` vs `<!--`       | Suspend. Buffer `<`.     |
| **Colon**      | `:` vs `:::`        | Suspend. Buffer `:`.     |
| **Escape**     | `\` vs `\*`         | Suspend. Buffer `\`.     |

### 2.3 The "Flush" Signal

The Parser supports a `flush()` operation (or `final: true` flag). When this signal is received, the Parser treats the current EOF as the **True End of Stream**.

- **Action**: All Suspensible States MUST resolve immediately to their fallback token.
  - _Example_: If suspended in `AsteriskState` with `*`, `flush()` forces the emission of `Character(*)` and then `EOF`.

## 3. Tree Builder Integration

The Tree Builder (Layer 0) also participates in the streaming protocol to ensure the DOM remains stable.

### 3.1 The "Open Node" Policy

The Tree Builder MUST maintain a pointer to the **Current Open Node**.

- **Streaming Updates**: As text tokens arrive, they are appended to the Current Open Node.
- **Event Firing**: The Tree Builder SHOULD emit an `update` event whenever the tree is modified, allowing the UI to re-render.

### 3.2 Auto-Closing (Virtual)

When the stream ends (or pauses), the Tree Builder effectively "Auto-Closes" all open nodes to produce a valid render tree.

- **Implementation**: This is often a "Virtual Close". The nodes remain open in the internal state (waiting for more data), but the _rendered_ output treats them as closed.
- **Example**: An unclosed `<b>` tag at the end of the stream is rendered as `<b>...</b>`.

## 4. Implementation Guidelines

### 4.1 The `Parser` Class

To support **Halting** and **Time-Slicing**, the Parser SHOULD be implemented as a Generator.

```typescript
class StreamingParser {
  private state: State = State.Data;
  private buffer: string = "";

  /**
   * Feeds a chunk of text to the parser.
   * Yields events as they are discovered.
   * This allows the caller to interrupt processing mid-chunk.
   */
  public *write(chunk: string): Generator<RTDEvent, void, void> {
    // 1. Append chunk to internal buffer
    // 2. Loop through state machine
    // 3. Yield events immediately
    // 4. If EOF in Suspensible State, return (save state)
  }

  /**
   * Signals the end of the stream.
   */
  public *close(): Generator<RTDEvent, void, void> {
    // 1. Force resolution of Suspensible States
    // 2. Yield EOF
  }
}
```

## 5. Advanced Control Flow

### 5.1 Halting & Interruption

The Generator architecture allows the Consumer to halt processing at any point, even within a single chunk.

- **Scenario**: The "Hallucination Brake" (Layer 1) detects forbidden text.
- **Action**: The Consumer iterates the generator. Upon receiving the forbidden `Text` event, it simply **stops iterating** (and aborts the upstream LLM connection). The remaining characters in the chunk are never processed.

### 5.2 Stream Adjustments (Rewriting)

Stream adjustments can occur at two levels:

1.  **Pre-Parse Normalization**: Modifying the raw string before passing it to `write()` (e.g., normalizing Math delimiters).
2.  **Post-Parse Transformation**: Modifying the `RTDEvent` stream before it reaches the Tree Builder (e.g., censoring sensitive data).

### 5.3 Backpressure

Because the Parser is synchronous (CPU-bound) but yields control, the Consumer can implement **Backpressure** by delaying the next call to `next()` or `write()`, allowing the UI thread to breathe during heavy updates.
