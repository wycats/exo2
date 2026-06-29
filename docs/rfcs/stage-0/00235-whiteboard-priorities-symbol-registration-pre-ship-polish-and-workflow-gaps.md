<!-- exo:235 ulid:01kmzxefe2ypkzsnsq2gr825r3 -->

# RFC 235: Whiteboard Priorities: Symbol Registration, Pre-Ship Polish, and Workflow Gaps


# RFC 00235: Whiteboard Priorities: Symbol Registration, Pre-Ship Polish, and Workflow Gaps

## Summary

Captures priorities from a whiteboard review (2026-02-06) and structures them into three actionable threads. The whiteboard represents shipping-oriented priorities — things that cause friction when trying to share or ship Exosuit.

## Motivation

The current Situational Awareness epoch has two remaining phases (Inbox Badging, ExoSpec), but a whiteboard review revealed higher-priority concerns not captured in any epoch. This RFC captures those priorities and proposes a phase to explore them.

## Thread 1: Symbol Registration Spike

**Goal**: Use `WorkspaceSymbolProvider` + `TextDocumentContentProvider` to expose plan entities (RFCs, phases, epochs) as chat-referenceable symbols.

### What We Know

- `WorkspaceSymbolProvider` symbols appear in the chat context **picker** (not inline `#sym` autocomplete, which uses document symbols from outline cache)
- Symbols can point to virtual documents via `TextDocumentContentProvider` with custom URI schemes (e.g., `exo://rfc/00232`)
- The agent sees a code excerpt for the symbol's location range, plus a summary of the surrounding document
- `SymbolKind` includes general kinds (`File`, `Module`, `Namespace`) — not limited to code constructs
- No API limit on symbol count; Copilot renders top 20 results in tool output
- `containerName` field allows grouping (e.g., "RFCs", "Phases", "Epochs")

### Open Questions (Require Prototyping)

1. Does manual `#sym:Name` typing work, or must symbols be inserted via completion picker?
2. Does `TextDocumentContentProvider` content get read correctly by Copilot Chat for custom URI schemes?
3. What's the token budget behavior for virtual documents? Full content or summarized?
4. Can we influence ranking to prioritize plan entities over code symbols?
5. What's the UX of the picker — is it natural to find "RFC 00232" among code symbols?

### Why This Matters

If symbols work well, it could:

- Make RFC content available in chat without custom rendering UI
- Let agents reference `#sym:current-phase` instead of calling `exo-status`
- Reduce the urgency of several UI/visualization items on the whiteboard
- Cut a Gordian knot between "more UI" and "more agent context"

## Thread 2: Pre-Ship Polish

Items with ⚠️ warning markers on the whiteboard — shipping blockers:

| Item                            | Why It Blocks Shipping                                            |
| ------------------------------- | ----------------------------------------------------------------- |
| **RFC pipeline mental model**   | Users can't understand how stages flow or what triggers promotion |
| **Copying instructions/agents** | Can't clone the repo and get started                              |
| **exohook → test explorer**     | Long-running terminal commands confuse agents                     |
| **Bootstrap**                   | No onboarding story                                               |

## Thread 3: Workflow Gaps

Items that affect daily workflow quality:

| Item                                | Description                                     |
| ----------------------------------- | ----------------------------------------------- |
| **Re-enter phase**                  | What happens when a phase finish was premature? |
| **"Quick" chore phase**             | Lightweight interstitial phases (RFC 00231)     |
| **Remove/invalidate goal/task**     | Ability to abandon work items                   |
| **Remove "step" more aggressively** | Clean up remaining impl step references         |
| **fix `< full`, should stage**      | Exohook behavior in non-full mode               |

## Relationship to Shipping Docs

The [shipping brainstorming docs](../../brainstorming/2026-01-23-workflow-revamp/shipping/) identify 5 core disconnects:

| #   | Disconnect            | This RFC Addresses                              |
| --- | --------------------- | ----------------------------------------------- |
| 1   | Workflow vs Practice  | ✅ Already addressed (SOAR)                     |
| 2   | Visibility            | Thread 1 (symbols as alternative to custom UI)  |
| 3   | Idea Integration      | Partially — inbox badging deferred, not dropped |
| 4   | RFC/Phase Integration | Thread 2 (RFC pipeline mental model)            |
| 5   | Lost Concepts         | Thread 3 (workflow gaps)                        |

## Implementation Approach

Start with Thread 1 (symbol spike) as it's low-cost and could reshape priorities for Threads 2-3. Then assess which Thread 2/3 items are true shipping blockers vs nice-to-haves.

## Whiteboard Source

Transcribed from physical whiteboard photo, 2026-02-06. Items marked done (crossed out): reactivity changes, StatusBar w/ reactivity, SOAR.

