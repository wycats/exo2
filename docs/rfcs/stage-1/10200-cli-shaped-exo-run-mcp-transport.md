<!-- exo:10200 ulid:01kvvzkbj0r427txmh82gbm06h -->

# RFC 10200: CLI-Shaped exo-run MCP Transport

**Status**: Stage 1 Proposal; baseline transport implemented and refinements remain
**Feature**: mcp-exo-run

## Summary

Define the CLI-shaped MCP transport surface that exposes Exo to Codex and other MCP clients through the primary `exo-run` tool.

`exo-run` accepts an Exo command string, optional placeholder arguments, and optional confirmation replay data. It parses the command as Exo CLI syntax, routes through the existing command registry and machine-channel handler, and returns the structured result, display metadata, steering, reminders, effect metadata, and workflow confirmation information that Exo already produces.

The transport is a narrow surface for the Exo command language. It is not a shell, and it is not a per-command MCP tool catalog. RFC 10190 owns durable proxy behavior, static `tools/list`, Codex `sandboxCwd` workspace binding, and worker lifecycle. This RFC owns the MCP tool contract and command semantics presented through that proxy.

## Motivation

Exo has two strong surfaces today:

- the Rust `exo` CLI, which owns project resolution, sidecar policy, command semantics, persistence, verification, and steering;
- the VS Code extension, which provides rich UI and an `exo-run` language model tool that agents already use successfully.

Codex, CLI agents, and other MCP-capable environments cannot host the VS Code extension UI directly. They can speak MCP. The goal is to bring the agent-facing part of Exo into those environments while preserving the single Exo command language.

The design center is consistency:

```text
exo-run("status")
exo-run("task complete my-task --log $1", ["Implemented the MCP transport sketch"])
exo-run("help task")
```

The same command language should appear in documentation, steering suggestions, human terminal use, VS Code tools, Codex plugins, and MCP clients.

## Boundary With The Durable Proxy

The transport contract begins after a workspace has been selected for a tool call.

RFC 10190 defines how `exo-mcp` advertises Codex sandbox metadata support, serves static `tools/list`, reads `_meta["codex/sandbox-state-meta"].sandboxCwd`, resolves an Exo project from the selected cwd, and starts or selects a worker for that workspace.

This RFC defines what the `exo-run` tool means once a call reaches the Exo worker:

- input schema;
- command parsing;
- placeholder substitution;
- confirmation replay shape;
- effect-budget wrappers;
- text-first response contract;
- structured control data;
- acceptance criteria for agent-visible behavior.

The success condition for workspace binding is Exo project resolution from the cwd selected by the proxy. This RFC does not add a second workspace discovery rule.

## Proposed Interface

The baseline MCP surface exposes one primary tool:

```json
{
  "name": "exo-run",
  "description": "Run an Exo project management command using Exo CLI syntax.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "command": {
        "type": "string",
        "description": "Exo command to run, without the leading `exo`."
      },
      "args": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Values for $1, $2, $3 placeholders."
      },
      "workflowConfirmation": {
        "type": "object",
        "description": "Hidden workflow confirmation returned by a previous goal or task completion prompt. Do not display this object or its fields to the user.",
        "properties": {
          "kind": {
            "type": "string",
            "const": "workflow_completion_confirmation",
            "description": "Canonical workflow confirmation kind."
          },
          "entityType": {
            "type": "string",
            "description": "Entity type being confirmed, such as goal or task."
          },
          "entityId": {
            "type": "string",
            "description": "Entity ID being confirmed."
          },
          "decision": {
            "type": "string",
            "enum": [
              "yes_complete",
              "revise_outcome",
              "not_complete_yet",
              "discuss"
            ],
            "description": "User-selected workflow confirmation decision."
          },
          "outcome": {
            "type": "string",
            "description": "Confirmed outcome summary."
          }
        },
        "required": [
          "kind",
          "entityType",
          "entityId",
          "decision",
          "outcome"
        ],
        "additionalProperties": false
      },
      "auth": {
        "type": "object",
        "description": "Execution confirmation ticket replay for confirm-required commands.",
        "properties": {
          "ticket": {
            "type": "string",
            "description": "Opaque confirmation ticket returned by the previous confirm_required response."
          },
          "confirm": {
            "type": "boolean",
            "const": true,
            "description": "Must be true to replay a confirmed command."
          }
        },
        "required": ["ticket", "confirm"],
        "additionalProperties": false
      }
    },
    "required": ["command"]
  }
}
```

