# GitHub Profile Sidecar Discovery Inventory

**Audit Date**: 2026-05-19 \
**Scope**: Map every existing surface that touches sidecar configuration so the upcoming GitHub profile discovery implementation can add the right hooks without creating dead zones.

## Grounding references

- Design intent lives in [/docs/design/github-profile-sidecar-discovery.md](/docs/design/github-profile-sidecar-discovery.md).
- The current CLI sidecar namespace is implemented in [/tools/exo/src/command/sidecar.rs](/tools/exo/src/command/sidecar.rs).
- Project-level sidecar policy resolution is centralized in [/tools/exo/src/project.rs](/tools/exo/src/project.rs).
- Runtime surfaces (status, steering, machine channel) flow through [/tools/exo/src/world_state.rs](/tools/exo/src/world_state.rs), [/tools/exo/src/status.rs](/tools/exo/src/status.rs), [/tools/exo/src/steering.rs](/tools/exo/src/steering.rs), and [/tools/exo/src/main.rs](/tools/exo/src/main.rs).
- VS Code consumes CLI metadata via [/packages/exosuit-vscode/src/command-spec.json](/packages/exosuit-vscode/src/command-spec.json), machine-channel harnesses such as [/packages/exosuit-vscode/src/test/suite/MachineChannelContract.test.ts](/packages/exosuit-vscode/src/test/suite/MachineChannelContract.test.ts), and recovery UI plumbing in [/packages/exosuit-vscode/src/services/sidebarRecoveryRendering.test.ts](/packages/exosuit-vscode/src/services/sidebarRecoveryRendering.test.ts).

## Summary

- No discovery command or report exists today; every `exo sidecar ...` operation assumes the binding already exists or must be provided explicitly.
- Project policy resolution only reads local config/sidecar state; there is no extension point to consult registry metadata before falling back to defaults.
- Runtime status/steering understands when a sidecar remote is missing, but it can only suggest manual `exo sidecar repo remote --url ...` commands because no discovery metadata is available.
- VS Code mirrors the CLI surfaces exactly (command spec, machine channel tests, sidebar recovery messaging) and therefore has nowhere to display discovery provenance or recommendations.

## CLI and API entry points

### Command namespace

- `SidecarCommands` enumerates bootstrap/init/link/status/repo/unlink, and the handlers never attempt network discovery or registry lookups before acting. Introducing discovery requires:
  - Adding a new `discover` command with read-only output (referenced in the design doc) that can share parsing/report structs with other commands.
  - Teaching `bootstrap` and `status` handlers to call the discovery module when the repo lacks a remote so they can show next-actions grounded in discovered data instead of static steering.

### Project resolution

- `ProjectResolver::resolve_state_root()` only considers explicit sidecar bindings from `exo/projects.toml` plus repo/shadow defaults. Discovery should **not** mutate these rules, but it needs a staging area to attach proposed state (key/root/remote) before the user accepts it. The audit outcome:
  - Keep existing resolution untouched; discovery proposals should remain out-of-band until the user explicitly applies them (e.g., via `exo sidecar bootstrap --discover`).
  - Ensure the discovery module can serialize outputs in the same shape as `SidecarLinkOptions` so adoption inside bootstrap/status is mechanical.

### CLI dispatch and daemon routing

- `main.rs` treats `sidecar` commands as "direct" (bypassing the daemon) because they interact with local filesystem state. Discovery fits naturally here: it should be another direct command so that `exo sidecar discover` can run even when the daemon is offline.
- The JSON/machine server in `api/handler.rs` currently exposes only the existing subcommands. Once discovery is implemented we need to plumb the new command through the machine channel metadata (see VS Code section).

## Runtime status and steering surfaces

- `WorldState::probe()` already captures `SidecarRepoSyncStatus`, which `status.rs` and `steering.rs` use to recommend `exo sidecar repo remote` / `push` actions when the repo is remote-less or dirty. There is no knowledge of discovery sources, so the best it can do is say "add a remote manually".
- Hook plan:
  1. Extend the world snapshot to include the latest discovery report (even if it failed), so status/machine outputs can mention the provider and confidence.
  2. Update steering to prefer actions like `exo sidecar repo remote --url <discovered url>` with rationale that cites the profile registry.
  3. Preserve existing remote health logic so that once a remote is configured the discovery hints disappear.

## VS Code surfaces

- `command-spec.json` mirrors the CLI namespace; adding `sidecar discover` (and new flags on existing commands) is required for UI command pickers and LM tool metadata.
- `MachineChannelContract.test.ts` asserts the projection kinds returned by `context.paths`; when discovery becomes part of status output, machine-channel JSON must surface the same report so hosts can render it without scraping human text.
- `sidebarRecoveryRendering.test.ts` (and related providers) only show remote-less remediation as "Sidecar repo has no remote" with manual commands. Once discovery data exists, sidebar messaging should include the discovered source and a quick action to apply it.

## Hook insertion map

