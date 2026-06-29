<!-- exo:10183 ulid:01knn6geky8j571wnmc5wk7jhe -->

# RFC 10183: Agent Activity Model: Event Sourcing for Steering Context

## Summary

An event-sourcing architecture that captures agent activity into SQLite and derives projections that steering can query. Three layers with distinct responsibilities:

1. **Event Log** — append-only capture of what happened (facts)
2. **Projections** — derived state computed on demand from events (meaning)
3. **Reactions** — steering queries projections to enrich responses (attention)

This is the producer infrastructure for RFC 10182's Level 3 steering. It provides the implicit entity context and activity signals that contextual steering needs to go beyond explicit command-level entity IDs.

## Motivation

### The perception gap

RFC 10182 designs three levels of steering resolution: project (orientation), command (entity-scoped), and activity (behavior-inferred). Levels 1 and 2 are achievable with existing data. Level 3 requires knowing what the agent has been *doing* — which entities it's been touching, how long it's been working, whether it's drifting from the plan.

Today, there is no record of agent activity. The daemon handles requests statelessly — each request is independent, with no memory of what came before. Session boundary detection in `session_boundary.rs` uses heuristics (git dirty, in-progress tasks) to guess session state because it has no actual activity data to reason from.

### The three-layer separation

The previous attempt to connect Copilot hooks to steering didn't land because it tried to go directly from raw events to steering decisions — sensory input to attention without perceptual processing. The missing layer is projections: derived state that answers "what does this activity *mean*?"

| Layer | Analogy | Responsibility |
|-------|---------|---------------|
| Event Log | Sensory input | Record what happened, faithfully |
| Projections | Perceptual processing | Compute what it means |
| Reactions | Attention | Focus steering on what matters now |

Separating capture from interpretation means the event log can be simple and dumb (append-only INSERTs), while projections can be sophisticated and evolving (swap out inference logic without touching the capture path).

### Independent value

Even without Level 3 steering, the event log improves:

- **Session boundary detection**: Replace heuristics with "time since last event" — near-certain detection of compaction, sleep, and fresh sessions.
- **Debugging**: "What did the agent do in the last 5 minutes?" becomes a SQL query.
- **Session continuity**: SESSION-TRAJECTORY handoffs can include "last session's activity summary" computed from events rather than manually maintained.

## Design

### Event Log

An append-only SQLite table. NOT a reactive vtab — events don't need trace invalidation, and the sidebar shouldn't refresh on every tool call.

```sql
CREATE TABLE agent_events (
    id          INTEGER PRIMARY KEY,
    text_id     TEXT NOT NULL UNIQUE,
    timestamp   TEXT NOT NULL,
    agent_id    TEXT,
    event_type  TEXT NOT NULL
                CHECK (event_type IN ('command', 'file_save')),
    namespace   TEXT,
    operation   TEXT,
    entity_type TEXT,
    entity_id   TEXT,
    effect      TEXT CHECK (effect IS NULL OR effect IN ('read', 'write')),
    duration_ms INTEGER,
    summary     TEXT NOT NULL
);

CREATE INDEX idx_agent_events_timestamp ON agent_events(timestamp);
CREATE INDEX idx_agent_events_session ON agent_events(agent_id, timestamp);
CREATE INDEX idx_agent_events_entity ON agent_events(entity_type, entity_id);
```

**Privacy**: Only structural metadata. No command input payloads, no response bodies, no file contents. The `summary` field is a server-generated one-liner (e.g., "task complete auth-task" or "file saved: src/auth/mod.rs").

**Retention**: Events older than 7 days are deleted on daemon startup. The table stays small — an active session produces tens to low hundreds of events.

### Event Sources

#### Source 1: exo commands (daemon-side capture)

The daemon already sees every request and response. The capture point is in the request handler, after command execution:

