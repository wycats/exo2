<!-- exo:10184 ulid:01kq09x2d9y6gc9eg27jatcgvf -->

# RFC 10184: Project / Workspace / Worktree: unbundling the conflated root

- **Supersedes**: RFC 0093, RFC 0163


## Summary

RFC 10184 defines exo's project/workspace split for the `exo Everywhere` epoch.

A **project** is the shared exo identity and state boundary. A **workspace** is the checkout the user is actively working in. Git worktrees are a git mechanism that creates multiple workspaces for one project.

This RFC carries forward the Local XDG directory taxonomy from RFC 10177, but relocates canonical state and runtime to the project state root instead of the current checkout. It also locks the project identity model used by the current implementation and defines persistence policy as the thing that decides who owns durable state: the repository, the local machine, or a personal portable sidecar.

## Status

This RFC is the Stage 1 successor to RFC 10177.

Implemented facts already present in the codebase:

- exo requires git.
- `project resolve` reports project identity, workspace root, state root, database path, runtime directory, socket path, and PID path.
- linked git worktrees share a project id, state root, database, runtime directory, socket, and daemon.
- linked git worktrees keep distinct workspace roots.
- workspace-active phase state is persisted in `workspace_active_phase_data`, keyed by canonical workspace root.
- repo project state lives in `.exo` beside the primary repository checkout.
- shadow project state lives in `$HOME/.exo/projects/<project-id>` when enabled by policy.
- repo policy reads and writes workspace SQL projections; shadow policy does not.
- sidecar policy uses an explicit `sidecar_key` to bind a local project to private portable personal state.
- sidecar policy stores portable SQL projections separately from local runtime materialization.
- `exo sidecar bootstrap`, `exo sidecar init`, `exo sidecar link`, `exo sidecar unlink`, `exo sidecar discover`, and `exo sidecar status` guide sidecar setup and binding.
- `exo sidecar repo status`, `exo sidecar repo commit`, `exo sidecar repo push`, and `exo sidecar repo sync` manage the sidecar git repository.
- `exo project move-root` retargets sidecar-backed project state after a checkout is moved.

Lifecycle actions not taken in this edit:

- RFC 10177 has not been marked superseded.

Those actions require explicit human approval.

## Motivation

The user-visible friction is direct: exo must be usable in the other git repositories where work happens, not only in exo2.

Three workflows force the split:

1. **External work repos**: exo state may be useful but must not be committed to the repository. The project needs a shadow state policy for machine-local private state and a sidecar state policy for private state that follows the user across machines.
2. **Git worktrees**: one repository can have multiple checked-out workspaces. They should share the same exo project state and daemon, while each workspace keeps its own active work focus.
3. **Personal portability**: a user may want high-fidelity exo context for a work repository without asking the team to adopt Exosuit artifacts. That context needs a portable home outside the work repository.

The old model used a single "root" axis for all of these concerns. That made the checkout, the database, the daemon socket, and the active phase all move together. They no longer can.

## Vocabulary

| Term                     | Meaning                                                                                                                                                                                                    |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **project**              | The shared exo identity and state boundary. One project has one project id, one state root, one SQLite database, one runtime directory, one daemon, and one dashboard.                                     |
| **workspace**            | The canonical checkout root where the user is working. This is the VS Code folder / git worktree currently issuing commands.                                                                               |
| **git common directory** | The canonical git directory returned by `git rev-parse --path-format=absolute --git-common-dir`. This is the project identity input.                                                                       |
| **state root**           | The directory that contains exo's project-local state and runtime subdirectories.                                                                                                                          |
| **repo policy**          | Team-owned project state. The canonical database and runtime live in the repository-adjacent state root, and selected state is projected to workspace SQL dumps for git.                                   |
| **shadow policy**        | Private machine-local project state. The canonical database and runtime live under the user's home directory, and workspace SQL dumps are not read or written by default.                                  |
| **sidecar policy**       | Private portable personal state. The local project binds to a user-named sidecar key; portable SQL projections live outside the work repository, while the live database and runtime remain machine-local. |
| **sidecar key**          | A user-chosen portable identifier such as `client-api`. It is the authority for cross-machine sidecar binding. Remote URLs may be recorded as hints, but they do not define identity.                      |
| **worktree**             | Git's term for an alternate checkout. exo treats a worktree as a workspace that belongs to the same project as other worktrees with the same git common directory.                                         |

