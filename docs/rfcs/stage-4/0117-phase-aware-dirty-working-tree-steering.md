<!-- exo:117 ulid:01kg5kp2gwaz25spsy6nq3nbme -->

# RFC 117: Phase-Aware Dirty Working Tree Steering


# RFC 0117: Phase-Aware Dirty Working Tree Steering

## Summary

When the repository working tree is dirty, Exosuit should steer agents toward a **phase-aware review → recommendation → wait for feedback** loop.

This RFC standardizes:

- A phase-first orientation step (`exo phase status`).
- A lightweight, deterministic change classification used for guidance.
- A strong bias toward **commit + PR** as the usual resolution.
- A clear rule that **deletion is exceptional**, not a default cleanup mechanism.

## Motivation

Agents frequently encounter dirty working trees during iterative work. In the absence of structured guidance, a common failure mode is to “clean up” by deleting files, even when the changes are legitimate phase work.

The desired default behavior is:

1. Understand what the current phase implies about the changes.
2. Inspect and classify the changes.
3. Recommend a path (usually commit + PR).
4. Pause for user feedback before any destructive action.

## Decision

### The Default Loop

When `git status --porcelain` is non-empty:

1. **Orient**: run `exo phase status`.
2. **Inspect**: run `git status --porcelain` and `git diff`.
3. **Recommend**:
   - If changes touch source/context/RFCs: recommend **commit + PR**.
   - If changes appear generated-ish only: recommend confirming intent (commit if intentional, otherwise revert/regenerate).
4. **Wait for feedback** before destructive cleanup.

### Deletion Is Exceptional

Do not recommend deleting files unless at least one is true:

- The file is ignored/generated and disposable.
- The file is explicitly deprecated.
- The user confirms deletion is desired.

### PR Tooling in Guidance

Steering should mention `gh pr create --fill` as the preferred PR path (after committing and pushing). This is guidance, not an automatic action.

## Implementation

### CLI Steering

The `exo` CLI includes a “dirty tree” steering block that:

- Places `exo phase status` first.
- Prompts inspection via `git status --porcelain` and `git diff`.
- Provides a recommendation that includes a canonical “commit + PR” command template:

`git add -A && git commit -m "<message>" && git push -u origin HEAD && gh pr create --fill`

The recommendation confidence is adjusted based on:

- Whether the active phase suggests active work (pending tasks/steps or red step).
- Whether changes touch non-generated paths.

### Deterministic Classification (Lightweight Tripwire)

The classification is intentionally simple and deterministic:

- **context**: `docs/agent-context/**`, `docs/rfcs/**`
- **generated-ish**: `target/**`, `node_modules/**`, `.debug/**`, `dist/**`, and paths containing `/out/` or starting with `out/`
- **source**: `crates/**`, `tools/**`, `packages/**`, `scripts/**`, `src/**`
- **other**: everything else

This classification is used only to shape guidance and to prevent “delete files” as a default suggestion.

## Documentation

This policy is codified in:

- `AGENTS.md` (agent-facing workflow policy)
- the relevant stabilized workflow RFCs (authoritative reference)

## Non-Goals

- Automatically committing, pushing, or opening PRs without user confirmation.
- Building a generalized “tripwire system” framework beyond deterministic steering.