```rust
// In handle_call_with_namespace_operation(), after invoke_command_box_json()
log_agent_event(&writer, AgentEvent {
    agent_id: request.agent_id,
    event_type: "command",
    namespace,
    operation,
    entity_type: inferred_entity_type,
    entity_id: inferred_entity_id,
    effect: response.effect,
    duration_ms: elapsed,
    summary: response.display.summary,
});
```

Entity inference from command input: task/goal commands pass the entity ID as a positional argument. The handler already parses this into an `Invocation`. The event logger reads it from the same parsed structure.

#### Source 2: file saves (extension-side notification)

The extension registers `vscode.workspace.onDidSaveTextDocument` and sends a lightweight notification to the daemon:

```typescript
vscode.workspace.onDidSaveTextDocument((doc) => {
  const relativePath = vscode.workspace.asRelativePath(doc.uri);
  daemon.notify({
    event_type: "file_save",
    summary: `file saved: ${relativePath}`,
  });
});
```

This uses a new one-way notification in the NDJSON protocol (similar to `write_happened` but inbound). The daemon writes to the event log without sending a response.

File saves are the primary signal for inferring which code areas the agent is touching. They don't require any new VS Code API — `onDidSaveTextDocument` is stable.

### Projections (computed on demand)

Projections are SQL queries over the event log, not materialized tables. With <1000 events per session, aggregation is sub-millisecond.

#### `last_event_at`

```sql
SELECT MAX(timestamp) FROM agent_events
WHERE agent_id = ?1 OR agent_id IS NULL
```

Used by: session boundary detection, idle timeout, "how long since last activity?"

#### `active_entity`

```sql
SELECT entity_type, entity_id, COUNT(*) as freq
FROM agent_events
  AND timestamp > datetime('now', '-10 minutes')
  AND entity_type IS NOT NULL
GROUP BY entity_type, entity_id
ORDER BY freq DESC
LIMIT 1
```

Used by: Level 3 steering (RFC 10182) to infer implicit entity context when a command doesn't carry one. Window: 10 minutes (tight — "what is the agent working on right now?").

#### `session_window`

```sql
SELECT MIN(timestamp) as session_start,
       MAX(timestamp) as last_activity,
       COUNT(*) as event_count
FROM agent_events
WHERE timestamp > (
    SELECT COALESCE(
        (SELECT timestamp FROM agent_events
         WHERE timestamp < (SELECT MAX(timestamp) FROM agent_events)
         AND julianday((SELECT MAX(timestamp) FROM agent_events)) - julianday(timestamp) > 0.02  -- ~30 min gap
         ORDER BY timestamp DESC LIMIT 1),
        (SELECT MIN(timestamp) FROM agent_events)
    )
)
```

Used by: session boundary detection (replacing heuristics), session effort estimation.

#### `recent_file_areas`

```sql
SELECT DISTINCT
    substr(summary, 13) as file_path,  -- strip "file saved: " prefix
    COUNT(*) as save_count
FROM agent_events
WHERE event_type = 'file_save'
  AND timestamp > datetime('now', '-30 minutes')
GROUP BY file_path
ORDER BY save_count DESC
LIMIT 10
```

Used by: drift detection (are file saves in areas related to the active entity?). Window: 15 minutes (current work burst, not whole session).

### Reactions (how steering consumes projections)

RFC 10182's `derive_entity_steering()` gains an optional `ActivityContext` parameter:

```rust
pub struct ActivityContext {
    pub active_entity: Option<(String, String)>,  // (entity_type, entity_id)
    pub session_duration_mins: u32,
    pub event_count: u32,
    pub recent_file_areas: Vec<String>,
}
```

This is computed by querying the projections. Steering uses it for:

- **Implicit entity scoping**: If a command has no explicit entity but the activity model says "the agent has been working on auth-task," scope perception to that entity tree.
- **Effort-based nudges**: "You've been on this task for 45 minutes without logging progress. Consider `exo task log`."
- **Drift warnings**: "Recent file edits are in `src/billing/` but your active task is under the `auth-system` goal."
- **Cross-session continuity**: When `session_boundary` detects a new session, steering includes a summary of the previous session: event count, duration, primary entity, last action. Computed from the event log — replaces what SESSION-TRAJECTORY.md does manually.