## Design

### Git is required

exo projects are git-backed. If project resolution does not find a git repository, it fails with a friendly precondition error that points the user at `git init`.

Gitless operation is rejected. A git repository is the identity anchor, the worktree model, and the adoption boundary. Supporting arbitrary directories would require a second identity model and would make shadow state harder to reason about.

### Project identity

Project identity is derived from the canonical git common directory.

Algorithm:

1. Run `git rev-parse --path-format=absolute --git-common-dir` from the current working directory.
2. Canonicalize the returned path.
3. Hash the canonical path bytes with BLAKE3.
4. Use the first 8 hash bytes as 16 lowercase hexadecimal characters.

Formally:

```text
ProjectId = hex(blake3(abs_canonical_git_common_dir_bytes)[0..8])
```

Properties:

- The ID is stable across linked worktrees because they share the same git common directory.
- The ID is stable across branch changes and remote URL changes.
- The ID changes when the repository's canonical git common directory moves.
- The ID does not expose the repository path in filenames.

### Workspace identity

The workspace root is the canonical path returned by:

```text
git rev-parse --path-format=absolute --show-toplevel
```

For a normal checkout, this is the repository root. For a linked worktree, this is the linked worktree root. Multiple workspace roots may map to the same project.

Workspace roots are used for workspace-local pins and for commands that need to render or update files in the checked-out working tree.

### State policy

State location and projection behavior are policy decisions, not identity decisions. `repo`, `shadow`, and `sidecar` policies use the same local `ProjectId`; the policy decides the local state root and where durable SQL projections are imported from and exported to.

#### Repo policy

For a normal repository, repo state lives at:

```text
<primary-workspace>/.exo/
```

The current implementation computes this as the parent of the canonical git common directory plus `.exo`. For a standard repository with `.git` as the common directory, that resolves to the repository root's `.exo` directory. Linked worktrees share that same directory because their common directory is the primary repository's `.git` directory.

Repo policy is the only implemented policy that reads and writes `docs/agent-context/*.sql` as the workspace projection. This is the team-owned mode: the repository owns the serialized steering state.

#### Shadow policy

Shadow state is enabled by project policy in:

```text
$XDG_CONFIG_HOME/exo/projects.toml
```

falling back through `$HOME/.config/exo/projects.toml` when `XDG_CONFIG_HOME` is unset.

Policy shape:

```toml
[projects.<project-id>]
state = "shadow"
```

Equivalent accepted spellings are `shadow = true` and `state_root = "shadow"`.

When shadow policy is active, state lives at:

```text
$HOME/.exo/projects/<project-id>/
```

This keeps the worktree untouched while preserving the same project identity and worktree-sharing behavior.

Shadow policy is machine-local. It does not import from or export to `docs/agent-context/*.sql` by default, even when those files exist in the checkout. The SQLite database under `$HOME/.exo/projects/<project-id>/cache/exo.db` is the project state for that machine.

#### Sidecar policy

Sidecar policy is private portable personal state. It is distinct from repo policy because the state is not team-owned repository content, and distinct from shadow policy because the state is intended to move with the user instead of staying bound to one machine.

Sidecar binding is explicit. A user binds the local project id to a portable sidecar key in the local policy file:

```toml
[projects.<project-id>]
state = "sidecar"
sidecar_key = "client-api"
sidecar_root = "/absolute/path/to/exo/sidecars"
```

The local `ProjectId` remains the machine-local binding key. It is still derived from the canonical git common directory and is still the lookup key in `projects.toml`. The `sidecar_key` is the cross-machine identity. It must be explicit because the local `ProjectId` is path-derived and remote URLs are not stable identity.

Sidecar policy has two roots:

```text
# Portable projection root — user syncs or commits this outside the work repo.
{sidecar_root}/projects/{sidecar_key}/sidecar.toml
{sidecar_root}/projects/{sidecar_key}/agent-context/*.sql

# Local materialization root — never synced as portable state.
$HOME/.exo/sidecars/{sidecar_key}/cache/exo.db
$HOME/.exo/sidecars/{sidecar_key}/runtime/daemon.sock
$HOME/.exo/sidecars/{sidecar_key}/runtime/daemon.pid
```

