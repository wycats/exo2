<!-- exo:116 ulid:01kg5kp2gty476sv5b6h51hn65 -->

# RFC 116: Feedback System

- **Status**: Archived
- **Stage**: 4
- **Reason**:

# RFC 0116: Feedback System

- **Superseded by**: RFC 0124


**Phase**: 22.5

## 1. Overview

The Feedback System enables a persistent, asynchronous conversation between the user and the agent directly attached to the project's context artifacts (Axioms, Decisions, Plan). Unlike chat messages which scroll away, Feedback is "sticky" and must be explicitly resolved.

## 2. Data Model (`feedback.toml`)

We will use a "Sidecar" file `docs/agent-context/feedback.toml` to store all feedback items. This keeps the core artifacts clean and allows for independent versioning of the conversation.

### Schema

```toml
# docs/agent-context/feedback.toml

[[threads]]
id = "fb-1234567890"
target_file = "docs/agent-context/decisions.toml"
target_id = "2025-12-02-unified-project-state" # Optional: ID of the specific item
target_field = "context" # Optional: Specific field being discussed
status = "open" # open | proposed-resolved | resolved | archived
created_at = "2025-12-03T10:00:00Z"
updated_at = "2025-12-03T10:05:00Z"

  [[threads.messages]]
  id = "msg-1"
  author = "user" # user | agent
  content = "This context seems a bit vague. Can we clarify?"
  created_at = "2025-12-03T10:00:00Z"

  [[threads.messages]]
  id = "msg-2"
  author = "agent"
  content = "Agreed. I suggest adding specific examples of fragmentation."
  created_at = "2025-12-03T10:05:00Z"
```

## 3. Content Structure (RSS)

We use **RTD Surface Syntax (RSS)** to define the structure of the feedback content.

### 3.1 Feedback List Item

Each item in the feedback list is an RTD structure.

```html
<li>
  <p><strong>decisions.toml</strong> (Open)</p>
  <p>This context seems a bit vague...</p>
  <rtd-command id="exosuit.feedback.open" args='["fb-1234567890"]'>Open</rtd-command>
</li>
```

### 3.2 Feedback Thread (Overlay Content)

The conversation history is rendered as a list of messages.

```html
<h3>Feedback: Context</h3>
<ul>
  <li>
    <p><strong>User</strong>: This context seems a bit vague. Can we clarify?</p>
  </li>
  <li>
    <p><strong>Agent</strong>: Agreed. I suggest adding specific examples of fragmentation.</p>
  </li>
</ul>
<rtd-command id="exosuit.feedback.resolve">Mark Resolved</rtd-command>
```

### 3.3 Resolution Request

```html
<p>The agent has marked this thread as <strong>Proposed Resolved</strong>.</p>
<blockquote>
  <p>I have updated the decision context as requested.</p>
</blockquote>
<p>
  <rtd-command id="exosuit.feedback.accept">Accept & Close</rtd-command>
  <rtd-command id="exosuit.feedback.reject">Reject (Re-open)</rtd-command>
</p>
```

## 4. Integration Strategy

1.  **Core**: Add `FeedbackService` to `exosuit-core`. It watches `feedback.toml` and exposes a reactive store.
2.  **Editor**: The `RichContextEditor` (Svelte) subscribes to the store.
3.  **Visuals**:
    - Fields with active feedback get a visual indicator (e.g., a yellow border or icon).
    - Clicking the indicator opens the `FeedbackOverlay`.
