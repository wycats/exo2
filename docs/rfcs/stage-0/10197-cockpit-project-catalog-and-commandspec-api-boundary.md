<!-- exo:10197 ulid:01kv3zzq6g2gengvdnqd4b7aap -->

# RFC 10197: Cockpit Project Catalog and CommandSpec API Boundary

**Status**: Stage 0 (Idea)
**Feature**: shared-perception-cockpit

## Summary

The browser cockpit should become a project-aware Exo surface, not a Svelte app that happens to shell out from the current working directory.

The key architectural boundary is that cockpit clients consume an Exo API shaped by ExoSpec/CommandSpec. Shelling out to `exo ... --format json` is acceptable as the first local transport, but it must live behind an adapter that can later be replaced by daemon, MCP, or direct library calls without rewriting the UI.

The project-aware cockpit spike is historical input. The next product direction
for cockpit/workbench implementation is the
[lane-centered workbench design package](../../design/lane-centered-workbench/README.md),
which treats lanes as observable execution streams rather than as aliases for
branches, worktrees, PRs, task lists, phases, or chats.

## Context

The first web cockpit spike proved that a SvelteKit app can render a read-only Exo cockpit from structured JSON roots. It also exposed the next product gap: the cockpit currently reads whatever workspace the server was started in, so it only coincidentally displays `exo2`.

A useful cockpit needs a project catalog and a project switcher. The minimum source is shared sidecar state. A better catalog should merge sidecar registry entries, local project policy, known workspace/worktree bindings, active daemon sessions, and recent projects where available.

The project-aware spike added an initial `exo project list` surface and exposed a sharper architectural boundary: a project with readable sidecar state should be visible and useful in the cockpit even when Exo does not know a local checkout path for it. A workspace root unlocks repo/git/build commands; it should not be the admission ticket for reading Exo project state.

The cockpit's primary data plane should be Exo project state, not a local checkout. Workspace roots are capability enrichments.

## Project Model

Keep these concepts separate in both Exo data and cockpit UI:

- project identity: stable id/key and display name
- state location: local/sidecar/shadow state root and sidecar key/root when known
- workspace checkout: optional local source tree path
- command context: whether workspace-oriented commands can run for this project
- view capability: which cockpit panels can render from available state

The cockpit should treat project state as readable when Exo has a valid state location. A missing workspace checkout should produce a capability diagnostic, not hide the project from the project switcher.

For the long-run project model, Exo should require at most one canonical local checkout binding per project. Additional local checkouts are worktrees of that project, not separate competing roots in the project catalog. This keeps project identity stable while still leaving room for a worktree-aware view that can show branches, paths, and per-worktree activity under the selected project.

## Proposed Direction

Maintain `exo project list --format json` through the normal ExoSpec/CommandSpec path. It should return a stable catalog of projects with enough information for a cockpit switcher and capability-aware project views:

- project id/key and display name
- repository host/owner/name or remote when known
- state policy/source such as local, sidecar, shadow, or unknown
- sidecar key/root when known
- local workspace path when known
- state-readability, workspace availability, command availability, write availability, and diagnostic fields

Refactor the cockpit package so the Svelte app consumes a typed cockpit API, not command strings. The server adapter should expose a small `ExoApiClient` abstraction such as `call(address, input)` or equivalent. The initial implementation may be `ShellExoApiClient`, but shell mechanics must not leak into Svelte components or normalized cockpit models.

## Historical Spike Findings

The project-aware spike established these useful boundaries:

- list the current resolved project
- list projects from local `exo/projects.toml` policy
- list local shared sidecar projects under known sidecar roots
- hide stale local-policy sidecar entries from the switcher while preserving compact repair diagnostics
- show valid sidecar projects even when a local workspace root is not known
- render sidecar-readable project state where available
- show capability diagnostics for missing checkout/workspace command support instead of treating the project as non-loadable
- derive a canonical checkout binding from project state when exactly one valid local workspace root is recorded
- avoid treating multiple recorded workspace roots as peer project roots; report that a canonical checkout binding is required and model additional paths as worktrees later
- keep remote registry discovery, active daemon session recents, and write actions as follow-up work

These findings remain useful API evidence. The old read-only SvelteKit UI and
shell adapter are not the current product model.

## Next Design Source

The lane-centered workbench design package should drive the next implementation
slice. In that design, project catalog and snapshot APIs are substrate for lane
creation, focus, resume, and shared perception. The first implementation should
prove that a lane can be created, focused, and resumed from canonical project
state without relying on chat history.

## Desired API Shape

The browser-facing server API should become project-aware:

- `GET /api/projects`
- `GET /api/projects/:projectId/cockpit/snapshot`

The UI should select a project explicitly and pass that project identity into snapshot reads. Ambient server cwd may remain a development fallback, but not the product model.

Project snapshots should be capability-aware. A project with sidecar state but no workspace root should still return the phase/task/RFC/project-state data Exo can read from that state location, plus diagnostics for panels that require a checkout.

When project state records a single valid local checkout path, the snapshot can use it as the canonical command context for repo/git/build panels. If project state records zero or multiple workspace roots, the snapshot should remain readable but should not guess a command root. Multiple roots are a worktree-management problem, not a reason to fork project identity.

## Design Principles

- ExoSpec/CommandSpec remains the source of truth for command names, arguments, effects, and help.
- Cockpit-specific models compose Exo outputs; they do not redefine Exo commands.
- Transport is replaceable: shell today, daemon/MCP/direct client later.
- The Svelte app should not know whether Exo was reached through a subprocess, daemon socket, MCP transport, or embedded library.
- Project switching is a first-class shared-perception feature, not a visual sidebar flourish.
- Project identity and state readability come before workspace execution capability.
- The cockpit should degrade by panel capability, not by hiding valid projects.

## Open Questions

- Should `project list` include local git worktrees and recent daemon sessions in addition to configured/sidecar-known projects?
- How should Exo learn durable workspace checkout bindings for projects whose sidecar state already exists?
- Which snapshot panels can be served entirely from sidecar state, and which require a workspace checkout?
- Should project catalog state live entirely in Exo, or should the cockpit server maintain short-lived recents for browser ergonomics?
- What is the right write-authority model for sidecar-readable projects selected outside their workspace checkout?

## Recommendation

Before expanding the web cockpit UI, keep the project catalog/API slice honest:

1. Preserve `project list` through ExoSpec/CommandSpec as the authoritative catalog surface.
2. Introduce capability fields instead of overloading `selectable`.
3. Make `/api/projects/:projectId/cockpit/snapshot` read from project identity/state location first, using workspace roots only for panels that require checkout-backed commands.
4. Keep the cockpit `ExoApiClient` boundary transport-neutral.

This turns the current visual/model spike into a real shared-perception product surface while preserving the ability to move to live daemon/trace sync next.
