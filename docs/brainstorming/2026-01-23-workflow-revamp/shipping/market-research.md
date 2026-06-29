# Market Research: The AI Coding Landscape vs. Exosuit

> **Originally**: 2026-01-23 | **Updated**: 2026-02-03
> **Objective**: Identify the "Status Quo" of AI coding tools to highlight Exosuit's unique value proposition.

## The Competitive Landscape

### 1. The "Assistant" (GitHub Copilot)

- **Model**: "Autocomplete on Steroids" & Sidebar Chat.
- **Workflow**: Stateless interaction. The user is the driver; Copilot is the passenger offering navigation tips.
- **Context**: Limited to open tabs and recent cursor history (though expanding with "Workspace").
- **Weakness**: No concept of "project state" or "multi-step plans". It forgets what you agreed on 5 minutes ago if it falls out of the context window.

### 2. The "AI-Native Editor" (Cursor)

- **Model**: Forked VS Code with deep LLM integration.
- **Workflow**: "Chat to Diff". You ask for a feature; it applies changes across multiple files.
- **Context**: Better RAG (Retrieval Augmented Generation) across the codebase.
- **Weakness**: It's still "Command & Control". You give a command; it executes. It doesn't help you _plan_ or _verify_ beyond simple compilation. It creates "smooth" code that might be architecturally incoherent.

### 3. The "Autonomous Agent" (Devin)

- **Model**: "Fire and Forget". You give a high-level goal ("Build a Snake game"); it plans, codes, debugs, and deploys.
- **Workflow**: User is a client; Devin is a contractor.
- **Weakness**: The "Black Box" problem. When it inevitably gets stuck or hallucinates, the user has to dive into a messy, generated codebase to rescue it. It lacks "Collaboration" — it tries to solve _for_ you, not _with_ you.

### 4. The "CLI Pair Programmer" (Aider)

- **Model**: Chat inside the terminal.
- **Workflow**: Git-centric. "Edit these files to do X".
- **Context**: Uses the "Repository Map" (compact syntax tree) to fit large context.
- **Weakness**: Tactical, not strategic. Excellent for "implement this ticket", poor for "refactor the architecture based on these new axioms".

### 5. The "Anthropic Native" (Claude Code) _(added 2026-02-03)_

- **Model**: Terminal-based agent with file system access.
- **Workflow**: Agentic coding with tool use.
- **Context**: Full codebase access via tools.
- **Weakness**: No diagnostic integration. No workflow structure. No persistent project state.

### 6. The "Spec-Driven Agent" (Kiro) _(added 2026-02-03)_

- **Model**: Spec-first development with agent execution.
- **Workflow**: Write specs, agent implements.
- **Context**: Spec documents as context.
- **Weakness**: No diagnostic integration. Specs are static, not part of a living workflow.

## The "Exosuit" Differentiator

Exosuit is neither an "Assistant" (passive) nor an "Autonomous Agent" (black box). It is a **Collaborative Architecture**.

| Feature          | The Status Quo (Cursor/Devin/Copilot)                                     | The Exosuit Way                                                                                  |
| :--------------- | :------------------------------------------------------------------------ | :----------------------------------------------------------------------------------------------- |
| **Workflow**     | **Chat → Code**. The prompt is the only plan.                             | **SOAR Loop**: Status → Orient → Act → Review. Formalized in RFC 00224.                          |
| **Context**      | **Buffer Filling**. "Stuff as much text as possible into the window."     | **Structured Brain**. A persisted `docs/agent-context` database that survives sessions.          |
| **User Role**    | **Debugger**. Fixing AI mistakes after generation.                        | **Architect/Manager**. Approving plans and reviewing narratives _before_ code is written.        |
| **Truth**        | **The Codebase**. Documentation is usually ignored or generated post-hoc. | **The Artifacts**. The Manual, Axioms, and RFCs are the "Source of Truth"; code is a projection. |
| **Verification** | **Tests Passing**. "Does it run?"                                         | **Clean Pane = Clear Mind**. Problems pane integration + coherence checks. (RFC 00225)           |

## Key Findings for Blog Post

1.  **The "Smoothness" Trap**: Current tools aim to reduce friction (typing less). Exosuit intentionally _adds_ helpful friction (planning, reviewing) to ensure quality.
2.  **The "Context Drift"**: Other tools treat context as ephemeral RAM. Exosuit treats it as a persistent Hard Drive.
3.  **The "Black Box" Rejection**: Exosuit forces the AI to "show its work" via human-readable artifacts (Walkthroughs, Implementation Plans) _before_ it touches the code.
