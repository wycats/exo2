<!-- exo:10063 ulid:01kmzxeff3na23s2t3td0qzdrz -->


# RFC 10063: Bug Reporting Workflow

- **Superseded by**: RFC 0046


## Summary

This RFC proposes a new `exo bugreport` command designed to streamline the creation of high-quality bug reports. It introduces a "Steering Command" pattern where the root command (`exo bugreport`) acts as a Just-In-Time (JIT) steering mechanism for the Agent/User, while a subcommand (`exo bugreport create`) performs the actual artifact generation.

## Motivation

Creating comprehensive bug reports is often a friction point. Users (and Agents) may forget to include critical context like severity, reproduction steps, or specific file contents. By formalizing this into the `exo` CLI, we ensure:

1.  **Consistency**: All bug reports follow a standard format.
2.  **Completeness**: The CLI enforces required fields (severity, description).
3.  **Agent Alignment**: The "Steering Command" provides the Agent with the exact protocol to follow, reducing hallucination or process drift.

## Design

### The "Steering Command" Pattern

We propose a general pattern for complex workflows:

1.  **Steering Command** (`exo <cmd>`): When run without arguments, it outputs a "Prompt" or "Instruction Set" optimized for an AI Agent (or a human following a protocol). It explains _how_ to use the tool and what information to gather.
2.  **Action Command** (`exo <cmd> <action>`): The functional tool that executes the task based on the gathered inputs.

### `exo bugreport` (Steering)

Running `exo bugreport` will output instructions similar to:

```markdown
# Bug Report Protocol

You are about to create a bug report. Please gather the following information:

1.  **General Problem**: A concise description of the issue.
2.  **Severity**:
    - `critical`: Blocking all work.
    - `high`: Blocking a specific feature.
    - `medium`: Inconvenient but workable.
    - `low`: Cosmetic or minor.
3.  **Files**: Which files are relevant? For each file, describe what is seen vs. expected.
4.  **Workarounds**: Any known workarounds?

Once gathered, run:
`exo bugreport create --description "..." --severity <level> --file <path>:"<observation>" ...`
```

### `exo bugreport create` (Action)

This command generates the actual report.

**Arguments:**

- `--description` (Required): The general problem.
- `--severity` (Required): `critical` | `high` | `medium` | `low`.
- `--file` (Repeated): `<path>:<observation>`. E.g., `--file src/main.rs:"Panics on line 50"`.
- `--workaround` (Optional): Description of workarounds.
- `--debug-info` (Optional): Any pre-determined debug info.

**Output:**
The command will generate a Markdown-formatted bug report.

**Destination (Scratch File):**
The report should be opened in a VS Code "Untitled" (scratch) editor to allow the user to review, copy, or save it without polluting the file system.

#### Technical Implementation of Scratch Files

To open a scratch file from the CLI:

1.  **Pipe to Code**: `exo bugreport create ... | code -`
    - _Pros_: Simple, standard.
    - _Cons_: Requires `code` in PATH.
2.  **Temp File**: Write to `/tmp/bugreport.md` and run `code /tmp/bugreport.md`.
    - _Pros_: Robust.
    - _Cons_: Persists on disk until cleaned.

**Recommendation**: Use the `code -` pattern if available, falling back to printing to stdout if not.

## Questions & Trade-offs

### Do we have an existing RFC for "Steering Commands"?

No. This RFC establishes the precedent. If successful, we should extract this into a "CLI Interaction Patterns" RFC.

### VS Code Scratch Files

- **Constraints**: The `code` CLI must be available.
- **Value**: High. Reduces friction and "file clutter".
- **Best Practice**: Use `code -` to pipe content directly into an untitled buffer.

## Future Work

- Integrate with GitHub Issues API (`exo bugreport submit`).
- Auto-capture context (logs, environment info).
