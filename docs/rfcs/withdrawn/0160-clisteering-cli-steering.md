<!-- exo:160 ulid:01kg5kp2jy2szebwa0v4ax2zje -->

# RFC 160: CLI Steering & VS Code Integration


# RFC: CLI Steering & VS Code Integration

## Problem

Currently, the "Agent Instructions" (the _how_) are stored in static `.prompt.md` files, while the "Agent Tools" (the _what_) are in the `exo` CLI. This separation creates friction:

1.  **Context Switching**: The user has to manually copy-paste prompts or reference them by name.
2.  **Drift**: The instructions in the prompt might drift from the actual behavior of the CLI tool.
3.  **Ergonomics**: There is no native way in VS Code to say "Help me start a phase" that pulls in both the _tooling_ to do it and the _instructions_ on how to do it right.

## Proposal

We propose unifying the "Steering" (Instructions) and the "Tooling" (Capabilities) into the `exo` CLI, and exposing this via a native VS Code integration.

### 1. Steering in `exo`

The `exo` CLI commands should support an `--ai` flag (or `ai` subcommand) that outputs the **Agent Steering** (instructions) alongside the data.

```bash
$ exo phase start --ai
# Output:
# <context>
#   [phase]
#   id = "..."
# </context>
# <instructions>
#   1. Read the plan.
#   2. Create the implementation-plan.toml...
# </instructions>
```

While we considered making this the default (on stderr), an explicit flag is cleaner for scripting and composition. The risk of "forgetting the flag" is mitigated by the VS Code integration (see below), which will always invoke the command with `--ai`.

### 2. VS Code Contribution Point

We will add a contribution point to the Exosuit VS Code extension (e.g., `exosuit.action.chat.startPhase`) that:

1.  Invokes the underlying `exo` command (e.g., `exo phase start ai`).
2.  Captures the output (Context + Steering).
3.  Injects it directly into the Chat input or context.

**User Flow:**

1.  User clicks "Start Phase" in the Sidebar (or runs a command).
2.  Extension calls `exo phase start ai`.
3.  Extension populates the Chat with: "I want to start a new phase. Here is the context and the instructions: [Context Block]".
4.  Agent receives both the _state_ and the _procedure_ in one shot.

## Open Questions

1.  **Steering Content**: We need to explicitly define the "Steering Instructions" for each command. These should be derived from the current `.prompt.md` files but refined to match the new RFC-driven workflow.

    - _Action_: Audit all `.prompt.md` files and map them to `exo` subcommands.
    - _Action_: Draft the canonical instructions for `phase start`, `phase finish`, etc. in this RFC before promotion.

2.  **Output Format**: Should the steering be plain text, Markdown, or a structured XML block (like `<instructions>`)?

    - _Proposal_: Use XML-like tags (`<instructions>`, `<context>`) within the Markdown output to help the agent distinguish between "data to process" and "rules to follow".

3.  **VS Code Integration**: How do we handle the "Chat Input" injection?
    - _Proposal_: Use the `vscode.chat.createChatSession` API (if available) or simply paste into the active chat input box via a command.