| Surface                                                                                                                                                                       | Current behavior                                                              | Discovery hook requirement                                                                                                        |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| CLI sidecar commands ([/tools/exo/src/command/sidecar.rs](/tools/exo/src/command/sidecar.rs))                                                                                 | Explicit bootstrap/init/link/status/repo/unlink with no upstream lookup.      | Add `discover` command and wire bootstrap/status to consume discovery reports when no remote or key is configured.                |
| Project policy resolver ([/tools/exo/src/project.rs](/tools/exo/src/project.rs))                                                                                              | Resolves only local bindings and defaults.                                    | Leave untouched; consume discovery results as optional overrides that must be explicitly applied.                                 |
| Runtime snapshots ([/tools/exo/src/world_state.rs](/tools/exo/src/world_state.rs)) and steering ([/tools/exo/src/steering.rs](/tools/exo/src/steering.rs))                    | Know only about local sidecar sync state.                                     | Capture discovery report metadata so `exo status` and machine-channel outputs can cite the registry and suggest concrete actions. |
| CLI dispatch ([/tools/exo/src/main.rs](/tools/exo/src/main.rs))                                                                                                               | Routes sidecar commands directly; no discovery-specific logic.                | Ensure the new `discover` command is marked "direct" and can run without daemon/bootstrap state.                                  |
| VS Code command metadata ([/packages/exosuit-vscode/src/command-spec.json](/packages/exosuit-vscode/src/command-spec.json))                                                   | Enumerates current sidecar subcommands only.                                  | Regenerate spec after the CLI gains discovery so the extension exposes the command.                                               |
| Machine channel contract ([/packages/exosuit-vscode/src/test/suite/MachineChannelContract.test.ts](/packages/exosuit-vscode/src/test/suite/MachineChannelContract.test.ts))   | Verifies projection metadata but has no discovery fields.                     | Extend the JSON contract and tests so discovery output is available to the extension without parsing human text.                  |
| Sidebar recovery rendering ([/packages/exosuit-vscode/src/services/sidebarRecoveryRendering.test.ts](/packages/exosuit-vscode/src/services/sidebarRecoveryRendering.test.ts)) | When the sidecar repo lacks a remote it only suggests manual remote commands. | Update rendering to surface discovered remote suggestions with provenance, matching the new steering payload.                     |

## Implementation implications

1. Build a standalone discovery module (remote parser, identity resolver, profile fetcher, registry parser) so both CLI and hosts can reuse it.
2. Ensure discovery reports are serializable and cached just enough to avoid redundant network calls during status refresh loops.
3. Defer any automatic mutation: all existing write paths (`repo remote`, `bootstrap`, `status` next-actions) must keep explicit `--accept` switches so discovery remains advisory until the user confirms it.
4. Extend machine-channel schemas and VS Code rendering before wiring CLI behavior so development hosts can visualize the new data the moment it lands.

## Locald dogfood result — 2026-05-21

The profile-fetch slice now has a durable locald registry fixture at [/tools/exo/tests/fixtures/github_profile/wycats/.exosuit/sidecars.toml](/tools/exo/tests/fixtures/github_profile/wycats/.exosuit/sidecars.toml). The fixture models the authenticated `wycats` GitHub profile registry and contains an exact `github.com/wycats/locald` entry with:

- key: `locald`
- root: `~/.exo/sidecars`
- remote: `git@github.com:wycats/locald-exosuit-state.git`
- auto-push: `if_remote`

### Discover flow

The isolated `locald` discover dogfood passed. Running bare `exo --format json sidecar discover` from a temporary repo with origin `git@github.com:wycats/locald.git`, isolated `HOME`/`XDG_CONFIG_HOME`, and fake `gh` backed by the fixture produced:

- `kind = "sidecar.discovery"`
- `ok = true`
- repository: `github.com/wycats/locald`
- identity: `authenticated-user` / `wycats`
- registry source: `github-profile`
- registry location: `github.com/wycats/wycats:.exosuit/sidecars.toml`
- match: exact project `github.com/wycats/locald`
- confidence: high
- proposal: key `locald`, root `~/.exo/sidecars`, remote `git@github.com:wycats/locald-exosuit-state.git`, auto-push `if_remote`

The human discover output also exposed the concrete registry location. Discover remained pure: it did not write isolated project config or home `.exo` state.

### Bootstrap/status flow

The isolated `locald` bootstrap/status dogfood did not satisfy the full RFC success criteria. Bootstrap fetched the profile registry and applied the `locald` binding, but the result exposed two coupled implementation blockers:

1. The discovered root `~/.exo/sidecars` is written and used literally instead of being expanded relative to `HOME`.
2. Because the literal path is created under the temporary workspace repo, git operations inside that path inherit the workspace repo's `origin`. Status then reports `has_remote = true`, suppresses discovery guidance, and treats the workspace origin as if it were the sidecar repo remote.

Observed bootstrap/status result:

- bootstrap `sidecar_key = "locald"`
- bootstrap `sidecar_root = "~/.exo/sidecars"`
- bootstrap `has_remote = true`
- bootstrap `remote = "origin"`
- status `linked = true`
- status `discovery = null`
- status `next_actions = []`

The observed `origin` is not the discovered sidecar-state remote `git@github.com:wycats/locald-exosuit-state.git`; it is inherited from the temporary workspace repo. This means the locald discover path is working, but the bootstrap/status success criteria remain unmet.

### Implementation evidence

- `SidecarBootstrap::execute()` passes discovered `proposal.root` through `PathBuf::from` before calling `init_sidecar`, preserving the literal `~` string.
- `init_sidecar()` accepts an explicit root unchanged.
- `ProjectResolver::sidecar_binding()` reads persisted `sidecar_root` as a raw `PathBuf`.
- `status_discovery()` suppresses discovery whenever sync status reports `has_remote`.
- `first_remote()` runs plain `git remote` in the sidecar root, so a sidecar root accidentally nested inside the workspace can inherit the workspace Git remote.

### Follow-up work

The next implementation pass needs to:

1. Expand discovered sidecar roots before writing project policy. `~` must resolve under `HOME`, not become a repo-relative directory named `~`.
2. Ensure sidecar git repo detection does not inherit the workspace Git repository. A sidecar root must be an independent sidecar git repository before remote status is trusted.
3. Apply or preserve discovered sidecar remotes according to the RFC bootstrap semantics. Exact project matches should not silently treat the workspace origin as a valid sidecar remote.
4. Re-run the locald bootstrap/status dogfood after those fixes and require final status to reflect the real sidecar-state remote semantics.
