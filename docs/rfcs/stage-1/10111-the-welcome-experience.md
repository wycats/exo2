<!-- exo:10111 ulid:01kmzxey1hykx9kyefwr6nyv38 -->


# RFC 10111: The Welcome Experience

- **Superseded by**: RFC 0025


## Summary

This RFC defines the "First Run" experience for new Exosuit projects. It proposes a "Welcome Wizard" (interactive CLI or Webview) that bootstraps the `AGENTS.md`, the scoped axioms files (`docs/agent-context/axioms.workflow.toml`, `docs/agent-context/axioms.system.toml`, `docs/design/axioms.design.toml`), and initial `plan.toml` based on a user interview.

## Motivation

- **Blank Slate Paralysis**: New projects start empty. Users don't know which files to create first.
- **Axiom Alignment**: We want to capture the user's "Philosophy" (e.g., "Strict Types", "Fast Iteration") early and encode it into the scoped axioms files (`docs/agent-context/axioms.workflow.toml`, `docs/agent-context/axioms.system.toml`, `docs/design/axioms.design.toml`).
- **Persona Setting**: The agent needs to know "Who am I?" (The Strict Pair Programmer? The Hacker?).

## Design

### 1. The Wizard Flow

1.  **Trigger**: `exo init` or "Exosuit: Initialize Project".
2.  **Question 1: Mission**: "What are we building?" -> Populates `AGENTS.md`.
3.  **Question 2: Mode**: "How should I behave? (Strict/Loose)" -> Populates `modes.toml` / `docs/agent-context/axioms.workflow.toml`.
4.  **Question 3: First Step**: "What is the first milestone?" -> Creates Epoch 1 in `plan.toml`.

### 2. Artifact Generation

The wizard generates the standard `docs/agent-context` structure, ensuring the project is "Exosuit Compliant" from second zero.
