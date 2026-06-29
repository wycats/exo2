<!-- exo:88 ulid:01kmzxefeqpew1cfd4etfs6cgw -->

# RFC 88: Agent Discipline: Context Injection and Command Control


# RFC 0088: Agent Discipline: Context Injection and Command Control

## Summary

This RFC proposes a two-pronged approach to agent discipline: (1) **Organic Context Injection** that provides agents with real-time workspace state awareness, and (2) **Structured Command Control** that restricts agents to blessed, validated commands defined in `exosuit.toml`. Together, these prevent "Existential Crises" (confusion from stale state) and "Command Drift" (using wrong tools or CWD).

## Consolidated From

This RFC merges:

- **RFC: Organic Context Injection** - State-aware agents with persistent tree metadata
- **RFC: Structured Agent Command Discipline** - Task menu system with CWD enforcement

## Motivation

Agents face two related but distinct failure modes:

### The Contextual Confusion Problem

- **"Existential Crisis"**: Agents spiral when `read_file` shows correct code but `run_build` shows an error. They lack temporal context to know the build is stale.
- **Token Waste**: Agents spend massive compute "thinking" about contradictions trivially solvable with timestamp comparisons.
- **Unused Infrastructure**: We already maintain a real-time graph of the workspace (for the Sidebar). We should expose this to agents.

### The Command Chaos Problem

- **"Guessing Game"**: Agents guess wrong commands (e.g., `vitest` instead of `pnpm test`, `npm install` instead of `pnpm install`).
- **"Lost Agent"**: Agents `cd` into subdirectories and forget to return, breaking subsequent commands.
- **"Wild West"**: Agents use `npx` or ephemeral tools that may not be deterministic or approved.

Both problems share a root cause: **agents lack explicit, structured knowledge of the workspace environment and its intended operations**.

## Detailed Design

### Part 1: Organic Context Injection (State Awareness)

#### 1.1 The State Graph

Extend the Extension's `ExosuitTreeProvider` to track **Operational Metadata** for workspace files:

**Metadata Tracked:**

- `lastModified`: Timestamp of file change.
- `lastBuild`: Timestamp of last successful/failed build.
- `lastTest`: Timestamp and status of last test run covering this file.
- `dependencies`: Files/packages this file depends on.

**State Computation:**

- **Fresh**: File unchanged since last successful build/test.
- **Modified**: File changed since last build/test.
- **Stale**: File unchanged but dependencies changed.
- **Unknown**: No build/test data available.

#### 1.2 The Injection Mechanism

When the agent is active (via `@exosuit` participant), inject a "State Summary" into the system prompt or context window.

**Example Context Block:**

```text
[WORKSPACE STATE]
- src/lib/inspector/overlay.ts
  Status: Modified (10:00:05)
  Last Build: Failed (10:00:00) → ⚠️ File is newer than build
  Last Test: Passed (09:55:00) → ⚠️ Tests are stale

- src/lib/inspector/utils.ts
  Status: Fresh
  Last Build: Passed (09:58:00)
  Last Test: Passed (09:55:00)
```

#### 1.3 Context Budgeting

Injecting state for every file exhausts context window.

**Heuristic - Only inject state for:**

- **Active Files**: Files currently open in the editor.
- **Referenced Files**: Files explicitly mentioned in the chat or "read" by the agent.
- **Problem Files**: Files with active diagnostics (errors/warnings).
- **Recently Modified**: Files changed in the last 30 minutes.

#### 1.4 Dependency-Aware State

In monorepos, changes in one package invalidate builds in dependents.

**Solution**: The State Graph must be dependency-aware.

- If `core/src/lib.rs` is modified, the "Last Build" status for `extension` transitions to `Stale (Dependency Changed)`.
- Use `package.json`, `Cargo.toml`, and other manifest files to build the dependency graph.

#### 1.5 External Build Detection

Users might run builds in separate terminals or via `git pull`.

**Solution**:

- **File Watchers**: Watch `target/`, `dist/`, `node_modules/` directories to detect output changes.
- **Epistemic Humility**: Report state as "Last _Observed_ Build". If the agent sees a discrepancy, it should trust its tools (run a fresh build) over cached state.

### Part 2: Structured Command Control (Task Menu)

#### 2.1 The `exosuit.toml` Configuration

Define a "Blessed Menu" of commands in the workspace root.

**Example Configuration:**

```toml
[agent.tasks]
test-core = { cmd = "pnpm test --filter exosuit-core", desc = "Run core tests", cwd = "root" }
build-ext = { cmd = "pnpm build:ext", desc = "Compile the VS Code extension", cwd = "root" }
check = { cmd = "./scripts/check", desc = "Run full CI check", cwd = "root" }
check-rust = { cmd = "cargo check --workspace", desc = "Check Rust code", cwd = "root" }

[agent.discipline]
allow-shell = false  # Strictly enforce tasks (no raw shell access)
forbidden-patterns = ["npx", "cd ", "npm install", "npm i"]
```

#### 2.2 VS Code Language Model Tool

Expose tasks via `contributes.languageModelTools` in `package.json`.

**Tool Definition:**

```json
{
  "name": "exosuit_run_task",
  "tags": ["exosuit", "cli"],
  "displayName": "Run Exosuit Task",
  "modelDescription": "Run a pre-defined project task (build, test, check, etc.)",
  "inputSchema": {
    "type": "object",
    "properties": {
      "task": {
        "type": "string",
        "description": "The ID of the task to run (e.g., 'test-core', 'build-ext')"
      }
    },
    "required": ["task"]
  }
}
```

**Dynamic Context Injection:**
Since we cannot put dynamic enums in `package.json`, inject available tasks into the **System Prompt**:

