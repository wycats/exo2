# GitHub profile sidecar discovery

## Problem

Sidecar bootstrap can create and seed portable Exosuit state, but it currently stops at the first network boundary: the sidecar git repository has no remote. Agents then need a human to tell them where sidecar state should live before they can publish or reuse it across workspaces.

The dogfood friction is concrete:

- `exo sidecar bootstrap` can make a sidecar-backed project locally.
- `exo sidecar repo commit` can save sidecar state locally.
- `exo sidecar status` and steering can say the sidecar has no remote.
- Nothing tells an agent the user's preferred sidecar registry, naming convention, or remote template.

The missing capability is **discovery**, not git mechanics. Exosuit needs a stable, user-owned place to advertise where sidecar state should live.

## Design goal

Use the authenticated GitHub identity as a stable discovery point for sidecar remote configuration.

If a workspace has a GitHub remote and the user has advertised an Exosuit sidecar registry from their GitHub profile/profile repository, `exo` should be able to derive:

- the sidecar key,
- the local sidecar root,
- the sidecar git remote URL,
- the auto-push policy,
- and the next action needed to publish or reuse sidecar state.

## Non-goals

- No GitHub-only state model. GitHub is one discovery provider, not the sidecar architecture.
- No silent remote creation in v1. Creating a GitHub repository requires explicit user authorization.
- No secret storage in profile files.
- No execution of registry-provided commands.
- No replacement for explicit CLI arguments. Explicit local choices always win.

## Discovery precedence

When sidecar bootstrap/status needs a remote recommendation, resolve configuration in this order:

1. Explicit command arguments: `--key`, `--root`, `--url`, `--remote`, `--branch`.
2. Existing local project binding in the Exosuit projects config.
3. Existing sidecar git remote in the sidecar root.
4. GitHub profile sidecar registry discovery.
5. Built-in default: local sidecar root only, with a next action to add a remote manually.

Discovery must never override an existing local binding or an existing sidecar remote unless the user passes an explicit replace flag.

## GitHub identity resolution

Discovery starts from the workspace git remote.

1. Parse `origin` or the selected repo remote.
2. Accept GitHub HTTPS and SSH remotes.
3. Extract:
   - host, such as `github.com`,
   - owner, such as `wycats`,
   - repo, such as `locald`.
4. Resolve the authenticated GitHub user when available:
   - VS Code extension path: GitHub authentication provider.
   - CLI path: `gh api user` when `gh` is installed and authenticated.
5. If authenticated identity is unavailable, fall back to the remote owner as a read-only registry lookup candidate.

The command output must say which identity source was used.

## Profile registry location

GitHub uses different profile repository names for users and organizations:

- User profile repositories are named after the user, for example `wycats/wycats`.
- Organization profile repositories are the organization's `.github` repository, for example `acme/.github`.

When discovery has an authenticated user identity, the user profile lookup uses `login/login`. When discovery falls back to a repository owner, it should use the owner account type when the GitHub API can provide it: user owners map to `owner/owner`, and organization owners map to `owner/.github`. If the owner account type is unavailable, discovery should try the organization profile repository first and then the user profile repository, reporting which source succeeded.

Exosuit looks for this file in the default branch:

```text
.exosuit/sidecars.toml
```

The file is machine-readable, versioned, and public by default. Private profile repositories can be supported later through authenticated GitHub API reads.

## Registry schema

```toml
version = 1

[defaults]
root = "~/.exo/sidecars"
remote_template = "git@github.com:{owner}/{repo}-exosuit-state.git"
auto_push = "if_remote"

[projects."github.com/wycats/locald"]
key = "locald"
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"

[owners."wycats"]
remote_template = "git@github.com:wycats/{repo}-exosuit-state.git"
```

### Fields

- `version`: schema version. v1 is required.
- `defaults.root`: local sidecar root template. `~` expands to the current user's home directory.
- `defaults.remote_template`: fallback remote URL template.
- `defaults.auto_push`: one of `never`, `if_remote`, or `always`.
- `projects.<host/owner/repo>.key`: exact sidecar key for a repository.
- `projects.<host/owner/repo>.remote`: exact sidecar remote URL.
- `owners.<owner>.remote_template`: owner-scoped remote template.

