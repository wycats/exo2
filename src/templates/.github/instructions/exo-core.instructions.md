---
applyTo: "**/*"
---

# Exosuit Core Workflow

This project uses **Exosuit** for structured human-AI collaboration.

## Tool Discovery

All exo operations go through `exo-run`:

```
exo-run("help")              # List all namespaces
exo-run("help <namespace>")  # Show operations in a namespace
exo-run("help <ns>.<op>")    # Detailed help for an operation
```

When unsure what command to use, explore with `help`. The CLI is self-documenting.

## The SOAR Loop

Every productive session follows **Status → Orient → Act → Review**:

| Phase      | Question                          | Tools                                       |
| ---------- | --------------------------------- | ------------------------------------------- |
| **Status** | Where am I? What's the delta?     | `exo-status`, `exo-phase`                   |
| **Orient** | What are my options? What's next? | `exo-run("help")`, `exo-run("steering")`    |
| **Act**    | Execute the chosen action         | `exo-run("task ...")`, `exo-run("rfc ...")` |
| **Review** | Did it work? What did we learn?   | Verify, `exo-run("verify")`, tests          |

**Start of session**: Status → Orient (where am I? what's next?)
**During work**: Act → Review → Status (tight loop)
**At decision points**: Orient (what are my options?)

## Critical Rule: Managed Directories are Databases

Files in managed Exosuit state have schemas, IDs, and relationships. RFCs are managed documents; operational state is managed through the `exo` CLI and may be projected as generated SQL under `docs/agent-context/` only for repo policy.

**Always use CLI commands** to create/modify files in managed directories:

- `exo-run("rfc create --title '...'")` for RFCs
- `exo-run("idea add --title '...'")` for ideas
- `exo-run("task add --label '...'")` for tasks
- `exo-run("axiom add ...")` for axioms

**Never** use `create_file` or direct editing for managed state files—it bypasses validation and ID generation. Durable human notes belong under `docs/design/`, `docs/research/`, or `docs/specs/`.

## Context Management

Treat the main chat context as a scarce resource:

1. **Delegate modular work** to subagents when it can be fully defined by a standalone prompt
2. **Keep entangled work** in main chat when it relies heavily on conversation history
3. **Pass by reference** - point subagents to file paths, don't paste content

When delegating, the subagent has the same file access tools you do.
