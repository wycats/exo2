<!-- exo:147 ulid:01kmzxeffpd6fm6mbfgj0qbs6b -->

# RFC 147: LM Tool: Read Output Channel Logs


# RFC 0147: LM Tool: Read Output Channel Logs

## Problem

When errors occur in the Exosuit extension, the agent advises users to "check the Exosuit output channel for details." This is reasonable advice, but it creates friction:

1. The user must manually open the Output panel
2. The user must select the correct channel
3. The user must copy/paste relevant logs back to the agent
4. The agent cannot proactively diagnose issues

## Solution

Add an LM tool `exo-logs` that returns recent entries from the Exosuit output channel, enabling the agent to self-diagnose issues.

## Design

### Tool Definition

```json
{
  "name": "exo-logs",
  "displayName": "Extension Logs",
  "toolReferenceName": "logs",
  "canBeReferencedInPrompt": true,
  "icon": "$(output)",
  "tags": ["exosuit", "debug"],
  "userDescription": "Read recent Exosuit extension logs for debugging.",
  "modelDescription": "Returns recent log entries from the Exosuit output channel. Use when diagnosing errors, understanding extension behavior, or when a previous operation failed unexpectedly.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "lines": {
        "type": "number",
        "description": "Number of recent log lines to return (default: 50, max: 500)"
      },
      "level": {
        "type": "string",
        "enum": ["error", "warn", "info", "debug"],
        "description": "Minimum log level to include (default: all levels)"
      },
      "component": {
        "type": "string",
        "description": "Filter by component (e.g., 'lmtool', 'extension', 'webview')"
      }
    },
    "required": []
  }
}
```

### Implementation

1. **Log Buffer**: Maintain a circular buffer of recent log entries in `logging.ts`
2. **Structured Entries**: Store entries as objects with `{timestamp, level, component, message}`
3. **LM Tool Handler**: Query the buffer with optional filters

### Privacy Considerations

- Logs may contain file paths, operation names, and error details
- No credentials or secrets should be logged (existing policy)
- Tool is read-only and only accesses extension's own logs

## Alternatives Considered

1. **File-based logging**: Write logs to a file and read it back. Rejected: adds I/O complexity, file management.
2. **Expose via MCP**: Make it a Rust-side command. Rejected: logs are TypeScript-side, would require IPC overhead.

## Implementation Plan

1. Add `LogBuffer` class to `logging.ts` with circular buffer
2. Update `delegatingSink` to also push to buffer
3. Add `exo-logs` tool definition to `package.json`
4. Implement tool handler in extension activation
5. Add tests