The portable sidecar stores a deterministic SQL projection and a manifest. The live SQLite database, daemon socket, and PID file remain machine-local runtime materialization. Syncing a live SQLite database or Unix socket through a sidecar is not supported.

The sidecar manifest records the explicit key and the local project id that
currently owns the sidecar binding:

```toml
[sidecar]
key = "client-api"
project_id = "0123456789abcdef"
```

Remote-derived identity is rejected as authority. Remote URLs can change across forks, mirrors, protocol rewrites, and host moves. Discovery can use remote and registry information to suggest a sidecar configuration, but the selected sidecar key and the local policy binding remain explicit.

When both repo projections and sidecar projections exist, the selected policy wins. There is no implicit merge between team-owned repo state and personal sidecar state.

The command surface is:

```text
exo sidecar bootstrap [--key <sidecar-key>] [--root <sidecar-root>] [--discover] [--accept-discovered-remote]
                    [--no-git] [--registry-file <path>]
exo sidecar init [--key <sidecar-key>] [--root <sidecar-root>] [--git]
exo sidecar link --key <sidecar-key> --root <sidecar-root>
exo sidecar unlink
exo sidecar discover [--registry-file <path>]
exo sidecar status [--registry-file <path>]
exo sidecar repo status
exo sidecar repo commit --message <msg>
exo sidecar repo remote --url <url> [--remote <remote>] [--replace]
exo sidecar repo push [--remote <remote>] [--branch <branch>]
exo sidecar repo sync
exo project move-root --key <sidecar-key> --to <checkout-root> [--dry-run]
```

`sidecar bootstrap` is the ergonomic onboarding command. By default it derives the sidecar key from the workspace directory name, uses `$HOME/exo/sidecars` as the sidecar root, initializes the sidecar root as a git repository unless `--no-git` is supplied, and seeds the sidecar projection from existing repo `docs/agent-context/*.sql` files when the sidecar projection is empty. In a non-git directory, bootstrap reports the git precondition and points the user at `git init` before asking them to choose sidecar storage.

`sidecar init`, `sidecar link`, and `sidecar unlink` remain supported lower-level binding commands. `init` creates or reuses sidecar state for the current git repo, `link` binds an existing sidecar key/root to the local project policy, and `unlink` removes the local sidecar binding. New project onboarding should use `sidecar bootstrap` because it combines binding, projection seeding, default-root handling, and sidecar repo setup guidance.

`sidecar discover` is the read-only discovery command. It reports GitHub profile or registry-based sidecar proposals when available, labels `$HOME/exo/sidecars` as the default user sidecar root, and separates that default from sidecar roots that are already attached to existing local project policy entries. `sidecar bootstrap --discover` can apply a discovered proposal when the selected remote and root are safe to adopt.

`sidecar repo` is the sidecar repository management command. It operates on the resolved `sidecar_root`, never on the current work repository. `status` reports the sidecar git state. `commit --message <msg>` flushes the current SQL projection to the sidecar before staging and committing sidecar files; when the sidecar is clean it succeeds with no new commit. `remote --url <url>` configures a sidecar git remote, defaults to `origin`, and requires `--replace` before changing an existing remote URL. `push` pushes the current branch to an existing remote, defaulting to `origin`, and requires `--branch` when the sidecar repo is in detached HEAD. Repository creation on GitHub or another forge is outside this command; users configure remotes with normal git or a future adoption flow.

`project move-root` is the supported relocation command for sidecar-backed projects. It is used when the checkout moves and the path-derived local project id changes, while the portable sidecar key and state root should stay the same. The command verifies that the destination exists and is a non-bare git worktree, verifies that RFC 00001 can still be discovered when present, previews the change with `--dry-run`, and applies only when no active old/new workspace conflict or live write-owner conflict blocks the move.

When applied, `project move-root` retargets the local policy entry from the old project id to the destination project id, rewrites `sidecar.project_id` in the sidecar manifest, rewrites `workspace_active_phase` workspace roots, rewrites phase ownership workspace roots, and updates workspace owner ids for workspace-owned phase claims. It preserves the sidecar key, the sidecar project directory, and the sidecar database/state root.

