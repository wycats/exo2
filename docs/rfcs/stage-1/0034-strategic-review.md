<!-- exo:34 ulid:01kg5kp2cks0a7y5jwprhc2djm -->

# RFC 34: Strategic Plan Review

- **Supersedes**: RFC 10114


- **Superseded by**: -




# RFC 0034: Strategic Plan Review (The Groomer)


## Summary

This RFC proposes a new `exo plan review` command and a "Strategic Review Protocol" to maintain the health of the project plan. It introduces the concept of "Epoch Bankruptcy" to handle stale or misaligned plans and defines a heuristic for inferring the "Type of Progress" required.

## Motivation

### The "Stale Plan" Problem

Long-running projects accumulate "Plan Debt":

- **Zombie Epochs**: Future epochs planned months ago that are no longer relevant.
- **Non-Linearity**: Jumping between epochs creates a confusing history.
- **Orphaned RFCs**: Ideas that were approved but never scheduled.

### The "Progress" Ambiguity

"Making progress" means different things at different times. We define three modes of progress:

1.  **Leverage (Exploit)**: High-leverage work now that helps soon. Unblocking dependencies, building tools, or finishing "almost done" features.
2.  **Discovery (Explore)**: Researching, designing, and prototyping. Necessary when the path forward is unclear.
3.  **Clarity (Groom)**: Cleaning up the plan, archiving stale ideas, and reducing cognitive load.

The challenge is balancing **Explore** vs **Exploit**. The "Review" tool should help detect imbalances.

## Detailed Design

### 1. The `exo plan review` Command

A new subcommand for `exo plan` that performs a health check on `plan.toml`.

#### Usage

```bash
exo plan review [--fix]
```

#### Analysis Logic

The command analyzes the plan for:

- **Stale Phases**: Phases in "pending" status created > 30 days ago (if we tracked creation date) or simply preceding the current active epoch.
- **Non-Linearity**: Completed phases with higher IDs than the current active phase.
- **Orphans**: RFCs in `stage-1` not referenced in any phase.
- **Explore/Exploit Ratio**: The ratio of "Design/Research" phases to "Implementation" phases in the backlog.

#### Interactive Mode (`--fix`)

For each issue found, the tool offers:

- **Bankrupt**: Move the items to a "Bankrupt" epoch (archived state).
- **Reschedule**: Move to the end of the plan.
- **Delete**: Remove entirely.

### 2. The "Epoch Bankruptcy" Protocol

When a set of planned epochs is no longer aligned with reality (e.g., Epoch 15/16/17 after jumping to 18), we declare "Bankruptcy".

**The Protocol:**

1.  **Capture**: Extract all "pending" tasks and RFCs from the bankrupt epochs.
2.  **Liquidate**: Move these items to a new `epoch-99-backlog` or `ideas.toml` with a `source: bankruptcy` tag.
3.  **Close**: Mark the old epochs as `aborted` or `superseded`.
4.  **Re-Plan**: Create a new, clean epoch for the immediate next steps.

### 3. Progress Inference Heuristic

The agent should use the following heuristic to determine the "Type of Progress":

- **IF** `plan.toml` has > 3 pending phases in the current epoch **AND** no active blockers:
  - **Mode**: **Leverage** (Execute the plan).
- **IF** `plan.toml` has "Bankrupt" or "Stale" markers **OR** the user asks "What's next?":
  - **Mode**: **Clarity** (Groom the plan).
- **IF** `ideas.toml` has high-priority "User" ideas not in the plan:
  - **Mode**: **Discovery** (Update plan to include features).

**The "Leverage" Check**:
The tool will specifically highlight "Low Hanging Fruit" — tasks or phases that are:

1.  Small (few tasks).
2.  Unblock other phases.
3.  Have completed RFCs (Stage 2+).

## Updates to RFC 0023

This RFC extends RFC 0023 by adding `exo plan review` to the command set.