Template variables:

- `{host}`: Git host, such as `github.com`.
- `{owner}`: repository owner.
- `{repo}`: repository name.
- `{key}`: resolved sidecar key.

## Proposed command surface

### `exo sidecar discover`

Pure/read-only command that reports a proposed sidecar configuration.

It returns:

- discovered registry source,
- confidence level,
- selected project entry or template,
- proposed key/root/remote/auto-push,
- whether applying it would mutate local config,
- and suggested next actions.

### `exo sidecar bootstrap --discover`

Applies discovered local sidecar configuration during bootstrap.

Rules:

- Exact project match may add the sidecar git remote locally.
- Template match may suggest the remote by default; applying it requires `--accept-discovered-remote`.
- No GitHub repository is created automatically.
- No push happens unless normal sidecar auto-push policy permits it and a remote exists.

### `exo sidecar status`

If a sidecar has no remote, status should include discovery-aware steering:

```text
Sidecar repo has no remote.
Discovered candidate from github.com/wycats/wycats:.exosuit/sidecars.toml
  git@github.com:wycats/locald-exosuit-state.git

Next actions:
  → exo sidecar repo remote --url git@github.com:wycats/locald-exosuit-state.git
  → exo sidecar repo push
```

## Trust and safety

Profile registry data is configuration, not code.

Rules:

- Parse TOML only.
- Reject command fields, shell fragments, local filesystem remotes, and unsupported URL schemes.
- Accept only `https://github.com/...` and `git@github.com:...` remotes in v1.
- Show the registry source in human output before applying discovered remote configuration.
- Never replace an existing remote without explicit `--replace`.
- Never push to a newly discovered remote unless the normal explicit sidecar push path or configured auto-push policy allows it.

## Failure modes and steering

Discovery should degrade into actionable guidance, not raw network errors.

| Situation                          | Steering                                                                      |
| ---------------------------------- | ----------------------------------------------------------------------------- |
| No GitHub remote                   | Show manual `exo sidecar repo remote --url <url>` action.                     |
| No authenticated user              | Try remote owner profile as read-only; explain auth would improve confidence. |
| No profile registry                | Show sample `.exosuit/sidecars.toml` path and schema.                         |
| Registry parse error               | Name the profile repo path and TOML error.                                    |
| Template missing required variable | Name the template and missing variable.                                       |
| Remote already exists              | Do not change it; show current remote and `--replace` action.                 |

## Implementation sketch

Add a sidecar discovery module with narrow dependencies:

- `GitRemote`: parse and normalize GitHub remotes.
- `GithubIdentityProvider`: returns authenticated login when available.
- `ProfileRegistryFetcher`: fetches `.exosuit/sidecars.toml` from a profile repo.
- `SidecarRegistry`: parses schema and resolves project/template entries.
- `SidecarDiscoveryReport`: serializable output used by CLI, machine channel, and steering.

Keep network access behind traits so tests can use fixture registries without hitting GitHub.

## Acceptance criteria

- `exo sidecar discover --format json` returns a stable report for:
  - exact project registry match,
  - owner template match,
  - no registry,
  - malformed registry.
- `exo sidecar bootstrap --discover` preserves explicit CLI arguments over registry values.
- `exo sidecar status` suggests a discovered remote when the sidecar repo has no remote.
- Existing `exo sidecar repo remote --url ...` remains the explicit mutation path.
- Tests cover GitHub remote parsing for HTTPS and SSH forms.
- Tests cover refusal to execute or accept unsafe registry-provided values.

## Open questions

- Should org-owned repos prefer the authenticated user's profile registry or the org profile registry?
- Should Exosuit support a dedicated registry repo, such as `OWNER/exosuit-sidecars`, before profile-repo support?
- Should bootstrap create the remote GitHub repository when the GitHub CLI is authenticated, or should that remain a later explicit command?
- Should discovered registry metadata be cached in local Exosuit config with source and timestamp?