```text
[AVAILABLE TASKS]
You have access to the following project-specific tasks:
- test-core: Run core tests
- build-ext: Compile the VS Code extension
- check: Run full CI check
- check-rust: Check Rust code

Use the `exosuit_run_task` tool to execute them. Do NOT use generic shell commands.
```

#### 2.3 Execution Engine

Implementation of `exosuit_run_task` in the Extension Host:

1. **Lookup**: Find the task in `exosuit.toml`.
2. **Validate**: Ensure it exists and agent has permission.
3. **Sanitize CWD**: Execute in the specified `cwd` (defaulting to workspace root).
   - Effectively wraps: `(cd $ROOT && $CMD)`
4. **Execute**: Run command and stream output.
5. **Update State**: After execution, update the State Graph with new build/test timestamps.

#### 2.4 Enforcement via Participant Isolation

**Key Insight**: Chat Participants do not automatically inherit default Copilot tools.

**The Solution**:

- The `@exosuit` participant **ONLY** has access to `exosuit_run_task`.
- It does **NOT** have access to generic `run_in_terminal` or shell tools.
- This makes it physically impossible for the agent to run arbitrary shell commands.

**For General Copilot** (outside `@exosuit`):

- Generic tools are available, but System Prompt encourages using `exosuit_run_task`.
- This is "soft enforcement" (guidance only).

#### 2.5 Handling Missing Tasks

If the agent needs a command not in the menu:

1. **First**: Ask the user to add it to `exosuit.toml`.
2. **If `allow-shell = true`**: Fall back to generic shell (with warnings).
3. **If `allow-shell = false`**: Hard stop, user must update config.

### Part 3: Integration & Synergies

#### 3.1 State + Commands = Smart Execution

When the agent runs a task, the output includes relevant state:

```text
> exosuit_run_task("build-ext")
[Pre-Check]
  - packages/exosuit-core/src/index.ts: Modified (10:05:00)
  - packages/exosuit-vscode/src/extension.ts: Fresh
[Building...]
[Build Successful] (10:05:12)
[State Updated]
  - All files now Fresh relative to this build.
```

#### 3.2 State Prevents Confusion

**Scenario**: Agent sees build error for code that looks correct.

**Old Behavior**: "I'm confused. Let me re-read the file 5 times."

**New Behavior**:

```text
[Context] src/lib.rs shows correct code.
[Context] However, last build was 5 minutes ago (before your edit).
[Action] Run `exosuit_run_task("build-rust")` to rebuild.
```

#### 3.3 Command Control Prevents Drift

**Scenario**: Agent wants to run tests.

**Old Behavior**: Guesses `npm test`, `yarn test`, `vitest`, etc.

**New Behavior**:

```text
[Available] test-core, test-ext, test-all
[Action] Use `exosuit_run_task("test-core")`
```

## User Experience

**System Prompt (Automatic):**

```text
[WORKSPACE STATE]
- packages/exosuit-core/src/toml.ts: Modified (10:05:00) ⚠️ Newer than last build
- crates/exosuit-core/src/lib.rs: Fresh

[AVAILABLE TASKS]
- build-ext: Compile the VS Code extension
- test-core: Run core tests
- check: Run full CI check

Use `exosuit_run_task` for all commands. Do NOT use shell tools directly.
```

**Agent Workflow:**

```text
[Agent] I need to run tests for the core package.
[Tool Call] exosuit_run_task("test-core")
[Output]
  [Pre-Check] 3 files modified since last test run
  [Running] pnpm test --filter exosuit-core
  [Result] ✓ All tests passed
  [State Updated] Test timestamp: 10:06:15
```

## Edge Cases & Open Questions

### 1. Monorepo Dependency Tracking

**Q**: How do we efficiently compute the dependency graph?
**A**: Parse `package.json`, `Cargo.toml`, `tsconfig.json`, and cache the graph. Invalidate on manifest changes.

### 2. Race Conditions (Tooling Latency)

**Q**: Agent edits file and immediately runs build—file watcher hasn't updated yet.
**A**: `exo` CLI forces a "State Flush" or waits for FS settlement before proceeding.

### 3. External Builds

**Q**: User runs build in a separate terminal—Extension doesn't know.
**A**: Watch output directories. Report state as "Last _Observed_ Build" with epistemic humility.

### 4. Task Arguments

**Q**: What if a task needs dynamic arguments (e.g., test a specific file)?
**A**: Support template variables: `cmd = "pnpm test -- $ARGS"` and allow passing `args` parameter to `exosuit_run_task`.

## Drawbacks

- **Setup Friction**: User must define tasks in `exosuit.toml` before agent can use them.
- **State Complexity**: Tracking build/test timestamps across a large workspace is non-trivial.
- **Context Tokens**: Injecting state and task lists consumes context window budget.
- **Rigidity**: Agent cannot improvise commands (trade-off for reliability).

## Alternatives

- **Prompt-Only Discipline**: Just tell the agent "use pnpm" in the prompt. (Proven unreliable).
- **MCP Server**: Implement as external MCP server. (Valid, but VS Code native tools are easier to integrate).
- **No State Injection**: Let agent figure it out from tool output. (Causes existential crises and token waste).
- **Full Shell Access**: Allow arbitrary commands with warnings. (Rejected: too error-prone).

## Implementation Phases

1. **Phase A**: Implement `exosuit.toml` parsing and `exosuit_run_task` tool.
2. **Phase B**: Build State Graph infrastructure in Extension.
3. **Phase C**: Add file watchers for build/test output detection.
4. **Phase D**: Implement state injection in `@exosuit` participant system prompt.
5. **Phase E**: Add dependency graph computation for monorepos.
6. **Phase F**: Integrate state updates with task execution.

