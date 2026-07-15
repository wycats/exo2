<!-- exo:10093 ulid:01kmzxeff9edw52yp8mbbh2f07 -->


# RFC 10093: Organic Context Injection (State-Aware Agents)

- **Superseded by**: RFC 0172


## Summary

This RFC proposes leveraging the existing **Persistent Tree** (maintained by the VS Code Extension) to inject "Organic Context" into the Agent's prompt. Instead of just providing file contents, we provide **Derived State** (e.g., "File modified after last build", "Test passed 5m ago"). This prevents "Existential Crises" where the agent is confused by stale errors or out-of-sync artifacts.

## Motivation

- **The "Existential Crisis"**: Agents often spiral when `read_file` shows correct code but `run_build` shows an error. They lack the temporal context to know the build is stale.
- **Token Waste**: Agents spend massive compute "thinking" about contradictions that are trivially solvable with timestamp comparisons.
- **Unused Infrastructure**: We already maintain a real-time graph of the workspace in the Extension (for the Sidebar). We should expose this to the Agent.

## Detailed Design

### 1. The State Graph

The Extension's `ExosuitTreeProvider` currently tracks the structure of `plan.toml`, `axioms.*.toml`, etc. We will extend this to track **Operational Metadata** for all workspace files:

- `lastModified`: Timestamp of file change.
- `lastBuild`: Timestamp of the last successful/failed build.
- `lastTest`: Timestamp and status of the last test run covering this file.

### 2. The Injection Mechanism

When the Agent is active (e.g., via `@exosuit` participant), we inject a "State Summary" into the system prompt or context window.

**Example Context Block:**

```text
[CONTEXT STATE]
- src/lib/inspector/overlay.ts
  - Status: Modified (10:00:05)
  - Last Build: Failed (10:00:00) -> WARNING: File is newer than build.
  - Last Test: Passed (09:55:00) -> WARNING: Tests are stale.
```

### 3. The "Organic" Experience

The Agent doesn't need to query a tool to find this. It is simply _aware_ of it, just like a human developer looking at their terminal history.

- **Scenario**: Agent sees a build error.
- **Old Behavior**: "Code looks right. Error says wrong. I am confused. Let me re-read the file 5 times."
- **New Behavior**: "Ah, the file is newer than the build. I just need to run `exo run build`."

## Edge Cases & Interactions

### 1. The Monorepo Dependency Graph

In a workspace with multiple packages (e.g., `core` and `extension`), a change in `core` invalidates the build for `extension`.

- **Challenge**: Simple timestamp comparison isn't enough.
- **Solution**: The State Graph must be dependency-aware. If `core/src/lib.rs` is modified, the "Last Build" status for `extension` transitions to `Stale (Dependency Changed)`.

### 2. The "Ghost" Build (External Events)

Users might run builds in a separate terminal or via `git pull`.

- **Challenge**: The Extension might not know a build occurred.
- **Solution**:
  - **File Watchers**: Watch `target/` or `dist/` directories to detect output changes.
  - **Epistemic Humility**: Report state as "Last _Observed_ Build". If the agent sees a discrepancy, it should trust its tools (run a fresh build) over the cached state.

### 3. Context Budgeting

Injecting state for every file will exhaust the context window.

- **Heuristic**: Only inject state for:
  - **Active Files**: Files currently open in the editor.
  - **Referenced Files**: Files explicitly mentioned in the chat or "read" by the agent.
  - **Problem Files**: Files with active diagnostics (errors/warnings).

### 4. The Race Condition (Tooling Latency)

The Agent edits a file and immediately runs a build.

- **Risk**: The Extension's file watcher might not have updated the State Graph yet.
- **Mitigation**: The `exo` CLI acts as the synchronization point. When `exo run` executes, it forces a "State Flush" or waits for file system settlement before proceeding.

## Drawbacks

- **Complexity**: Requires the Extension to track build/test events (likely via Task End events or file watchers on `target/`).
- **Context Window**: Adds tokens to the prompt. (Mitigation: Only inject state for "Active" or "Open" files).

## Alternatives

- **Smart Tools**: Have `exo run` return this metadata. (Reactive, not Proactive).
- **Explicit Tool**: Have `exo verify <file>` tool. (Requires Agent to know when to ask).
