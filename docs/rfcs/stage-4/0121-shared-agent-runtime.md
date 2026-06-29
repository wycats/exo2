<!-- exo:121 ulid:01kg5kp2h2bebv6c02zeyqznqk -->

# RFC 121: Shared Agent Runtime


# RFC 0121: Shared Agent Runtime

## Context

Currently, the "Agent Loop"—the logic that handles the recursive cycle of `Model Request -> Tool Execution -> Model Reaction`—is hardcoded within `ExosuitChatParticipant.ts`.

This logic includes several critical, non-trivial components:

1.  **Literate Interceptor**: Intercepts the stream to parse RTD (Rich Text DOM) blocks and execute tools.
2.  **History Reconstruction**: Rebuilds the chat history from VS Code's flattened format, injecting hidden tool outputs and system messages.
3.  **Recursion Limit**: Manages the maximum number of turns to prevent infinite loops.
4.  **Error Handling**: Gracefully handles tool failures and model errors.

We recently introduced a second participant, `@exosuit-triage`, to handle idea management. Currently, this participant is implemented as a simple "Chat-CLI" using regex matching. It cannot use tools dynamically, cannot render rich UI (RTD), and does not benefit from the robustness of the main agent loop.

## Problem

1.  **Duplication/Fragmentation**: If we want `@exosuit-triage` to be "smart" (e.g., "Find all ideas about testing"), we would have to copy-paste the agent loop logic.
2.  **Inconsistency**: Improvements to the core agent logic (e.g., better history handling, new RTD tags) are not automatically available to other participants.
3.  **Maintenance**: Fixing bugs in the agent loop requires modifying the specific participant file, rather than a shared core.

## Proposal

Extract the core agent logic into a reusable **`AgentRuntime`** class.

### The `AgentRuntime` Abstraction

The `AgentRuntime` will encapsulate the complexity of the conversation loop. Participants will become thin configuration layers that provide:

1.  **Identity**: System Prompt, Icon, Name.
2.  **Capabilities**: A set of Tools specific to their domain.
3.  **Context**: Initial context (e.g., workspace root, specific files).

### Architecture

```typescript
interface AgentConfig {
  toolRegistry: ToolRegistry;
  workspaceCache: WorkspaceCache;
  logger: LogService;
}

interface RequestOptions {
  systemPrompt: string; // Dynamic context injected per-request
  model?: vscode.LanguageModelChat; // Optional model override
}

class AgentRuntime {
  constructor(private config: AgentConfig) {}

  async handleRequest(
    request: vscode.ChatRequest,
    context: vscode.ChatContext,
    stream: vscode.ChatResponseStream,
    token: vscode.CancellationToken,
    options: RequestOptions,
  ): Promise<vscode.ChatResult> {
    // 1. Reconstruct History (User + Assistant + Hidden Tool Outputs)
    //    - Extracts persisted tool outputs from ChatResult metadata
    const messages = this.reconstructHistory(context.history);

    // 2. Add Current User Prompt
    messages.push(vscode.LanguageModelChatMessage.User(request.prompt));

    // 3. Enter Agent Loop
    let turn = 0;
    const allToolOutputs = [];

    while (turn < MAX_TURNS) {
      // a. Send Request to Model
      // b. Intercept Stream (LiterateInterceptor)
      // c. Execute Tools
      // d. Update History with Assistant Response + Tool Outputs
      // e. Collect tool outputs for persistence
      // f. Loop if tools were used
    }

    // 4. Return Result with Metadata for Persistence
    return {
      metadata: {
        toolOutputs: allToolOutputs,
      },
    };
  }
}
```

### Key Considerations

1.  **Dynamic Context**: The `systemPrompt` is passed to `handleRequest`, not the constructor. This allows the participant to rebuild the context (e.g., reading the latest canonical project state) for every message.
2.  **Persistence Protocol**: The Runtime must return `toolOutputs` in the `ChatResult` metadata. This ensures that when the user reloads the window, the history reconstruction logic can find the output of tools that were run previously.
3.  **Literate Interceptor**: The runtime owns the `LiterateInterceptor`, ensuring that all agents support RTD (Rich Text DOM) streaming and parsing.

### Migration Plan

1.  **Extract**: Move the loop logic from `ExosuitChatParticipant.ts` to `packages/exosuit-vscode/src/agent/AgentRuntime.ts`.
2.  **Refactor Main Agent**: Update `ExosuitChatParticipant` to instantiate and use `AgentRuntime`.
3.  **Upgrade Triage Agent**: Update `TriageParticipant` to use `AgentRuntime`, equipping it with `IdeaService` tools instead of regex handlers.

## Benefits

- **Unified Intelligence**: All participants become "Agents" capable of reasoning and tool use.
- **Rich UI Everywhere**: Any participant can emit RTD tags (diffs, file trees, buttons).
- **Single Point of Fix**: Improvements to the loop benefit the entire ecosystem.