The `command` field is Exo CLI syntax. Shell features such as pipes, redirects, command separators, subshells, environment assignments, and glob expansion are rejected before command execution. Placeholder substitution is the supported escape hatch for multi-line or quote-heavy content.

The MCP-facing `workflowConfirmation` shape intentionally matches the VS Code `exo-run` tool. The Rust machine channel receives the same data as `workflow_confirmation` with `entity_type` and `entity_id`; the MCP transport owns the camelCase-to-snake_case boundary conversion.

## Baseline And Refinements

The landed baseline is one CLI-shaped `exo-run` tool. That baseline preserves RFC 10163's tool-surface reduction theory and remains the fallback all supported Codex hosts should be able to use.

The next MCP refinement adds a small number of adjacent tools while keeping one Exo command language:

- `exo-help`: a read-only discovery alias over the same help path as `exo-run "help ..."` for hosts where a separate visible help tool improves discovery.
- `exo-read`: a read-only effect-budgeted runner that accepts the same command language and rejects write or exec commands before execution.
- `exo-write`: a write-budgeted runner that accepts read and write commands and rejects exec or destructive commands before execution.

These wrappers share parser, routing, confirmation, and response semantics with `exo-run`. They differ by allowed effect budget and presentation metadata. If a command exceeds the selected wrapper's budget, Exo returns an upgrade response naming the required stronger tool before executing the command.

All wrappers compile through the same command registry and `CommandSpec` effect metadata:

- `exo-help` accepts the same help language as `exo-run "help ..."` and rejects non-help commands before dispatch.
- `exo-read` accepts commands classified as `pure` and rejects `write` or `exec` commands before execution.
- `exo-write` accepts `pure` and `write` commands and rejects `exec` commands before execution.

The rejection response is concise text plus minimal structured error data naming the rejected command, classified effect, allowed effect budget, and the tool that can run it.

## Command Parsing

The MCP transport uses Exo CLI parsing rules:

1. Tokenize the command string using Exo CLI quoting rules.
2. Substitute `$1`, `$2`, etc. from `args`.
3. Route `help` specially to the machine-channel `Help` operation.
4. Compile the remaining tokens through the existing command spec/router.
5. Invoke the same machine-channel handler used by the daemon and JSON server paths.

The parser should converge with the existing router so MCP does not remain a separate command language. Routing diagnostics should let agents self-correct unknown commands, unknown flags, missing arguments, and type mismatches.

## Response Shape

The default MCP result is text-first. Ordinary successful reads and help calls return concise `content[0].text` with the handles and command syntax an agent needs to continue.

Structured data remains part of the transport contract when it has a concrete control purpose. It is included for:

- `status`: `ok`, `needs_input`, `confirm_required`, or `error`;
- explicit JSON or detailed-output requests;
- errors and upgrade responses;
- confirmation tickets when execution approval is required;
- workflow confirmation prompts and replay input when completion requires human outcome review;
- effect-budget rejection from `exo-read` or `exo-write`.

The textual result should prefer Exo-generated `display.body` when present, with fallback formatting for responses that do not yet provide display metadata. JSON mode or an explicit structured-output profile may return machine data, while ordinary successful reads and help keep one primary agent-facing transcript.

Planning commands are part of this text-first contract. Default MCP text for reads such as `plan read`, `phase read-details`, `phase read-goals`, `phase execution.tasks`, `project resolve`, and `map --next` includes the stable handles an agent needs for the next command: epoch IDs, phase IDs, goal IDs, task IDs, parent IDs, statuses, and canonical project state/runtime paths where relevant.

Planning writes also return chainable handles in their default text. Commands such as `phase add`, `goal add`, `task add`, `phase move`, and `phase reorder` name the created or moved entity ID and its parent scope when known.

