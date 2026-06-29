# Lane-Centered Workbench Design Package

This folder is the current product/design baseline for the lane-centered workbench direction. It extends the workspace-centered architecture described in [`docs/vision.md`](../../vision.md).

It is intentionally a **design package**, not a single RFC. The lane-centered workbench combines a product model, system contracts, UI hierarchy, and visual interaction taste. A prose RFC alone would flatten too much of what matters.

## Read order for agents

Agents working on lane-centered workbench implementation should read these files in order:

1. [`proposal.md`](./proposal.md) — the self-contained product and system proposal.
2. [`mockups.html`](./mockups.html) — the primary visual and interaction reference.
3. [`implementation-brief.md`](./implementation-brief.md) — how to implement incrementally without overbuilding.
4. [`source-map.md`](./source-map.md) — how this package relates to the existing Exo RFC/document strata.

## Important guidance

The HTML mockups are not decorative output. They are a first-class design artifact that captures hierarchy, density, terminology, interaction tone, and what the product should feel like when it is credible rather than “AI dashboard”-ish.

Do not rederive the UI solely from the Markdown proposal or current Exo implementation patterns. The Markdown captures the architectural contract. The HTML captures product taste and screen-level emphasis.

Existing Exo RFCs are source strata, but implemented contracts are still contracts. Do not assume the latest nearby RFC is the current product model; also do not ignore Stage 3+ implemented RFCs or current manual/reference docs merely because this package does not enumerate them. Use `source-map.md` to decide whether a prior RFC is preserved, reframed, superseded, or still open.

## Current thesis

A workbench lane is an **observable execution stream**. It is not a prettier name for a branch, worktree, pull request, task list, validation lane, or chat thread. It is the product object that connects those artifacts while preserving the difference between durable project truth, branch-local workspace reality, observed evidence, and situated human judgment.

Product UI can say **lane** when the workbench context is clear. Implementation and docs should say **workbench lane** or **work lane** when disambiguating from existing Exohook validation lanes.

## First implementation posture

Do not implement the whole proposal at once. The first useful slice should prove this sentence:

> An agent can create, focus, and resume a lane from canonical project state without relying on chat history.

Everything else should build from that verified loop.