### Agent ID scoping

Each Copilot chat session has a unique `chatSessionResource` URI as `agent_id`. Projections scope by `agent_id`. Events with `agent_id IS NULL` (CLI usage) are "ambient" — included in all sessions' views since they represent human activity relevant to every agent.

```sql
WHERE (agent_id = ?1 OR agent_id IS NULL)
```

### Protocol: Inbound Notifications

The NDJSON protocol gains a new inbound notification type (extension → daemon, no response expected):

```json
{"kind": "activity_event", "event_type": "file_save", "summary": "file saved: src/auth/mod.rs"}
```

The daemon writes to `agent_events` and does not send a response. Uses the same NDJSON connection — no separate socket. This mirrors `write_happened` (outbound notification) but in reverse.

## Implementation Phases

### Phase 1: Event log + daemon capture

- V016 migration: `agent_events` table
- Capture exo commands in `handle_call_with_namespace_operation()`
- Entity inference from command input
- Retention cleanup on daemon startup
- No extension changes, no new protocol messages

### Phase 2: File save watcher + session improvement

- Extension: `onDidSaveTextDocument` → daemon notification
- Daemon: inbound notification handler → event log INSERT
- Replace `session_boundary.rs` heuristics with event-based detection
- `last_event_at` and `session_window` projections

### Phase 3: Projection queries + RFC 10182 Level 3

- `active_entity` and `recent_file_areas` projections
- `ActivityContext` struct fed to `derive_entity_steering()`
- Drift detection: cross-reference file areas with entity scope
- Effort-based steering nudges

## Relationship to existing RFCs

- **RFC 10182** (Contextual Steering): The consumer. Level 2 (command-scoped) is independent. Level 3 (activity-inferred) reads from this RFC's projections.
- **RFC 10181** (Shared Perception): Inbox events are human→agent communication. Activity events are agent behavior observations. Both feed steering, from different angles.
- **RFC 10170** (Mutation Boundaries): Activity events ARE observations at mutation boundaries. This RFC operationalizes the principle.
- **RFC 00242** (Progress Tool): `task log` is both a steering touchpoint (10182) and an activity event (this RFC). The log call produces steering AND records the event.

## Resolved questions

1. **File path privacy**: Yes, capture the full relative path. The data stays in local SQLite, same security boundary as inbox items and task logs. The path is essential for drift detection.

2. **Inbound notification protocol**: Same NDJSON connection, `kind: "notify"`. No separate socket. Resolved for Phase 2 — Phase 1 doesn't need extension notifications.

3. **Projection window sizes**: Hardcoded per projection, not configurable. 10 min for `active_entity` (tight: "what is the agent doing now?"), 15 min for `recent_file_areas` (current work burst), 30 min gap for `session_window` (session boundary). Tunable empirically in Phase 3.

4. **Cross-session continuity**: Yes. When session boundary is detected, steering includes a one-line summary of the previous session (event count, duration, primary entity, last action). Computed from the event log. Phase 3 work.

5. **Concurrent agent sessions**: `agent_id` scoping is sufficient. `agent_id IS NULL` events are "ambient" (CLI usage, human activity) — included in all sessions' views. No session-level partitioning needed.

## Unresolved questions

1. **Entity inference accuracy**: How reliably can we extract entity_type/entity_id from command input? Task/goal commands are straightforward (positional ID argument), but compound commands or piped workflows may be ambiguous. Needs empirical testing in Phase 1.

2. **Daemon startup retention cleanup**: Should cleanup be synchronous (blocks daemon startup) or async (runs in background after first accept)? With 7-day retention and low event volume, this is likely trivial either way.