The VS Code surface should expose the same model as a config pane: choose `repo`, `shadow`, or `sidecar`; enter the sidecar key; choose the sidecar root; inspect resolved paths; and intentionally unlink or rebind. The CLI owns the canonical operation. The config pane is an ergonomic editor over the same local policy file.

### Directory taxonomy

RFC 10177's Local XDG taxonomy survives, but its root changes from "the current checkout" to "the project state root" for state and runtime artifacts.

| Category                 | Location                                                                                                                                             | Git status                                                                      | Meaning                                              |
| ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------- |
| Config                   | repository files such as `exosuit.toml` and `.config/exo/hooks.toml`                                                                                 | checked in when appropriate                                                     | deterministic tool behavior                          |
| Canonical runtime store  | `{state_root}/cache/exo.db`                                                                                                                          | ignored / private                                                               | SQLite database used by commands and daemon          |
| Runtime                  | `{state_root}/runtime/daemon.sock`, `{state_root}/runtime/daemon.pid`                                                                                | ignored / private                                                               | socket and PID for the project daemon                |
| Git-friendly state dumps | `docs/agent-context/*.sql` in the workspace checkout for repo policy; `{sidecar_root}/projects/{sidecar_key}/agent-context/*.sql` for sidecar policy | checked in only when the repo owns its exo state; synced personally for sidecar | deterministic SQL export of selected canonical state |
| Documents                | `docs/rfcs/stage-*/*.md`, docs, specs                                                                                                                | checked in                                                                      | human-authored prose                                 |

`.cache/exo.db` and `.runtime/daemon.sock` are no longer the normative paths. The normative paths are under `state_root`: `cache/exo.db` and `runtime/daemon.sock`.

### Daemon boundary

There is one daemon per project state root.

The daemon socket path is:

```text
{state_root}/runtime/daemon.sock
```

The PID path is:

```text
{state_root}/runtime/daemon.pid
```

Consequences:

- linked worktrees share one daemon;
- shadow projects place daemon runtime under `$HOME/.exo/projects/<project-id>/runtime`;
- stale socket cleanup operates on the project runtime directory;
- the daemon handles requests with the workspace root that launched or connected to it, so workspace-scoped behavior remains possible.

### Database boundary

There is one SQLite database per project state root:

```text
{state_root}/cache/exo.db
```

Commands and machine-channel handlers resolve the project, then use `Project::db_path()` instead of joining a database path to the workspace root.

Legacy root-relative `.cache/exo.db` is retained only as a compatibility constant for fallback paths and tests. It is not the target path for resolved git-backed projects.

### Workspace-active phase

The active phase is no longer a single project-global fact.

A project may have multiple in-progress phases. Each workspace pins the phase that anchors its current work. The pin is stored in `workspace_active_phase_data`:

```text
workspace_root TEXT PRIMARY KEY
phase_id       INTEGER NOT NULL REFERENCES phases_data(id) ON DELETE CASCADE
updated_at     TEXT NOT NULL
```

The table is project-local runtime state. It is not part of the git-dumped SQL context.

Resolution order:

1. If the current workspace has a valid pin, use that phase.
2. Otherwise, fall back to the single global in-progress phase when exactly one exists.
3. Otherwise, report that no unambiguous active phase exists.

This preserves old single-workspace behavior while allowing linked worktrees to keep distinct active work.

### Tracked artifacts in a workspace

The project database is shared, but the current workspace still matters for files that are part of the checked-out repository.

RFC files, documentation, `exosuit.toml`, and `.config/exo/hooks.toml` are workspace files. SQL dumps are policy-controlled projections: repo policy writes them to `docs/agent-context/*.sql`; shadow policy does not read or write them by default; sidecar policy writes them to the private portable sidecar projection.

This means a linked worktree can share project state while still producing file diffs in the branch checked out in that worktree.

## Supersession of RFC 10177

RFC 10177 is correct about the directory taxonomy:

- config is deterministic tool configuration;
- state is persistent app-managed data;
- cache is regenerable binary/runtime store;
- runtime is socket/PID/session-local process data;
- documents remain files because their value is prose.

RFC 10177 is wrong about locality:

1. It treats the current checkout as the project root for cache and runtime.
2. It says each worktree gets its own runtime directory and daemon.
3. It rejects hash-keyed user-global paths as unnecessary.

