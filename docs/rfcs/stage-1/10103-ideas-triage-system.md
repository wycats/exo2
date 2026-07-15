<!-- exo:10103 ulid:01kmzxey1bjzga87qd1cx185zj -->


# RFC 10103: Ideas & Triage System

- **Superseded by**: RFC 0011


## Problem
Currently, `ideas.md` is a free-form unstructured list. This makes it difficult to:
1. Programmatically manage ideas (sort, filter, promote).
2. Allow an AI agent to triage ideas in the background without disrupting the main workflow.
3. Capture metadata like creation time, source, and status.

## Solution: `ideas.toml`

We will migrate `ideas.md` to `ideas.toml` with a structured schema.

### Schema

```toml
[[ideas]]
id = "unique-id-uuid-or-slug"
title = "Short summary of the idea"
description = """
Detailed description of the idea.
Can include markdown.
"""
status = "new" # new, triaged, accepted, rejected, deferred, implemented
created_at = "2025-12-01T12:00:00Z"
source = "user" # user, agent, chat-context
tags = ["ui", "refactor"]
related_tasks = ["task-id-1"]
```

## Background Triage Workflow

We want to enable "background triage" where an AI agent reviews new ideas and helps organize them.

### The `@exosuit-triage` Participant

We will introduce a specialized chat participant (or a mode of the main participant) dedicated to triage.

**Responsibilities:**
1. **Ingest**: Listen for "I have an idea..." or explicit `@exosuit-triage add "..."` commands.
2. **Refine**: Ask clarifying questions if the idea is vague (but do this asynchronously or in a non-blocking way? Or maybe just flag it as "needs-info").
3. **Categorize**: Auto-tag ideas based on content.
4. **Deduplicate**: Check against existing ideas and tasks.
5. **Promote**: Suggest promoting ideas to Tasks or Decisions when they are mature.

### Interaction Model

**User**: "I have an idea: we should use a vector db for context."
**@exosuit**: "Captured in `ideas.toml` as 'Vector DB for Context' (Status: New)."

*(Later, in background or on demand)*

**User**: "@exosuit triage"
**@exosuit**: "I found 3 new ideas.
1. **Vector DB for Context**: This seems related to Phase 16. Should we link it?
2. **Fix typo in README**: This is small. Should I just create a task?
..."

## Transition Plan

1. **Define Schema**: Add `Idea` schema to `exosuit-core`.
2. **Migrate Data**: Convert `ideas.md` to `ideas.toml`.
3. **Update UI**: Create a "Ideas" view in the sidebar (or a section in the Dashboard).
4. **Implement Triage Logic**: Build the logic to analyze and update `ideas.toml`.
5. **Create Participant**: Register `@exosuit-triage` (or add triage capabilities to `@exosuit`).
