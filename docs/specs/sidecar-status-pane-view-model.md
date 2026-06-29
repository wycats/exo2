# Sidecar Status Pane View Model

**Date**: 2026-05-22  
**Phase**: Plan Sidebar Sidecar Status Pane  
**Task**: `sidebar-sidecar-status-contract::define-sidebar-status-view-model`

## Purpose

The sidecar status pane needs a stable extension-facing view model that combines sidecar binding state, sidecar repository sync state, discovery provenance, actionable next steps, and diagnostics. The pane must consume structured JSON only. It must not scrape human CLI output.

This document defines the `SidecarStatusViewModel` contract. It is separate from raw CLI response shapes so the UI can render stable states even when source data comes from multiple commands.

## Source inputs

The view model is built from these structured sources:

- `exo sidecar status --format json`
- `exo sidecar repo status --format json`
- optional `exo sidecar discover --format json`
- root `exo status --format json` field `sidecar_sync`
- `TraceCache` diagnostics for any registered source roots

The initial extension implementation can compose existing roots. A later CLI/machine-channel aggregate operation can produce this shape directly if that becomes simpler.

## Root object

```ts
interface SidecarStatusViewModel {
  kind: "sidecar.status.view";
  version: 1;
  binding: SidecarBindingView;
  repository: SidecarRepositoryView;
  discovery: SidecarDiscoveryView;
  actions: SidecarPaneAction[];
  diagnostics: SidecarPaneDiagnostics;
}
```

## Binding

`binding` describes the local project binding and sidecar paths.

Source: `exo sidecar status --format json`.

```ts
interface SidecarBindingView {
  state: "linked" | "unlinked" | "unknown";
  ok: boolean | null;
  linked: boolean;
  projectId: string | null;
  policy: string | null;
  sidecarKey: string | null;
  sidecarRoot: string | null;
  autoCommit: boolean | null;
  autoPush: string | null;
  paths: {
    manifest: string | null;
    projectionDir: string | null;
    db: string | null;
    runtime: string | null;
  };
}
```

Mapping:

| View field            | Source field                                                    |
| --------------------- | --------------------------------------------------------------- |
| `state`               | `linked ? 'linked' : 'unlinked'`; `unknown` when source missing |
| `ok`                  | `ok`                                                            |
| `linked`              | `linked`                                                        |
| `projectId`           | `project_id`                                                    |
| `policy`              | `policy`                                                        |
| `sidecarKey`          | `sidecar_key`                                                   |
| `sidecarRoot`         | `sidecar_root`                                                  |
| `autoCommit`          | `auto_commit`                                                   |
| `autoPush`            | `auto_push`                                                     |
| `paths.manifest`      | `manifest_path`                                                 |
| `paths.projectionDir` | `projection_dir`                                                |
| `paths.db`            | `db_path`                                                       |
| `paths.runtime`       | `runtime_dir`                                                   |

## Repository

`repository` describes sidecar git repository health.

Primary source: `exo sidecar repo status --format json`.  
Fallback source: `exo status --format json` field `sidecar_sync`.

```ts
interface SidecarRepositoryView {
  available: boolean;
  source: "sidecar.repo.status" | "exo.status.sidecar_sync" | "none";
  state:
    | "unavailable"
    | "clean"
    | "dirty"
    | "needs-remote"
    | "needs-push"
    | "behind"
    | "error";
  ok: boolean | null;
  sidecarRoot: string | null;
  branch: string | null;
  clean: boolean | null;
  hasRemote: boolean | null;
  remote: string | null;
  ahead: number | null;
  behind: number | null;
  issue: string | null;
  files: SidecarRepoFile[];
}

interface SidecarRepoFile {
  path: string;
  status: string;
}
```

Mapping:

| View field    | Primary source                           | Fallback source             |
| ------------- | ---------------------------------------- | --------------------------- |
| `available`   | source present                           | source present              |
| `source`      | `'sidecar.repo.status'`                  | `'exo.status.sidecar_sync'` |
| `ok`          | derive from fields or aggregate producer | `ok`                        |
| `sidecarRoot` | `sidecar_root`                           | `sidecar_root`              |
| `branch`      | `branch`                                 | `branch`                    |
| `clean`       | `clean`                                  | `clean`                     |
| `hasRemote`   | `has_remote`                             | `has_remote`                |
| `remote`      | `remote`                                 | `remote`                    |
| `ahead`       | `ahead`                                  | `ahead`                     |
| `behind`      | `behind`                                 | `behind`                    |
| `issue`       | aggregate producer or null               | `issue`                     |
| `files`       | `files`                                  | `[]`                        |

State derivation:

1. No source: `unavailable`
2. `clean === false`: `dirty`
3. `hasRemote === false`: `needs-remote`
4. `behind > 0`: `behind`
5. `clean === true && hasRemote === true && ok === false`: `needs-push`
6. `ok === true` or `clean === true && hasRemote === true`: `clean`
7. Otherwise: `error`

