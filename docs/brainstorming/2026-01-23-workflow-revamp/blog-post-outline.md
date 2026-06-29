# Blog Post Outline: The Exosuit Hypothesis

_Drafted: Jan 23, 2026_

## Title Ideas

- "Against Smoothness: Why AI Coding Needs More Friction, Not Less"
- "The Exosuit: Turning the AI from a Junior Coder into a Chief of Staff"
- "Beyond Copilot: The Case for Phased, Rigorous AI Development"

## The Hook: "The Uncanny Valley of AI Code"

- We've all been there: You ask Cursor/Copilot for a feature. It writes beautiful, idiomatic code. It compiles.
- Two weeks later, you realize it implemented a pattern you explicitly banned three months ago.
- Why? Because **Chat is Stateless**. The AI lives in the "Eternal Now". It doesn't know your history, your constraints (Axioms), or your long-term plan (Epochs).

## The Diagnosis: The "Context-Buffer" Fallacy

- Current tools try to solve this by stuffing more tokens into the context window (1M context windows!).
- **The Problem**: More context ≠ More Coherence.
- **The Solution**: Structure. You don't need a larger RAM; you need a File System.

## The Exosuit Solution: The Workspace IS The Agent

Exosuit isn't a tool you _use_; it's a workspace you _inhabit_.

### 1. The Brain (Structured Context)

- Instead of ephemeral chat history, Exosuit maintains a persistent `docs/agent-context`.
- **The Inbox**: Where ideas land.
- **The Plan**: What we're doing now.
- **The Manual**: The compiled reality of the system.
- _Contrast_: Copilot "guesses" context. Exosuit "reads" the Manual.

### 2. The Loop (Phased Execution)

- **Status Quo**: Prompt → Diff.
- **Exosuit**:
  1. **Plan**: Draft a narrative `implementation-plan.md`. (Review phase).
  2. **Implement**: Code the changes. (Maker mode).
  3. **Verify**: Check against the plan and Axioms. (Coherence phase).
  4. **Transition**: Update the Manual. (Chief of Staff mode).
- _Benefit_: You catch architectural drift _before_ it becomes code.

### 3. The Rituals (Human-in-the-Loop)

- AI shouldn't just write code; it should help you _think_.
- **Fresh Eyes Reviews**: "Review this PR as if you were a skeptical Systems Architect."
- **The Council**: "Simulate a debate between a UI Expert and a Database Engineer about this feature."

## Conclusion: The "Cognitive Prosthetic"

- We don't need faster typing. We need better thinking.
- Exosuit aims to be a bicycle for the mind, not just a motor for the fingers.
