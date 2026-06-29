<!-- exo:10181 ulid:01kmzxbcz13dh6eft68dcwxc23 -->

# RFC 10181: Shared Perception: Inbox as a Steering Channel

# Shared Perception: Inbox as a Steering Channel

## Summary

Redefine the inbox from a triage queue into a **steering channel** — a unified mechanism for user feedback, system observations, and plan mutation events to enter the agent's perception at the right time and in the right amount.

The core problem: two collaborators (human and AI) maintain a shared artifact (the project plan) but can't see each other's screens. The user sees the sidebar. The agent sees steering responses. When either side changes something, the other needs to perceive it — not immediately and not in full, but at the right moment with the right granularity.

## Motivation

### The Shared Perception Gap

Today, when the user interacts with the sidebar — promoting an idea, reordering goals, providing feedback on a task — the agent doesn't know. It discovers changes indirectly by reading the plan and noticing deltas. This is fragile, sometimes too late, and prevents the user from safely making direct plan changes (we keep pulling back from adding UI controls because of the confusion risk).

The `get_errors` tool shows what this looks like when it works: the user sees red squiggles, the agent can see them too, and both react to the same reality. This RFC applies that pattern to the project plan.

### Three Sources, One Channel

The inbox currently conflates three distinct use cases:

1. **User feedback** — "This goal description is wrong" or "I think this task is done, can you check?"
2. **System observations** — "You haven't updated goal Y in a while" (future: hook-driven)
3. **Plan mutations** — "User promoted idea X to a goal in this phase"

These differ in source and urgency, not in kind. They all represent "something the agent should know about, at the right time."

### Quick-Stash Items Are Ideas

Global, unscoped inbox items ("bugs", "things I noticed") are really backlog items. They belong in `exo idea add`, not the inbox. The inbox should always be scoped to an entity.

## Design

### Core Model: Perception Events

An inbox item is a **perception event** — something that should influence the agent's behavior, delivered via steering at action boundaries.

Every perception event has:

- **Entity scope** — what is this about? (goal, task, RFC, phase, epoch, project)
- **Source** — who created it?
  - `user-feedback` — the user typed something via a feedback button
  - `system-observation` — the daemon detected something (future: hooks)
  - `plan-mutation` — a plan command was executed (goal added, task completed, RFC promoted)
- **Priority** — how urgently should the agent see this?
  - `immediate` — surface in the next steering response
  - `next-touch` — surface when the agent next interacts with the scoped entity
  - `when-relevant` — surface when contextually appropriate
