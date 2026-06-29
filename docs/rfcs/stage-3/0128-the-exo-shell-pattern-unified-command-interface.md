<!-- exo:128 ulid:01kg5kp2hcppkkph1ygjr6k5m9 -->

# RFC 128: The Exo-Shell Pattern (Unified Command Interface)


# RFC 0128: The Exo-Shell Pattern (Unified Command Interface)

## Summary

This RFC proposes using the `exo` CLI as the **exclusive** interface for project tasks (via `exo run <task>`), accessed via the standard `run_in_terminal` tool. This eliminates the need for a separate "Task Runner" tool, reducing cognitive noise and leveraging the LLM's natural affinity for shell interaction while enforcing strict discipline via the Rust binary.

## Motivation

- **Cognitive Noise**: Providing both `run_in_terminal` (generic shell) and `exosuit_run_task` (specific tool) confuses the agent. "Which one do I use?"
- **LLM Bias**: Models are heavily trained on shell interaction. They often hallucinate shell commands even when given specific tools.
- **Mashing**: We want to stop the agent from "mashing" buttons (trying random `npm` scripts) and force it into a disciplined workflow without fighting its nature.
- **Unified Interface**: The Human (via Sidebar) and the Agent (via Terminal) should share the same "Menu" of capabilities.

## Detailed Design

### 1. The `exosuit.toml` Configuration

We define the "Blessed Menu" in the workspace root.

```toml
[tasks]
test-core = { cmd = "pnpm test --filter exosuit-core", desc = "Run core tests", cwd = "root" }
build-ext = { cmd = "pnpm build:ext", desc = "Compile the VS Code extension", cwd = "root" }
```

### 2. The `exo run` Command (Rust)

We implement `exo run <task>` in the Rust CLI.

- **Logic**: It reads `exosuit.toml`, finds the task, sets the CWD to the workspace root (or specified `cwd`), and executes the command.
- **Discovery**: `exo run --list` prints the available tasks in a format optimized for LLM consumption.

### 3. The "Single Tool" Policy

We do **NOT** contribute a new `exosuit_run_task` tool to the VS Code Chat API.

- **The Agent** uses the standard `run_in_terminal` tool.
- **The Instruction**: "To run project tasks, execute `exo run <task>` in the terminal."
- **The Benefit**: The agent doesn't have to choose between tools. It just uses the shell, but the _commands_ it types are strictly disciplined.

### 4. Context Injection (VS Code Extension)

The VS Code Extension plays a critical role in "priming" the agent.

- **Watcher**: It watches `exosuit.toml`.
- **Injection**: It injects the output of `exo run --list` into the Agent's System Prompt or Context.
  > **System Context**:
  > "You are working in an Exosuit project.
  > AVAILABLE COMMANDS:
  >
  > - `exo run test-core`: Run core tests
  > - `exo run build-ext`: Compile the VS Code extension"

### 5. The Sidebar Loop (Human-Agent Alignment)

The Sidebar visualizes the same `exosuit.toml` file.

- **UI**: A list of clickable buttons for each task.
- **Action**: When the Human clicks a button, the extension sends `exo run <task>` to the integrated terminal.
- **Reinforcement**: If the Agent is watching the terminal (or if we feed terminal history back to the context), it _sees_ the correct pattern being used. "Ah, the human ran `exo run test-core`. I should do that too."

### 6. Artifact Discovery (The Manifest)

To prevent "Blind Writes" (guessing file paths), the Exo-Shell pattern extends to **File Discovery**.

- **The Problem**: Agents often guess paths (e.g., legacy execution projections versus canonical state-backed artifacts) or locations of key artifacts.
- **The Solution**: The `exo` CLI exposes the canonical paths for dynamic artifacts.
  - `exo context paths`: Returns JSON/Text of key files.
    ```json
    {
      "plan": "canonical:project-state",
      "implementationPlan": "canonical:task-state"
    }
    ```
- **Injection**: The VS Code Extension (which maintains a file watcher/trie) injects this map into the Agent's context.
  > **System Context**:
  > "KEY ARTIFACTS:
  >
  > - Plan: canonical project state
  > - Execution: canonical task state"

## Novel Combinations

This approach leverages our unique stack:

- **Rust (`exo`)**: Handles the "heavy lifting" of process management, CWD enforcement, and configuration parsing.
- **VS Code (`Extension`)**: Handles the "Context Management" (injecting the menu) and "UI" (Sidebar).
- **LLM (`Copilot`)**: Uses its native "Shell" capability but is constrained by the "Menu" we provide.

## Drawbacks

- **Dependency**: Requires `exo` to be in the path (handled by the extension).
- **Overhead**: Slight overhead of spawning the Rust process.

## Alternatives

- **Separate Tool**: The previous proposal (separate `run_task` tool). Rejected because it creates "Tool Confusion" (Cognitive Noise).
- **Shell Aliases**: Just setting up `alias test=...`. Harder to manage and document dynamically.
