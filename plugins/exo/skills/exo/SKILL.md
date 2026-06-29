---
name: exo
description: Use when working in an Exo-enabled repository from Codex and the exo-run MCP tool is available.
---

# Exo

Use the `exo-run` MCP tool as the authoritative interface to Exo project state.

## Operating Rules

- Start an Exo work session with `exo-run` command `status`, then read the task plan with `task list` when task state matters.
- Send commands without the leading `exo`; for example, use `status`, `task list`, or `task complete <id> --log $1`.
- Treat `exo-run` as an Exo CLI-language runner, not a shell. Do not use pipes, redirects, command substitution, environment assignment prefixes, or glob expansion.
- Use the `args` array with `$1`, `$2`, and later placeholders for multiline text instead of shell escaping.
- Keep the phase loop current with Exo commands as work progresses.

## Confirmations

- If Exo asks for execution approval, ask the human whether to approve the action in plain language. If they approve, continue the same action with the hidden approval data returned by Exo.
- If Exo asks for outcome review, ask the human the question and options Exo returned. If they approve recording the completed outcome, continue the same completion command with the hidden approval data.
- Do not display hidden confirmation fields, raw JSON, tickets, or protocol names to the user unless they explicitly ask for raw protocol details.
