<!-- exo:10163 ulid:01kmzxbcyvcykcbx5f1k1tdn3z -->


# RFC 10163: LM Tool Surface Reduction via CLI Delegation

## Status

Stage 2: Draft Specification

## Problem

LM tools are not better than CLIs. They are worse. CLIs have decades of design iteration, and agents already know how to navigate them. The only reason the LM tool surface exists is to avoid raw terminal friction: quoting and escaping, noisy output, and installation requirements.

Today, Exosuit ships ~30 LM tools. Their schemas cost about 3,540 tokens every turn. That cost is paid whether the tools are used or not. It competes directly with code, conversation, and steering.

## Solution: CLI-Shaped Machine Channel

Introduce a single tool that accepts CLI-shaped commands but executes through the machine channel.

- **CLI-shaped syntax**: familiar and discoverable through `help`.
- **Machine channel execution**: structured JSON responses, no shell quirks.
- **Zero installation**: ships with the extension, no PATH setup.
- **Placeholder substitution**: `$1`, `$2`, `$3` for complex values.

This keeps the agent in a command-line mental model while removing the terminal pain points.

## Tool Design

The machine channel is exposed as a single tool: `exo-run`. It takes a command string and optional placeholder values.

```json
{
  "name": "exo-run",
  "description": "Run an exo command. Use `help` to discover commands, `help <command>` for details.\n\nExamples:\n  exo-run(\"status\")\n  exo-run(\"task complete my-task --log 'Done'\")\n  exo-run(\"rfc create --title 'My RFC' --body $1\", [\"Multi-line\\nbody text\"])\n  exo-run(\"help task\")",
  "inputSchema": {
    "type": "object",
    "properties": {
      "command": {
        "type": "string",
        "description": "The command to run. Use $1, $2, $3 for complex values."
      },
      "args": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Values for $1, $2, $3 placeholders (for multi-line or quoted content)."
      }
    },
    "required": ["command"]
  }
}
```

### Usage Examples

```
exo-run("help")
exo-run("help task")
exo-run("task complete my-task --log 'Done'")
exo-run("rfc create --title 'My RFC' --body $1", ["Multi-line\nbody text"])
```

### Discovery Pattern

1. `help` to list commands.
2. `help <command>` for details.
3. Execute the command.

This mirrors how agents already explore CLIs.

## Technical Specification

### Processing Pipeline

1. **Tokenize** the command string (handle quotes and escapes).
2. **Substitute** `$1`, `$2`, `$3` placeholders from the args array.
3. **Route**:
   - If first token is `help`: emit `Op::Help` with remaining tokens as path. The server determines whether the path refers to a namespace or operation.
   - Otherwise: pass tokens to `compile_argv()` which produces an `Invocation`, then emit `Op::Call`.
4. **Execute** via machine channel (`handle_request()`).
5. **Return** the response (success with result/steering, or error with diagnostics).

### Error Design

Errors follow the **errors-answer-the-guess** axiom: messages should address the likely misunderstanding, not just report syntax violations.

The machine channel already returns structured diagnostics with:

- `code`: Error category (e.g., `unknown_flag`, `missing_value`, `ambiguous_subcommand`)
- `message`: Human-readable explanation
- `suggestions`: Concrete fixes (e.g., "Did you mean 'task'?")
- `steering`: Points to relevant help

Example: If an agent tries `taks complete`, the error should say "Unknown command 'taks'. Did you mean 'task'?" — not "Invalid namespace".

## Implementation Plan

### Files to Create/Modify

| File                                            | Change                                                       |
| ----------------------------------------------- | ------------------------------------------------------------ |
| `packages/exosuit-vscode/src/lmtool/exo-run.ts` | **NEW** — Tool handler: tokenize, substitute, route, execute |
| `packages/exosuit-vscode/package.json`          | Register `exo-run` tool with schema                          |
| `tools/exo/src/command/cli_parser.rs`           | Already exists — tokenize + placeholder substitution         |

### Execution Sequence

1. **Register tool**: Add `exo-run` to package.json with the schema above.
2. **Implement handler**: Create TypeScript handler that:
   - Tokenizes command string (reuse logic from cli_parser.rs or port to TS)
   - Substitutes placeholders
   - Routes to machine channel (help vs call)
   - Returns formatted response
3. **Remove delegated tools**: Delete tools that are now covered by `exo-run`.
4. **Update SOAR anchors**: Ensure retained tools (`exo-status`, etc.) have descriptions that guide agents to `exo-run` for other operations.

## Retained Tools (SOAR Anchors)

Keep the tools that teach the agent the system exists and how to enter it:

- `exo-status`
- `exo-steering`
- `exo-context`
- `exo-phase`

These are behavioral entry points. Their descriptions are part of the agent's orientation loop.

### Meta-Tools (VS Code-Native)

Keep tools that help the agent understand its own environment:

- `exo-diagnostics` — compile/lint errors from the editor
- `exo-logs` — extension output channel
- `exo-ai-chat-history` — past conversation sessions

These are **meta-tools**: they help the agent orient itself, not do work on the project. They remain as separate LM tools (not routed through `exo-run`) for two reasons:

1. **Discoverability**: Meta-tools should be directly visible in the tool list so agents can find them without first knowing to run `help`. An agent that needs to recover lost context shouldn't have to discover `exo-run` first.
2. **VS Code dependency**: These tools require VS Code APIs (diagnostics, output channels, workspace storage) that the CLI binary doesn't have access to.

The boundary is principled: `exo-run` is for **project work** (tasks, RFCs, phases, goals). Meta-tools are for **environmental awareness** (errors, logs, history). If a new tool is proposed, ask: "Is this about the project or about the agent's environment?" Project → `exo-run`. Environment → separate meta-tool.

## Rollout Plan

1. **Deploy** `exo-run` alongside existing tools.
2. **Remove** delegated tools immediately — the CLI covers all functionality.
3. **Restore** individual tools only if real usage reveals a compelling reason (e.g., a tool that benefits from richer schema description).

The bias is toward removal. Token cost is paid every turn; restoration cost is paid once.

## Alternatives Considered

### Just use terminal

Raw terminal use reintroduces quoting and escaping issues, dumps noisy output, and forces installation or PATH setup. It is operationally heavier and less reliable for agents.

### Wrap the CLI binary

Wrapping the CLI still routes through a shell and keeps shell parsing quirks. It also risks sync issues between wrapper semantics and real CLI behavior.

### Dynamic tool registration

Dynamic tool sets add state complexity and make tool visibility inconsistent. It reduces schema size without removing the core duplication between LM tools and the CLI.
