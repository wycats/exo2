<!-- exo:10119 ulid:01kmzxbczbc2e3q05q7kyzvkb6 -->


# RFC 10119: The Standard Bootstrap

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**:

## Summary

This RFC defines the "Standard Bootstrap" state for all Exosuit projects and mandates that the `exo` CLI (specifically `exo init` and `exo update`) be the sole source of truth for establishing this state.

It deprecates ad-hoc shell scripts (`bootstrap.sh`, `install-prompts.sh`) in favor of a robust, versioned, and idempotent CLI workflow.

## Motivation

Currently, initializing a new Exosuit project is a fragmented experience:
1.  `exo init` creates a skeleton but misses critical files (`implementation-plan.toml`, RFC scaffolding).
2.  Global prompts (like the new Coherence Matrix) require manual installation or separate scripts.
3.  Updating an existing project to the latest "Exosuit Way" is manual and error-prone.

We need a "One Command" guarantee: `exo init` sets up a perfect new environment, and `exo update` migrates an old one to the current standard.

## The Standard Bootstrap State

A fully bootstrapped Exosuit project MUST contain:

### 1. The Brain (Context)
*   `docs/agent-context/plan.toml` (The Big Picture)
*   `docs/agent-context/current/implementation-plan.toml` (The Current Phase)
*   `docs/agent-context/ideas.toml` (The Backlog)
*   `docs/agent-context/modes.toml` (The Persona)
*   `docs/agent-context/prompts.toml` (The Configuration)
*   `docs/agent-context/axioms.workflow.toml` (Workflow axioms)
*   `docs/agent-context/axioms.system.toml` (System axioms)

### 2. The Memory (Docs)

Notes:

- Decisions are captured as RFCs under `docs/rfcs/` (not a separate decisions file under `docs/agent-context/`).

*   `AGENTS.md` (The Constitution, referencing Global Prompts)
*   `docs/design/axioms.design.toml` (Design axioms)
*   `docs/manual/` (The Code Reality)
*   `docs/rfcs/` (The Law)

### 3. The Tooling (Global)
*   **Global Prompts**: The project must have access to the standard library of prompts (Coherence Matrix, etc.), installed via `exo` to the user's global config.

## Implementation Plan

### 1. Update `exo init`
*   **Expand Templates**: Add templates for all missing TOML files.
*   **Global Install**: Trigger the installation of global prompts to `~/.config/Code/User/prompts/exo/`.
*   **Config Generation**: Generate a default `prompts.toml` that references these global prompts.

### 2. Update `exo update`
*   **Idempotency**: Ensure it can run on an existing project without destroying data.
*   **Backfill**: Create missing canonical scaffolding (e.g., ensure `docs/rfcs/` exists; ensure `implementation-plan.toml` exists for an active phase).
*   **Migration**: Update `AGENTS.md` to the latest version while preserving the "Mission" section.
*   **Prompt Sync**: Re-install/Update the global prompts to ensure the user has the latest version.

### 3. Deprecation
*   Remove `bootstrap.sh`.
*   Remove `scripts/install-prompts.sh` (logic moves to Rust).

## User Experience

**New Project:**
```bash
mkdir my-new-os
cd my-new-os
exo init
# > Mission? "Build a better browser"
# > Mode? "Strict"
# > [System] Installing global prompts...
# > [System] Generating context...
# > Ready.
```

**Existing Project:**
```bash
cd my-old-os
exo update
# > [System] Ensured docs/rfcs/ exists.
# > [System] Updated 'AGENTS.md' to v2.0.
# > [System] Refreshed global prompts.
# > Ready.
```
