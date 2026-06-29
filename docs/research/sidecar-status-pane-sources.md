# Sidecar Status Pane Source Inventory

**Date**: 2026-05-22  
**Phase**: Plan Sidebar Sidecar Status Pane  
**Task**: `sidebar-sidecar-status-contract::inventory-status-json-sources`

## Purpose

The Plan sidebar sidecar pane should make sidecar state visible without requiring the user to remember which CLI commands to run. The pane must consume structured JSON only; it must not scrape human output.

## Existing structured sources

### `exo sidecar status --format json`

**Effect**: pure  
**Current Rust output**: `SidecarStatusOutput` in `tools/exo/src/command/sidecar.rs`

Fields exposed:

- `kind`: `sidecar.status`
- `ok`
- `linked`
- `project_id`
- `policy`
- `sidecar_key`
- `sidecar_root`
- `auto_commit`
- `auto_push`
- `manifest_path`
- `projection_dir`
- `db_path`
- `runtime_dir`
- `discovery`
- `next_actions`

Behavior:

- For sidecar-linked projects, this is the best source for binding state and discovery guidance.
- `discovery` is present only when status chooses to run discovery. Current behavior runs discovery for linked projects whose sidecar repo has no remote.
- If the sidecar repo already has its own remote, `discovery` is `null` and `next_actions` is empty.
- For unlinked projects, `linked = false`, project policy fields are present, and sidecar-specific paths are null.

Use in pane:

- Primary source for linked/unlinked state.
- Primary source for sidecar key/root and local path details.
- Primary source for advisory discovery/provenance when available.
- Primary source for suggested next actions when discovery is available.

### `exo sidecar repo status --format json`

**Effect**: the combined `sidecar repo` operation is classified write in command metadata, but the `status` action is read-only at runtime.  
**Current Rust output**: `SidecarRepoStatusOutput` in `tools/exo/src/command/sidecar.rs`

Fields exposed:

- `kind`: `sidecar.repo.status`
- `ok`
- `sidecar_root`
- `branch`
- `clean`
- `has_remote`
- `remote`
- `ahead`
- `behind`
- `files`

Behavior:

- Requires an active sidecar binding and an independent sidecar git repository.
- Shows dirty file details that `sidecar status` does not include.
- Shows remote presence and branch/upstream state.
- Does not expose discovery or binding metadata.

Use in pane:

- Primary source for sidecar git cleanliness and file list.
- Primary source for remote/branch/ahead/behind details.
- Needs to be combined with `sidecar status` for user-facing context.

Contract issue:

- Because command metadata classifies `sidecar repo` as write, this is awkward as an eager TraceCache root. The pane either needs a read-only root/action for sidecar repo status or an aggregate read-only operation that includes this information.

### `exo sidecar discover --format json`

**Effect**: pure  
**Current Rust output**: `SidecarDiscoveryOutput` in `tools/exo/src/command/sidecar.rs`

Fields exposed:

- `kind`: `sidecar.discovery`
- `ok`
- `repository`
  - `host`
  - `owner`
  - `repo`
  - `remote`
- `identity`
  - `source`
  - `login`
- `registry`
  - `source`
  - `label`
  - `profile_repo`
  - `path`
  - `version`
- `match`
  - `kind`
  - `key`
- `confidence`
- `proposal`
  - `key`
  - `root`
  - `remote`
  - `auto_push`
  - `would_mutate_config`
  - `requires_remote_acceptance`
- `failure`
- `checked`
- `attempt_index`
- `source_summary`
- `next_actions`

Behavior:

- Bare discover uses profile registry fetch by default.
- `--registry-file` is a local-file override.
- Discovery is pure: it does not write project policy or home state.

Use in pane:

- Useful for unlinked workspaces or linked remote-less sidecars when the pane wants to show what would be applied.
- Useful for provenance: registry source, identity source, checked attempts, confidence, and proposal.
- Duplicates data nested inside `sidecar status.discovery` when status runs discovery.

### `exo status --format json`

**Effect**: pure  
**Current Rust output**: `StatusJson` in `tools/exo/src/status.rs`

Sidecar-related field:

- `sidecar_sync`: optional `SidecarRepoSyncStatus`
  - `kind`
  - `ok`
  - `sidecar_root`
  - `branch`
  - `clean`
  - `has_remote`
  - `remote`
  - `ahead`
  - `behind`
  - `issue`

Behavior:

- Provides compact sidecar sync health for the current project.
- Does not expose discovery or binding/proposal details.
- Useful as a high-level status badge, but not sufficient for the pane.

Extension gap:

- `packages/exosuit-vscode/src/types/progress.ts` currently defines `ExoStatusResponse` without `sidecar_sync`, so typed extension consumption is incomplete.

## VS Code plumbing

### TraceCache roots

`packages/exosuit-vscode/src/extension.ts` currently registers these roots:

- `context-snapshot`
- `phase-details`
- `status`
- `plan-read`
- `rfc-pipeline`

No sidecar-specific root is registered.

### Provider bridge

`packages/exosuit-vscode/src/services/TracedProvider.ts` can already combine multiple TraceCache roots into a tree provider. The sidecar pane can follow the existing Phase Details / Epoch Context provider pattern.

### Plan container

`packages/exosuit-vscode/package.json` currently contributes only these Plan views:

- `exosuit.projectPlan`
- `exosuit.ideasBacklog`

There is no `exosuit.sidecarStatus` view yet.

## Current workspace sample shape

Recent command output from the active workspace confirmed the two most important source shapes:

- `sidecar status` exposes binding state, paths, optional discovery, and `next_actions`.
- `sidecar repo status` exposes sidecar git state: branch, clean/dirty status, remote, ahead/behind, and files.

The pane should be designed around these structured fields rather than parsing human messages.

## Contract gaps

1. No aggregate view model exists for the sidebar pane.
2. `sidecar status` does not include dirty file details from `sidecar repo status`.
3. `sidecar repo status` does not include binding/discovery metadata.
4. `exo status.sidecar_sync` is compact and typed incompletely in the extension.
5. `sidecar repo status` is read-only at runtime but inherits write classification from the combined `sidecar repo` command.
6. Command metadata describes command arguments, not response shape/nullability.
7. Discovery is optional and conditional; the view model must handle `discovery = null` explicitly.
8. The pane needs state-specific next-action semantics, not a generic text blob.

## Recommendation for next task

For `sidebar-sidecar-status-contract::define-sidebar-status-view-model`, define a single extension-facing view model with these sections:

- `binding`
  - linked/unlinked
  - policy
  - project ID
  - sidecar key/root
  - manifest/projection/db/runtime paths
  - auto-commit/auto-push
- `repository`
  - branch
  - clean
  - remote configured
  - remote name
  - ahead/behind
  - dirty files
  - sync issue
- `discovery`
  - present/absent
  - source/provenance
  - profile repo/path
  - identity source/login
  - match kind/confidence
  - proposal
  - checked attempts
  - failure
- `actions`
  - bootstrap from discovery
  - commit sidecar state
  - configure remote
  - push
  - inspect failure/details
- `diagnostics`
  - source fetch errors
  - missing roots
  - effect-classification limitations

The implementation can either compose `sidecar status` + `sidecar repo status` in the extension or add a read-only aggregate CLI/machine operation. The contract task should decide which path gives the sidebar the most stable shape with the least duplication.