- **Intent** — what the creator is communicating
  - `claim` — "I believe something about this entity's state" (with `confidence: high | low`)
  - `concern` — "Something about this worries me"
  - `inquiry` — "What's the status of this?"
  - `fyi` — "Just be aware of this" (also serves as an escape valve for anything that doesn't fit the other three)

Intent describes the _sender's_ communicative purpose, not the agent's expected behavior. The steering layer interprets source + priority + intent to compose the right message. For example:

| Source             | Intent       | Steering message                                           |
| ------------------ | ------------ | ---------------------------------------------------------- |
| user-feedback      | claim (high) | "User believes goal X is complete — verify and close"      |
| user-feedback      | claim (low)  | "User is wondering if goal X might be done — check status" |
| user-feedback      | concern      | "User has feedback on goal X"                              |
| user-feedback      | inquiry      | "User is asking for a status update on goal X"             |
| plan-mutation      | fyi          | "Goal Y was added to this phase"                           |
| system-observation | concern      | "Task Z hasn't been updated in a while"                    |

### Steering Delivers Signals, Not Payloads

The steering response carries **summaries**, not full messages:

```
Feedback: 2 items on goal 'Widget Refactor' (1 user, 1 system)
  → Review: exo-run("inbox list --subject-ref goal:widget-refactor")
```

The agent decides when to drill in. This prevents deluging — the agent gets a one-line signal and pulls details when ready.

**Entity closure forces delivery.** When the agent attempts to complete a goal or task, the system checks for unresolved feedback items scoped to that entity. If any exist, they are surfaced in full: "3 unresolved feedback items — resolve before completing."

### Plan Mutations Emit Events Automatically

When a plan command executes, it writes the state change AND creates a perception event in one atomic operation:

- `goal add "Widget Refactor"` → goal created + inbox item: "Goal 'Widget Refactor' added to phase X"
- `inbox resolve <id> --promote goal` → goal created + inbox resolved + notification: "Idea promoted to goal"
- `rfc promote 00238` → RFC promoted + inbox item: "RFC 00238 promoted to Stage 2"

**Suppression rule:** Events created via `exo-run` carry the invoking agent's ID. That agent's own steering suppresses its own events (it already knows what it did). Other agents and the user's next session see them.

### Agent Identity

Each `exo-run` invocation carries a **session identity** derived from VS Code's `chatSessionResource` — a URI that uniquely identifies the chat conversation. This is **client-owned**: the extension reads it from the tool invocation context and sends it to the daemon with every request.

**How it works at runtime**: VS Code passes a `toolInvocationToken` to every tool `invoke()` call. At runtime, this object is an `IToolInvocationContext` containing a `sessionResource: URI` property. This URI is stable across all tool calls within the same conversation and unique across conversations. The proposed `chatParticipantPrivate` API exposes this as `options.chatSessionResource`, but it's also accessible at runtime via the opaque token.

The extension reads `sessionResource.toString()` as the agent identity and sends it to the daemon on every request. This means:

- The ID survives daemon reconnections (it's derived from the conversation, not the connection)
- The daemon just stores whatever `agent-id` the client sends — no ID management on the server side
- Multiple conversations in the same VS Code window each have distinct session URIs

This enables:

- **Self-suppression** — steering filters out events where `source-agent-id == my-agent-id`
- **Cross-agent notification** — agent A's changes surface in agent B's steering
- **User attribution** — sidebar actions don't go through `exo-run`, so they carry no agent ID. No ID = from the user = surfaces to all agents

**Stability note**: `chatSessionResource` is not part of the stable VS Code API surface — it's behind the `chatParticipantPrivate` proposed API. However, the runtime object is always present on `toolInvocationToken` (it's how VS Code internally associates tool calls with chat sessions). We accept this dependency with the understanding that it may need updating as VS Code stabilizes these APIs.

### The "I Think This Is Done" Button

Rather than a "Complete" button that directly changes state, entities get a **"I Think This Is Done"** button. Pressing it creates a perception event:

- `source: user-feedback`
- `intent: claim` (confidence: high)
- `priority: immediate`
- Scoped to the goal/task

The steering layer composes: "User believes goal X is complete — verify and close." The agent runs verification (tests, review, walkthrough) and either confirms or explains what's still missing. The state change only happens after agent verification.

A lower-confidence variant — "Can you check if this is done?" — uses `claim` with `confidence: low`, producing softer steering: "User is wondering if goal X might be done — check status."

This connects naturally to the walkthrough pattern. When a completion claim arrives on a complex entity (a goal with multiple tasks, or a phase), the agent's natural response is a structured walkthrough: review changes chunk by chunk, address pending feedback items, confirm or identify gaps. The perception system delivers the trigger; the walkthrough skill does the work.

### Feedback Buttons on All Entities

Every entity in the sidebar (goals, tasks, RFCs, phases) gets a feedback button. Clicking it opens a text input. The feedback becomes a perception event scoped to that entity, surfaced when the agent next touches it (or at entity closure).

This applies to RFCs too — "this conflicts with what we discovered" becomes scoped feedback on the RFC, surfaced when the agent works on that RFC or attempts to promote it.

### Ideas as the Generalized Backlog

Ideas absorb what was previously "global inbox items":

- Bugs → `exo idea add --kind bug "description"`
- Feature thoughts → `exo idea add "description"` (default kind: `thought`)
- Improvements → `exo idea add --kind improvement "description"`

Ideas are unscoped, untimed, no urgency. They're candidates for future promotion to goals. The `kind` field adds flavor without changing the concept.

## Phasing

Phases are defined by dependency order, not by epoch mapping. The project plan decides when to schedule each phase.

### Phase 1: Foundation (DONE)

- Consolidated inbox schema to perception event model (V013: entity_type/entity_id, source, intent, priority, confidence)
- Moved global inbox items to ideas
- Reactive vtab self-healing: Database::new() unconditionally rebuilds vtabs
- DaemonChannelServer reconnect → sidebar refresh
- Dev workflow: build-ext-dev, open-dev-host.sh, fixed double-bundling

### Phase 2: Perception Pipeline (DONE)

- Agent ID plumbed end-to-end: toolInvocationToken → RequestEnvelope → CommandContext → inbox storage (V014)
- Inbox drill-in filters: --entity-type, --entity-id, --source
- **RFC metadata made reactive**: rfcs_data table + reactive vtab (V015), 317 files migrated to ULID anchor format, frontmatter moved to SQLite
- **RFC reconciliation**: startup scan syncs disk state to SQLite; create/promote/edit/withdraw/archive/supersede all maintain the anchor format contract
- **Steering self-suppression**: agent's own inbox items filtered from steering output
- **Perception signal summaries**: grouped by entity with count, priority, and drill-in commands

**Design evolution**: Plan-mutation inbox events were originally planned for Phase 2 but were replaced by the insight that the SQLite reactivity system already handles data freshness for entities that live in SQLite. The inbox is reserved for human-to-agent communication (feedback, claims, concerns, inquiries). RFC metadata was the gap — it lived on disk outside the reactive system — and bringing it into SQLite closed that gap without inbox event noise.

### Phase 3: User Interaction Surface

Depends on Phase 2 (perception pipeline must exist for feedback to flow).

- Feedback buttons on sidebar entities
- "I Think This Is Done" button (verify-completion claim)
- Entity closure gate (must resolve feedback before completing)
- System observation events (hook-driven)

### Consolidated Schema

The current inbox schema has accumulated fields from different eras (`scope_type`/`scope_value`, `subject_ref_type`/`subject_ref_id`, `category`, `urgency`, `action_type`/`action_payload`). The perception event model consolidates these into orthogonal fields:

| Field                       | Purpose                                                                                                                                        |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `entity_type` + `entity_id` | What this is about (goal, task, RFC, phase, epoch, project). Replaces both `scope_type`/`scope_value` and `subject_ref_type`/`subject_ref_id`. |
| `source`                    | Who created it (user-feedback, system-observation, plan-mutation)                                                                              |
| `intent`                    | Why it was created (claim, concern, inquiry, fyi)                                                                                              |
| `priority`                  | When to surface (immediate, next-touch, when-relevant). Replaces `urgency`.                                                                    |
| `confidence`                | Strength of a claim (high, low, null for non-claims)                                                                                           |
| `agent_id`                  | Which agent created it (null = user/sidebar)                                                                                                   |
| `subject` + `body`          | The content                                                                                                                                    |
| `status`                    | Lifecycle (pending, acknowledged, resolved, archived)                                                                                          |

`category` (correction/guidance/question/priority) is subsumed by `intent` + `priority`: a correction is a concern, guidance is fyi, a question is an inquiry, and priority is a concern with immediate priority. `action_type`/`action_payload` is replaced by `intent` — the steering layer decides what to recommend, not the data.

## Drawbacks

- **Plan mutation abstraction is non-trivial.** Every command that mutates plan state must emit a perception event atomically. This forces creation of a shared mutation abstraction (e.g., a trait or wrapper). The upside: that abstraction becomes a substrate for future mutation-related features (undo, audit log, conflict detection). The downside: it must be built before any Phase 2 work can start.
- **Agent ID adds a protocol concept.** `chatSessionResource` resolves the identity question, but the `RequestEnvelope` needs a new `agent_id` field, and the daemon must store it. Mechanical work, but it touches the wire protocol.
- **Over-notification risk.** Even with signals-not-payloads, rapid mutations generate many events. Self-suppression handles the common case (agent doesn't see its own). For cross-session visibility, the per-session acknowledgment cursor (high-water mark) ensures new sessions see a summary ("10 plan changes since last session") rather than a flood.

## Alternatives

- **Keep inbox as-is** — a triage queue with manual items. Doesn't solve shared perception.
- **VS Code comments API** — could provide feedback UI, but doesn't integrate with steering or plan state.
- **Derive perception from SQLite diffs** — the git-friendly SQL dumps already capture what changed between commits. Steering could diff the dumps instead of maintaining explicit perception events. This tells you _what_ changed but not _why_, _who_, or _with what intent_. The diff and perception events aren't mutually exclusive — the diff serves as a fallback if a mutation fails to emit an event.
- **Use VS Code agent memory for session context** — time-bound, person-level constraints ("I'm on vacation Friday") could live in `/memories/session/` instead of the inbox. Agent memory is personal and ephemeral (per machine); perception events are checked in and shared. Both can coexist — memory for private context, perception events for shared project context.

## Unresolved Questions

- What's the best UX for feedback input? `showInputBox` is simple but single-line. Multi-line feedback ("this RFC conflicts with...") might need a comment-thread-style UI or a webview.
- How should the per-session acknowledgment cursor (high-water mark) be stored? Per-agent-ID in the daemon? In the steering response itself?
- Should the plan mutation abstraction be a Rust trait, a wrapper around `SqliteWriter`, or a higher-level command middleware? The choice affects how much existing code needs to change.

## Future Possibilities

- Hook-driven system observations ("agent hasn't committed in 20 minutes")
- Feedback threading (responses to feedback create a conversation)
- Cross-workspace perception (changes in a dependency workspace notify the parent)
- User perception of agent actions (the sidebar equivalent — "agent just completed task X")
- Cross-agent visibility in the sidebar: when multiple agents are active, show the user what each agent is doing via their perception events
- Team collaboration: threads of work with separate pointers into the project plan, so different people can work on different epochs without requiring linear progression. The current single-cursor model assumes one active epoch/phase; teams need parallel workstreams.
