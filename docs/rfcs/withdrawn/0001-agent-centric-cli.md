<!-- exo:1 ulid:01kg5m2yartenanjdaw26z4cnc -->

# RFC 1: Agent-Centric CLI Design (The Conversationalist)

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC 0001: Agent-Centric CLI Design (The Conversationalist)

## Meta

- **Status**: Stage 0 (Strawman)
- **Created**: 2025-12-05
- **Tags**: tooling, agent-experience, axioms

## The Insight: Cognitive Alignment

Most CLIs are designed for **Humans** (concise, visual, interactive) or **Scripts** (silent, exit codes, pipeable).
We are building for a third user: **The Agent**.

### The Agent's Cognitive Style

To design for an Agent, we must understand its "Psychology":

1.  **Statelessness**: The Agent lives in the "Eternal Now". It cannot easily "remember" a file listing from 5 turns ago.
    - _Design Rule_: Error messages must be **Self-Contained**. Include the relevant context (e.g., "File not found. Current files in `src/`: [a.ts, b.ts]").
2.  **Suggestibility**: Agents are highly compliant. If a tool suggests a fix, the Agent will likely try it immediately.
    - _Design Rule_: Use **"Did You Mean?"** aggressively. This turns a "Dead End" into a "Multiple Choice Question".
3.  **Literalness**: Agents struggle with ambiguity. "Invalid input" leads to guessing. "Expected 'json' or 'yaml'" leads to correction.
    - _Design Rule_: Be **Enumerative**. List valid options whenever possible.
4.  **Token Economics**: Verbosity is expensive, but ambiguity is more expensive (retries).
    - _Design Rule_: Be **High-Signal**. Omit fluff ("Welcome to Exo CLI!"), but include dense, actionable data in errors.

## The Axiom: "Agent-First Tooling"

**Principle**: A tool's output is not just a log; it is a prompt for the next turn.

**Why**: When a tool fails, it breaks the agent's chain of thought. A good error message repairs the chain by providing the missing context or the correct syntax immediately.

**Implication**:

- **Never say "File not found"**. Say "File 'foo.ts' not found. Did you mean 'src/foo.ts'?"
- **Never say "Invalid JSON"**. Say "Invalid JSON at line 10: missing comma."
- **Never say "Ambiguous match"**. Say "Ambiguous match. Your query matched: [A, B, C]. Please refine."

## The Standard: `Exo-CLI` Output Protocol

All `exo` commands must adhere to this protocol:

### 1. The "Did You Mean?" Protocol (DYM)

If an entity (file, key, symbol) is not found, the tool **MUST** perform a fuzzy search against valid entities and suggest the top 3 matches.

### 2. The "State of the World" Protocol

If an operation fails due to state conflict (e.g., "git lock"), the tool **MUST** describe the current state that caused the conflict.

- _Bad_: `Error: Lock file exists.`
- _Good_: `Error: Lock file exists. Process 1234 (exo phase start) has been running for 5 minutes.`

### 3. The "Syntax Repair" Protocol

If the agent passes invalid arguments, the tool **MUST** show the correct usage example for that specific command, not the generic help text.

### 4. The "Machine-Readable" Flag

All commands must support `--format json` for cases where the agent needs to ingest the output programmatically without parsing text.

## Example: The "Stale Read" Scenario

**Agent**: `exo code rename --symbol "User" --to "Account"`
**Tool**:

```text
Error: Symbol 'User' not found in src/models.ts.

Context Analysis:
- I found 'UserProfile' on line 10.
- I found 'UserSession' on line 45.
- I found 'Account' (already renamed?) on line 88.

Action:
- If you meant 'UserProfile', retry with --symbol "UserProfile".
- If you think the file is stale, run 'exo context refresh'.
```

## Integration with Axioms

This RFC proposes promoting this concept to **Axiom 11: Agent-Centric Tooling**.

> **Principle**: Tools must treat the Agent as a first-class user, providing "Actionable Intelligence" on failure to minimize context thrashing.

