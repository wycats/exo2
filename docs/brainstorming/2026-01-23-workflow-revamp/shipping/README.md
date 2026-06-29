# Shipping Focus Documents

These documents capture the **big-picture vision and gaps** identified during the 2026-01-23 workflow revamp session. They should guide near-term shipping decisions.

> **Last Updated**: 2026-02-03 — Added SOAR Loop progress, status indicators, new differentiators.

## How to Use This Document

This is a **canonical supplementary planning doc**. Use it to:

1. **Orient new sessions**: Read the Key Insights table to understand current gaps
2. **Track progress**: Check status indicators (🟢 addressed, 🟡 partial, 🔴 open)
3. **Guide prioritization**: Open items (🔴) are candidates for next work
4. **Understand positioning**: key-differentiators.md explains what makes Exosuit unique

**Update cadence**: Refresh status indicators when significant work completes (RFCs, epochs).

## Contents

| Document                                             | Purpose                                       |
| ---------------------------------------------------- | --------------------------------------------- |
| [workflow-disconnects.md](workflow-disconnects.md)   | The 5 core disconnects and downstream effects |
| [key-differentiators.md](key-differentiators.md)     | What makes Exosuit unique vs competitors      |
| [market-research.md](market-research.md)             | Competitive landscape analysis                |
| [repo-history-analysis.md](repo-history-analysis.md) | Evolution of the codebase and workflow        |

## Key Insights

From `workflow-disconnects.md`:

| #   | Disconnect                | Status                                                      |
| --- | ------------------------- | ----------------------------------------------------------- |
| 1   | **Workflow vs. Practice** | 🟢 Addressed — RFC 00224 (SOAR Loop)                        |
| 2   | **Visibility**            | 🟡 Partial — Dashboard Expansion epoch planned              |
| 3   | **Idea Integration**      | 🟡 Partial — Triage done, tooling gap remains               |
| 4   | **RFC/Phase Integration** | 🔴 Open                                                     |
| 5   | **Lost Concepts**         | 🔴 Open — Axioms, Modes, Walkthroughs, Manual still dormant |

## Progress Since 2026-01-23

| Work                                                                                                             | Impact                                                         |
| ---------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| [RFC 00224: SOAR Loop](../../../rfcs/stage-1/00224-the-soar-loop-a-workflow-model-for-human-ai-collaboration.md) | Formalizes workflow model (Status→Orient→Act→Review)           |
| [RFC 00225: Problems Pane](../../../rfcs/stage-1/00225-problems-pane-integration-with-soar-loop.md)              | Addresses Review phase gap with VS Code diagnostics            |
| Ideas Triage                                                                                                     | 60+ ideas categorized (7 implemented, 12 designed, 14 planned) |
| Dashboard Expansion Epoch                                                                                        | 4-phase epoch for visualization work                           |
| PER Protocol                                                                                                     | Prepare→Execute→Review documented in copilot-instructions.md   |

## Related Work

- [RFC 00224: The SOAR Loop](../../../rfcs/stage-1/00224-the-soar-loop-a-workflow-model-for-human-ai-collaboration.md) _(new)_
- [RFC 00225: Problems Pane Integration](../../../rfcs/stage-1/00225-problems-pane-integration-with-soar-loop.md) _(new)_
- [RFC 10107: RFC-Centric Workflow Model](../../../rfcs/stage-1/10107-rfc-centric-workflow-model.md)
- [Ideas Triage Research](repo-history-analysis.md#ideas-triage) _(new)_
