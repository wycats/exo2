<!-- exo:115 ulid:01kg5kp2grb2mze2kxfa1xmwre -->

# RFC 115: Externalized Prompts


# RFC 0115: Externalized Prompts

## Context & Problem

Currently, the prompts used by the Exosuit extension (e.g., for the "Assess" button, or the system prompt for `@exosuit`) are hardcoded in the TypeScript source code. This has several drawbacks:

1.  **Iteration Speed**: Changing a prompt requires recompiling the extension.
2.  **Opacity**: Users cannot easily see or modify the prompts that drive the agent's behavior.
3.  **Rigidity**: We cannot easily have different prompt strategies for different project types or phases without complex logic.

## Proposal: `prompts.toml`

We propose moving these prompts into a configuration file, likely `docs/agent-context/prompts.toml` (or `.exosuit/prompts.toml`).

### Design Goals

- **Hot Reloading**: The extension should read this file on demand (or watch for changes), allowing for instant prompt iteration.
- **Template Support**: Prompts should support simple variable interpolation (e.g., `{task}`, `{file}`).
- **Scenario Grouping**: Prompts should be organized by feature or scenario.

### Proposed Structure (TOML)

```toml
[global]
# Shared context or instructions
style = "Be concise and evidence-based."

[walkthrough]
# Used when the "Assess" button is clicked
assess_task = """
Assess the status of the following task:
Task: "{task}"

Context:
{context}

Instructions:
1. Search for evidence in the codebase.
2. Do NOT mark the task as complete.
3. Report your findings.
"""

[chat]
# The system prompt for the @exosuit participant
system_instruction = """
You are @exosuit, a project assistant.
{style}
"""
```

## Lifecycle & Updates (The "Cascading Configuration" Strategy)

To address the tension between "User Ownership" (Axiom 5) and "Tool Updates", we will use a **Cascading Configuration** model.

### 1. The Hierarchy

The extension will resolve prompts in the following order (last match wins):

1.  **Internal Defaults**: The extension ships with a baked-in `defaults.toml` containing the latest "core" prompts.
2.  **Workspace Config**: `docs/agent-context/prompts.toml` (if it exists).

### 2. Merge Logic

The configuration loader will perform a **deep merge**.

- **Scenario**: If the user only wants to customize the "Assess" prompt, they only need to define `[walkthrough] assess_task` in their file.
- **Benefit**: The user automatically receives updates to _other_ prompts (e.g., new features) when they update the extension, without their custom overrides being clobbered.

### 3. Bootstrapping

When a new project is initialized (or if the file is missing):

- **Action**: The bootstrap script (or extension) creates a `docs/agent-context/prompts.toml`.
- **Content**: It should be **mostly comments**, documenting the available keys and their default values, but leaving them commented out. This encourages "inheritance by default" while making customization discoverable.

```toml
# docs/agent-context/prompts.toml

# [global]
# style = "Be concise and evidence-based."

# [walkthrough]
# assess_task = "..." (Default value shown in comment)
```

## Integration Plan

1.  **Define Schema**: Create a Zod schema for the TOML configuration.
2.  **Configuration Service**: Implement a service that loads defaults and merges the workspace file.
3.  **File Watcher**: Watch `prompts.toml` for hot-reloading.
4.  **Migration**: Move existing hardcoded strings to the internal `defaults.toml`.

## Open Questions

- **Security**: Are there any injection risks with user-defined prompts? (Low risk, as it's client-side generation).

