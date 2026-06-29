<!-- exo:10112 ulid:01kmzxey1shqn910fceg5xc1k2 -->


# RFC 10112: Chat History & Session Recovery

## Summary

This RFC defines the chat history system — how the agent recovers context from previous sessions to solve the "amnesia" problem where valuable reasoning, decisions, and tool outputs are lost when a chat session ends.

## Problem

1. **Session Amnesia**: When starting a new chat, the agent has no memory of previous conversations
2. **Lost Reasoning**: Decisions made in previous sessions must be re-explained
3. **Handoff Friction**: No structured way to carry context forward when context window fills up
4. **Context Recovery**: Agent needs to identify the right session to recover from

## Architecture

### Three-Layer Design

```
┌─────────────────────────────────────────────────────────────┐
│  LM Tool: exo-ai-chat-history                               │
│  (VS Code extension, calls CLI)                             │
├─────────────────────────────────────────────────────────────┤
│  CLI: exo ai chat-history                                   │
│  (Rust, shells out to exohistory)                           │
├─────────────────────────────────────────────────────────────┤
│  Binary: exohistory                                         │
│  (Rust, reads VS Code workspaceStorage/chatSessions)        │
└─────────────────────────────────────────────────────────────┘
```

**Key insight**: Rather than maintaining our own database, we piggyback on VS Code's native chat session storage. This avoids a separate recording pipeline and works with existing conversations.

### exohistory Binary

Located in `crates/exohistory/`, reads VS Code's `workspaceStorage/chatSessions` directory.

**Subcommands:**

- `recent` — Get recent turns from the most recent session
- `search` — Search across sessions (not yet implemented)
- `analyze` — Analyze session patterns (loop detection, tool stats)

**Output schema** (`recent`):

```json
{
  "session_id": "string",
  "workspace_name": "string | null",
  "total_turns": 42,
  "retrieved_turns": 10,
  "turns": [
    {
      "user": "string",
      "assistant": "string",
      "timestamp": null,
      "tool_invocations": []
    }
  ]
}
```

**Known limitations:**

- Timestamps are always `null` (VS Code doesn't persist them reliably)
- Large files (>10MB) skip metadata parsing, breaking recency ordering
- `match-text` only searches user messages, not assistant/tool content
- Tool invocations only extracted from `toolInvocationSerialized` entries

### CLI: exo ai chat-history

Located in `tools/exo/src/command/ai.rs`. Wraps `exohistory` output in standard CLI envelope.

**Arguments:**

- `--turns N` — Number of turns to retrieve (default: 10, max: 50)
- `--include-thinking` — Include extended thinking content
- `--include-tools` — Include tool invocations
- `--match-text TEXT` — Find session containing this text in user messages
- `--workspace-uri URI` — Match specific workspace

### LM Tool: exo-ai-chat-history

Registered in VS Code extension. Routes through CLI via machine channel for unified contract.

**Session discovery:**

- Always filters by current workspace URI (defaults automatically)
- Within the workspace, selects the most recently modified session file
- For JSONL sessions (the current format), file mtime reflects the last write — so the "current chat" is typically the most recent
- Use `--match-text` to find a specific previous session by content

**When to use:**

- Recovering context at session start (just call with no args — gets current workspace's most recent session)
- Finding a previous session that discussed a specific topic (use `--match-text`)
- Debugging what happened in a previous conversation

## Session Handoff Protocol

When context window fills up or user requests a new session:

### 1. Context Sync

Ensure agent-context files are up to date before handoff:

- `implementation-plan.toml` reflects current state
- Any in-progress work is logged

### 2. Context Extraction

Use `exo-ai-chat-history` with `--match-text` to identify the session to recover from.

### 3. Recovery

In the new session, the agent can:

- Read recent turns to understand what was being worked on
- Check `exo status` for current phase/goals
- Resume work with full context

## Future Vision (Aspirational)

The current `exohistory` approach solves immediate needs. Future enhancements:

- **SQLite backend**: Persistent, indexed storage (`.exosuit/memory.db`)
- **Semantic search**: Vector embeddings for "why did we choose X?" queries
- **Loop detection integration**: Wire `exohistory analyze` into steering
- **Automatic handoff triggers**: Detect when context window is filling up

## Privacy

- All data is local-only (VS Code's existing storage)
- No data leaves the machine
- Users can clear VS Code's chat history through normal VS Code mechanisms
