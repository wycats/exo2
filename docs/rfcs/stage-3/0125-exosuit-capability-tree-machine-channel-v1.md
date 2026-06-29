<!-- exo:125 ulid:01kg5kp2h81gcf333kqqw29frv -->

# RFC 0125: Exosuit Capability Tree + Machine Channel v1

## Summary

Define a stable, language-agnostic machine-channel protocol for Exosuit semantics.

The protocol lets:

- the Rust CLI (`exo`) expose structured capabilities;
- the VS Code extension discover and invoke those capabilities;
- LM tools call the same semantic operations;
- cross-language contract tests validate paging, failure modes, steering, and confirmation behavior.

## Motivation

Exosuit needs one semantic interface projected through multiple transports:

- project daemon socket;
- stdio subprocess fallback;
- future WASM/library transport.

Without an explicit protocol boundary, the VS Code extension, CLI, and LM tools drift.

## Detailed Design

### Transport

The protocol is transport-agnostic. Two transports are supported.

#### Socket transport (primary)

Unix domain socket at the project daemon path from RFC 10184:

```text
{state_root}/runtime/daemon.sock
```

Properties:

- persistent daemon process (RFC 0097);
- multiple clients can connect;
- primary path for VS Code and LM tools;
- supports future bidirectional push.

```text
Client ──connect──▶ {state_root}/runtime/daemon.sock
       ◀──────────▶ NDJSON request/response
```

For a normal default-state repository, `{state_root}` is usually `<primary-workspace>/.exo`. For shadow state, `{state_root}` is `$HOME/.exo/projects/<project-id>`.

#### Stdio transport (fallback)

Subprocess channel for testing and fallback:

- request: a single JSON object on stdin;
- response: a single JSON object on stdout;
- CLI command: `exo json channel`.

```text
Caller ──spawn──▶ exo json channel
       ◀────────▶ stdin/stdout JSON
```

Both transports use the same envelope format.

### Envelope

Requests and responses use a versioned envelope.

#### Request

Fields:

- `protocol_version: number` — must match the CLI-supported version;
- `id: string` — opaque request identifier, echoed back;
- `op: { kind: 'help'|'list'|'call', params: object }` — the operation;
- `auth?: { ticket: string, confirm?: boolean }` — optional authorization for effectful calls;
- `agent_id?: string` — optional caller identity for steering and perception filtering;
- `workflow_confirmation?: object` — optional semantic confirmation payload for workflow-level decisions.

#### Response

Fields:

- `protocol_version: number`;
- `id: string` — copied from the request;
- `status: 'ok'|'needs_input'|'confirm_required'|'error'`;
- `result?: any` — present for `ok`;
- `error?: { code: string, message: string, details?: any }` — present for `error`;
- `ticket?: string` — present for `confirm_required`;
- `steering?: { next_call: { kind: 'help'|'list'|'call', params: object } }` — guidance for recovery.

### Capability discovery: help ladder

Capability discovery is explicit and navigable:

- `help(root)` returns a compact set of top-level namespaces;
- `help(namespace)` reveals operations and sub-namespaces;
- `help(operation)` provides operation details.

**Implementation requirement**: The capability tree is derived from `CommandSpec`, not maintained separately. The `CommandSpec` schema is the single source of truth for:

- namespace structure;
- operation definitions;
- effect annotations;
- argument schemas;
- LM tool metadata.

This ensures the help ladder cannot drift from the actual CLI implementation.

### The `list` operation

The `list` operation enumerates items within a namespace with paging support.

Request example:

```json
{
  "protocol_version": 1,
  "id": "req-123",
  "op": {
    "kind": "list",
    "params": {
      "path": ["phase", "execution"],
      "resource": "tasks",
      "page_size": 10,
      "cursor": null
    }
  }
}
```

Response example:

```json
{
  "protocol_version": 1,
  "id": "req-123",
  "status": "ok",
  "result": {
    "items": [
      {
        "id": "setup-deps",
        "label": "Setup dependencies",
        "status": "completed"
      },
      { "id": "run-tests", "label": "Run test suite", "status": "pending" }
    ],
    "next_cursor": "cursor-abc",
    "has_more": true
  }
}
```

### Addressing model

Operations reference an address:

- `root`;
- `namespace` with a `path: string[]`;
- `operation` with a `path: string[]`.

### Effects

Each operation is annotated with a coarse effect:

- `pure` — read-only / deterministic;
- `write` — writes to Exosuit-managed artifacts;
- `exec` — executes a workflow.

**Implementation requirement**: The `effect` annotation is declared in `CommandSpec` on each leaf operation.

The VS Code LM tool uses this to decide when user confirmation is required.

### Authorization confirmation vs workflow confirmation

The machine channel has two distinct confirmation concepts.

#### Authorization confirmation

Write/exec operations require effect authorization. This uses the `auth.ticket` handshake and answers: "May this tool perform a write/exec operation?"

#### Workflow confirmation

Some successful workflows need semantic human acceptance before state changes should be recorded. This uses `workflow_confirmation` and answers: "Is this outcome accepted as complete?"

Example: `goal complete` may return an error with `details.workflow_confirmation`. The LM tool presents that payload as a human-facing question. If the human chooses "Yes, complete it," the caller retries the same operation with the provided `workflow_confirmation` input. The command then records the internal completion evidence and completes the goal.

Workflow confirmation is product-facing; it must speak in outcomes and artifacts, not internal claim IDs.

### Steering

When an error occurs, the response includes a `steering` block with a `next_call` field.

- **Single recommendation**: When there is one clear recovery action, `next_call` contains that operation.
- **Exploration fallback**: When multiple paths are possible, `next_call` points to `help(root)` or the nearest valid ancestor.
- **Input correction**: When input is invalid, `next_call` points to `help(operation)`.

Tool grouping strategy is specified outside this protocol RFC. The stable VS Code extension currently contributes individual LM tools and does not rely on proposed `languageModelToolSets`.

## Failure Modes

- Version mismatch returns `status='error'`, `error.code='version_mismatch'`, and steering to `help(root)`.
- Unknown address returns `error.code='unknown_address'` with steering to `help(root)`.
- Invalid input returns `error.code='invalid_input'` with steering to the relevant `help(operation)`.
- Missing effect authorization returns `status='confirm_required'` with a ticket.
- Missing workflow confirmation returns `status='error'` with semantic confirmation details when the command supports it.

## Security Considerations

- The channel is structured; it does not accept free-form shell command strings.
- Write/exec effects require user authorization.
- Workflow confirmation does not authorize writes by itself; the write effect still uses the normal authorization path.

## Testing

Contract tests validate:

- `help(root)` returns expected namespaces;
- operation discovery is generated from `CommandSpec`;
- machine-channel fixtures remain stable;
- version mismatch returns error plus steering;
- command dispatch parity matches direct command behavior;
- workflow confirmation payloads round-trip through Rust and TypeScript formatting.

## Future Work

- Add richer typed schemas for call inputs and outputs.
- Support additional transports without changing semantics.
- Add bidirectional push for reactive UI updates.

## Related RFCs

- RFC 0097: Unified Server Architecture — daemon lifecycle and socket paths.
- RFC 10184: Project / Workspace / Worktree unbundling — project state root and daemon boundary.
- RFC 10180: Storage Disposition — canonical state, documents, and configuration.