## Discovery

`discovery` describes registry provenance and proposed sidecar configuration.

Primary source: `sidecar status.discovery` when present.  
Optional source: `exo sidecar discover --format json` when the pane explicitly wants advisory discovery for an unlinked or remote-less state.

```ts
interface SidecarDiscoveryView {
  state: "not-run" | "not-needed" | "available" | "failed";
  ok: boolean | null;
  repository: SidecarDiscoveryRepository | null;
  identity: SidecarDiscoveryIdentity | null;
  registry: SidecarDiscoveryRegistry | null;
  match: SidecarDiscoveryMatch | null;
  confidence: string | null;
  proposal: SidecarDiscoveryProposal | null;
  failure: SidecarDiscoveryFailure | null;
  checked: SidecarCheckedAttempt[];
  attemptIndex: number | null;
  sourceSummary: string | null;
}
```

State derivation:

- `discovery === null` and repository has remote: `not-needed`
- `discovery === null`: `not-run`
- `discovery.ok === true`: `available`
- `discovery.ok === false`: `failed`

Field mapping:

| View field                          | Source field                          |
| ----------------------------------- | ------------------------------------- |
| `repository`                        | `repository`                          |
| `identity`                          | `identity`                            |
| `registry.source`                   | `registry.source`                     |
| `registry.label`                    | `registry.label`                      |
| `registry.profileRepo`              | `registry.profile_repo`               |
| `registry.path`                     | `registry.path`                       |
| `registry.version`                  | `registry.version`                    |
| `match.kind`                        | `match.kind`                          |
| `match.key`                         | `match.key`                           |
| `confidence`                        | `confidence`                          |
| `proposal.autoPush`                 | `proposal.auto_push`                  |
| `proposal.wouldMutateConfig`        | `proposal.would_mutate_config`        |
| `proposal.requiresRemoteAcceptance` | `proposal.requires_remote_acceptance` |
| `failure`                           | `failure`                             |
| `checked`                           | `checked`                             |
| `attemptIndex`                      | `attempt_index`                       |
| `sourceSummary`                     | `source_summary`                      |

## Actions

`actions` is the normalized list of things the pane can offer.

Sources:

- `sidecar status.next_actions`
- `sidecar status.discovery.next_actions`
- derived state from binding/repository/discovery

```ts
interface SidecarPaneAction {
  kind:
    | "bootstrap"
    | "commit"
    | "configure-remote"
    | "push"
    | "inspect"
    | "repair";
  label: string;
  command: string;
  rationale: string | null;
  intent: string | null;
  confidence: number | null;
  source:
    | "sidecar.status.next_actions"
    | "sidecar.discovery.next_actions"
    | "derived";
}
```

Normalization rules:

- Unlinked project with discovery proposal: `bootstrap`, `exo sidecar bootstrap --discover`
- Dirty sidecar repo: `commit`, `exo sidecar repo commit --message "..."`
- No remote and discovery proposal remote: `configure-remote`, from `sidecar status.next_actions`
- No remote and no proposal: `configure-remote`, `exo sidecar repo remote --url <url>`
- Clean + remote + not fully synced: `push`, `exo sidecar repo push`
- Discovery failed: `inspect` or `repair`, from discovery `next_actions`

The pane should preserve structured action fields rather than flattening them into display text.

## Diagnostics

`diagnostics` captures source-level fetch/contract diagnostics.

Sources:

- `TraceCacheRootDiagnostic`
- source command errors
- known limitations from the inventory

```ts
interface SidecarPaneDiagnostics {
  sources: SidecarPaneSourceDiagnostic[];
  limitations: string[];
}

interface SidecarPaneSourceDiagnostic {
  id: string;
  status: "success" | "empty" | "error" | "unknown";
  message?: string;
  fetchedAt?: number;
}
```

Required limitations for the initial implementation:

- `sidecar repo status` is read-only at runtime but currently classified under a write command namespace.
- `discovery` is conditional and can be null.
- `exo status.sidecar_sync` lacks dirty file details.

## Nullability requirements

The view model must be renderable in all of these cases:

1. No project or command source is available.
2. Project exists but is not sidecar-linked.
3. Project is sidecar-linked but sidecar root is missing or not a git repo.
4. Project is sidecar-linked, sidecar repo is dirty.
5. Project is sidecar-linked, sidecar repo has no remote and discovery succeeds.
6. Project is sidecar-linked, sidecar repo has no remote and discovery fails.
7. Project is sidecar-linked, sidecar repo is clean with remote.
8. Source command fails; diagnostics should render instead of hiding the pane.

## Implementation guidance

The next task should add contract coverage that proves sample source payloads normalize into this view model. UI rendering should wait until that coverage exists.

Do not add a Plan sidebar view, TraceCache roots, or command actions in the contract task. Those belong to the UI and actions goals.