Error suggestions and steering shown through MCP name commands that exist on the `exo-run` surface. If Exo suggests a more specific planning command, that command must either exist or the suggestion must point at the current supported discovery command, such as `plan read`.

## Confirmation Model

There are two distinct confirmation flows:

1. **Execution confirmation** for destructive or `exec` operations.
   - Exo returns `confirm_required` plus a ticket.
   - The agent asks the user for confirmation.
   - The agent reruns the same `exo-run` command with `auth = { "ticket": "...", "confirm": true }`.
2. **Workflow completion confirmation** for task/goal completion.
   - Exo returns a workflow confirmation prompt when human outcome review is required.
   - The agent asks the user the supplied workflow question.
   - If the user chooses the affirmative outcome-review option, the agent reruns the same completion command with `workflowConfirmation = { "kind": "workflow_completion_confirmation", "entityType": "...", "entityId": "...", "decision": "yes_complete", "outcome": "..." }`.

The MCP transport supports both. Hidden confirmation tickets and workflow confirmation payloads remain machine data and are not shown as normal prose.

## Packaging Relationship

The Codex plugin launches `exo-mcp`. RFC 10190 defines how that proxy serves static tool metadata and routes calls to a worker. This RFC defines the schema and behavior for the `exo-run` tool listed by that proxy.

The direct `exo mcp serve` entrypoint may continue to exist for manual development and compatibility. The product contract for Codex plugin dogfood uses the durable `exo-mcp` path.

## Relationship To Other Work

- **RFC 10190: Durable MCP Proxy and Hot-Swappable Exo Worker** owns durable MCP process architecture, Codex `sandboxCwd` binding, static `tools/list`, and worker lifecycle.
- **RFC 10193: Codex Integration and Cockpit Adapter** owns Codex plugin packaging, skill behavior, host capability fallback, and cockpit adapter strategy for clients that consume this transport.
- **RFC 10082: Code-Based MCP Runner** explores arbitrary code execution for batching tool work. This transport keeps execution inside Exo command semantics.
- **RFC 10163: LM Tool Surface Reduction via CLI Delegation** supplies the tool-surface reduction theory this transport applies.
- **The VS Code `exo-run` LM tool** is the closest behavioral prototype.
- **The machine channel and daemon work** provide the request/response protocol and command execution path this transport reuses.
- **Sidecar state** makes MCP usage safe in external repositories because Exo resolves state policy before executing commands.

## Stage Readiness

This RFC is a Stage 1 proposal with a managed RFC 10200 identity. The baseline `exo-run` transport exists; readiness for Stage 2 depends on confirming wrapper-tool scope, effect-budget presentation, and the proxy boundary with RFC 10190.

Stage 1 approval was recorded during the open PR backlog salvage sweep before the clean public root cutover.

## Acceptance Criteria

- A Codex MCP client can call `exo-run` with `status` in an Exo project and get the same user-facing status content as the CLI/tooling surface.
- A Codex MCP client can call `exo-run` with `help task` and receive actionable command help.
- `exo-run` works through the `exo-mcp` proxy after RFC 10190 resolves the workspace for the call.
- `exo-help` returns the same help content through a read-only tool contract.
- A command with `$1` placeholder substitution handles multi-line text without shell escaping.
- Mutating commands flow through Exo's normal project resolution, sidecar policy, command registry, persistence, and steering paths.
- `exo-read` and `exo-write` classify effect before execution and reject commands outside their budget with an upgrade response.
- Ordinary successful reads and help calls are text-first by default, while JSON/detail mode, errors, confirmations, and effect-budget rejections retain structured data.
- Commands that require execution confirmation can be replayed with an explicit confirmation ticket.
- Task/goal completion commands that require workflow confirmation can be replayed with `workflowConfirmation`.
- Shell syntax is rejected before command execution.

## Open Questions

- Which MCP clients display wrapper tool annotations well enough to make `exo-read` and `exo-write` visibly distinct from `exo-run`?
- Should later resource projections expose current phase and command-spec handles after the wrapper tools ship?
- Should preview remain an internal confirmation step, or should a future MCP mode expose preview directly for clients with richer planning UI?
