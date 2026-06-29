<!-- exo:158 ulid:01kg5kp2jv3f28p22g7p7ad60v -->

# RFC 158: Structured Agent Command Discipline


# RFC: Structured Agent Command Discipline

## Summary

This RFC proposes replacing free-form shell execution with a structured **"Task Menu"** system defined in `exosuit.toml`. This enforces discipline (CWD, specific binaries, forbidden patterns) by restricting the agent to a set of "Blessed Commands" exposed via a specialized VS Code Tool.

## Motivation

- **The "Guessing Game"**: Agents often guess the wrong command (e.g., `vitest` instead of `pnpm test`, `npm install` instead of `pnpm install`).
- **The "Lost Agent"**: Agents `cd` into subdirectories and forget to return, breaking subsequent commands.
- **The "Wild West"**: Agents use `npx` or other ephemeral tools that may not be deterministic or approved.
- **Human Intent**: Humans know the "Happy Path" (the correct scripts). We should explicitly provide this map to the agent.

## Detailed Design

### 1. The `exosuit.toml` Configuration

We introduce a configuration file in the workspace root that defines the "Blessed Menu".

```toml
[agent.tasks]
test-core = { cmd = "pnpm test --filter exosuit-core", desc = "Run core tests", cwd = "root" }
build-ext = { cmd = "pnpm build:ext", desc = "Compile the VS Code extension", cwd = "root" }
check = { cmd = "./scripts/check", desc = "Run full CI check", cwd = "root" }

[agent.discipline]
allow-shell = false # If true, allows raw run_in_terminal (with warnings). If false, strictly enforces tasks.
forbidden-patterns = ["npx", "cd ", "npm install"]
```

### 2. The VS Code Tooling (`lmTools`)

We will use the `contributes.languageModelTools` contribution point in `package.json` to expose these tasks.

- **Tool Name**: `exosuit_run_task`
- **Parameters**: `{ "task": "string" }`
- **Description**: "Execute a project-specific task. Use this instead of running shell commands directly."

**Dynamic Context**:
Since we cannot put dynamic enums in `package.json`, we will inject the list of available tasks into the **System Prompt** or **Agent Context**.

> **System Prompt Injection**:
> "You have access to the following project tasks:
>
> - `test-core`: Run core tests
> - `build-ext`: Compile the VS Code extension
>   ...
>   Use the `exosuit_run_task` tool to execute them."

### 3. Execution Engine

The implementation of `exosuit_run_task` (in the Extension Host) will:

1.  **Lookup**: Find the task in `exosuit.toml`.
2.  **Validate**: Ensure it exists.
3.  **Sanitize**: Ensure the CWD is correct.
    - It will execute the command in the specified `cwd` (defaulting to workspace root).
    - It effectively wraps the command: `(cd $ROOT && $CMD)`.
4.  **Execute**: Run the command and stream the output.

### 4. Enforcement: The "Lurking Tool" Problem

A common concern is that even if we provide `exosuit_run_task`, the default VS Code `run_in_terminal` tool (or similar generic shell tools) will still be available to the model, leading to "Command Drift" where the agent ignores our blessed menu.

**The Solution: Participant Isolation**

This discipline is primarily enforced by the **`@exosuit` Chat Participant**.

- **Isolation**: Chat Participants do not automatically inherit the default toolset of GitHub Copilot. They only have access to the tools explicitly provided to them.
- **Whitelist**: The `@exosuit` participant will **NOT** be given access to a generic `run_in_terminal` tool. It will **ONLY** have access to `exosuit_run_task`.
- **Result**: It is physically impossible for the `@exosuit` agent to run an arbitrary shell command. It _must_ use a task from the menu.

**For General Copilot (YOLO Mode)**:
If the user interacts with the standard Copilot (outside of `@exosuit`), the generic tools _will_ be lurking. In this case, we rely on **System Prompt Injection** (soft enforcement) to guide the model towards `exosuit_run_task`, but we accept that strict discipline cannot be guaranteed.

### 5. Handling "Missing" Tasks

If the agent needs to do something not in the menu:

1.  It should ask the user to add it to `exosuit.toml`.
2.  Or, if `allow-shell` is true, it can fall back to `run_in_terminal` (but this is discouraged).

## VS Code Integration Details

- **Contribution Point**:
  ```json
  "contributes": {
    "languageModelTools": [
      {
        "name": "exosuit_run_task",
        "tags": ["exosuit", "cli"],
        "toolReferenceName": "exosuit_run_task",
        "displayName": "Run Exosuit Task",
        "modelDescription": "Run a pre-defined project task.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "task": { "type": "string", "description": "The ID of the task to run" }
          },
          "required": ["task"]
        }
      }
    ]
  }
  ```

## Drawbacks

- **Friction**: The user must define tasks before the agent can use them.
- **Rigidity**: The agent cannot improvise (e.g., "run tests for just this file").
  - _Mitigation_: We can allow arguments in the task definition, e.g., `cmd = "pnpm test -- $ARGS"`.

## Alternatives

- **Prompt-Only**: Just tell the agent "Please use pnpm test" in the prompt. (Proven unreliable).
- **MCP Server**: Implement this as an external MCP server. (Valid, but VS Code native tools are easier to integrate if we are already an extension).