This RFC supersedes those conclusions. Cache and runtime are project-scoped, worktrees share them, and shadow state uses a hash-keyed home-directory path when policy requests it.

## Relationship to other RFCs

- RFC 0097 remains the daemon lifecycle RFC. Its socket path must be interpreted as `{state_root}/runtime/daemon.sock`, not `{workspace}/.runtime/daemon.sock`.
- RFC 0125 remains the transport-agnostic machine-channel protocol RFC. Its socket transport must use the project daemon socket under `state_root`.
- RFC 10178 remains the git-friendly SQL dump RFC. Its `.cache/exo.db` references now mean the project database at `{state_root}/cache/exo.db`.
- RFC 10180 remains the state/document/config classification RFC. Its canonical SQLite rows live in the project database, not necessarily in a checkout-local `.cache` directory.

## Alternatives considered

### Gitless projects

Rejected. Git is the common denominator for the workflows exo serves. `git init` is cheap, gives exo a stable identity boundary, and avoids a second project identity system.

### Remote URL identity

Rejected. Remote URLs change when a repository is forked, mirrored, moved between hosts, or switched between SSH and HTTPS. Project identity should not change when a remote changes.

### Remote-derived sidecar binding

Rejected as the authority. A normalized remote URL can be a useful hint when suggesting a sidecar key or warning about a surprising binding, but the sidecar key must be explicit. Personal sidecar state is user-owned state, so the user chooses the binding.

### Workspace path identity

Rejected. It breaks linked worktrees by making each checkout a separate project.

### Per-worktree daemon

Rejected. Worktrees should be distinct active work surfaces over shared project state. A daemon per worktree recreates the old split-brain state boundary.

### User-global daemon registry as the primary path

Rejected. A global registry can be added later for observability, but the primary daemon socket should be derived directly from the resolved project state root.

## Implementation notes

Important implementation points:

- `ProjectResolver::resolve()` is the entry point for project identity.
- `ProjectId::from_git_common_dir()` implements the BLAKE3-derived ID.
- `Project::db_path()`, `Project::runtime_dir()`, `Project::socket_path()`, and `Project::pid_path()` define the canonical paths.
- `project resolve` exposes the resolved project and paths to humans and LM tools.
- `workspace_active_phase_data` stores per-workspace phase pins.
- daemon helpers derive `LocalRuntimePaths` from the resolved `Project`, not from the workspace path alone.
- sidecar policy extends project resolution with `sidecar_key`, portable projection path, and local materialization paths.
- sidecar repository management runs git commands from `Project::sidecar_root`, and `sidecar repo commit` explicitly calls the fallible `write_sql_dump_with_project_result(root, Some(&project))` before staging sidecar files.
- `project move-root` is the relocation path for sidecar-backed projects whose local project id changed because the checkout's git common directory moved.
- `project move-root --dry-run` reports policy, manifest, workspace-root, phase-ownership, write-owner, and RFC 00001 verification before applying the retarget.

## Open work

- Add a first-class adoption command and copy: `exo adopt` should make the state-policy decision explicit.
- Add broader ergonomic commands for changing policy instead of editing `projects.toml` directly. `project move-root` covers the sidecar relocation case.
- Add repository hosting creation for sidecar repos. `sidecar repo push` intentionally requires an existing remote.
- Add a VS Code config pane over the same policy file.
- Decide how dashboards present sibling workspaces and their active phases.
- Update stale RFC references that still say `{workspace}/.runtime/daemon.sock` or checkout-local `.cache/exo.db`.

## Success criteria

This RFC is successful when:

1. `project resolve` gives the same project id and daemon paths for primary and linked worktrees.
2. primary and linked worktrees can keep distinct active phase pins.
3. repo policy writes to the project `.exo` root, not legacy `.cache/exo.db` in each checkout.
4. shadow policy writes to `$HOME/.exo/projects/<project-id>` without creating repo project state.
5. the daemon socket lives under project `state_root/runtime`.
6. non-git directories fail with a friendly `git init` path.
7. sidecar policy binds a local project to an explicit portable sidecar key without writing Exosuit metadata into the work repository.
8. RFC 10177 is superseded after this RFC is promoted.
