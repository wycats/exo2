<!-- exo:10082 ulid:01kmzxefe0bwmddhb7tk8ghdr7 -->


# RFC 10082: Code-Based MCP Runner

## Summary

Investigate replacing the current verbose MCP tool style with a "Code Execution" approach, where the agent writes and executes scripts (e.g., Python/TS) to interact with the system.

## Motivation

The current tool use pattern (one tool call per turn, verbose JSON) creates "context noise" and latency. A "Code Interpreter" style approach allows the agent to chain multiple operations, perform logic, and process data in a single turn, which is far more efficient.

## Detailed Design

_Concept:_

- **The "Run Code" Tool**: A single tool that accepts a script (Python/TS).
- **Sandboxed Environment**: The script runs in a controlled environment with access to specific APIs (MCP tools).
- **Batching**: The agent can `read_file`, `process_text`, and `write_file` in one script.
- **Efficiency**: Reduces round-trips and token overhead significantly.

## Unresolved Questions

- Security: How to sandbox this effectively?
- DX: How to debug these scripts?
- Compatibility: Can we wrap existing MCP tools as libraries in this environment?
