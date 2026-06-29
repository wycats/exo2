<!-- exo:10187 ulid:01ks122q842ynvkx842pz00ndm -->

# RFC 10187: GitHub Profile Sidecar Discovery

## Summary

Exosuit sidecar bootstrap can create local sidecar-backed state, but it cannot discover where that sidecar state should live remotely. This RFC specifies GitHub profile sidecar discovery: a read-only discovery mechanism that derives recommended sidecar key/root/remote/auto-push settings from a user- or organization-owned profile registry.

## Motivation

Dogfooding sidecar support exposed a repeatable gap: `exo sidecar bootstrap` and `exo sidecar repo commit` can prepare local state, but `exo sidecar status` can only report that the sidecar repository has no remote. Agents then need a human to provide the preferred registry, naming convention, and remote URL.

The missing capability is discovery, not automatic remote creation. Exosuit needs a stable, user-owned place to advertise sidecar remote configuration while preserving explicit local control.

## User Experience

The setup experience is: publish sidecar preferences once, then let every GitHub-backed workspace discover them.

### One-time profile setup

The user creates or updates the GitHub profile repository that represents the registry owner:

- User account: `wycats/wycats`
- Organization account: `my-org/.github`

Then the user adds a public, machine-readable registry file:

```text
.exosuit/sidecars.toml
```

The minimal registry establishes defaults:

```toml
version = 1

[defaults]
root = "~/.exo/sidecars"
remote_template = "git@github.com:{owner}/{repo}-exosuit-state.git"
auto_push = "if_remote"
```

Users can add exact project overrides when a repository needs a stable key or a non-template remote:

```toml
[projects."github.com/wycats/locald"]
key = "locald"
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"
```

Users can also define owner-wide conventions:

```toml
[owners."wycats"]
remote_template = "git@github.com:wycats/{repo}-exosuit-state.git"
```

This file is configuration, not a secret store and not an executable hook. It should be safe to keep public by default.

### Discovering from a repository

In any repository with a GitHub remote, a human or agent can run:

```sh
exo sidecar discover
```

The intended profile fetch contract makes this bare command sufficient: discovery probes the authenticated user's GitHub profile registry first, then the remote owner's profile registry, and reports every checked source.

The current implementation slice supports the same registry schema and report shape through an explicit local registry file while network-backed profile fetching is completed:

```sh
exo sidecar discover --registry-file .exosuit/sidecars.toml
```

In the current local-file slice, bare `exo sidecar discover` returns a `registry-not-found` discovery report that points at the expected profile registry location. The next phase replaces that placeholder with the profile fetch contract specified below.

The command reports:

- detected repository remote,
- identity source (`authenticated-user`, `remote-owner-user`, `remote-owner-organization`, `remote-owner-unknown`, or `unavailable`),
- every registry source checked and the loaded registry, when one succeeds,
- match kind (`exact-project`, `owner-template`, `defaults`, `none`, or `error`),
- proposed sidecar key/root/remote/auto-push,
- confidence level,
- and next actions.

Example successful discovery:

```text
Discovered sidecar configuration

Repository:
	github.com/wycats/locald

Registry:
	github.com/wycats/wycats:.exosuit/sidecars.toml
	Source: authenticated user profile
	Match: exact project
	Confidence: high

Proposed sidecar:
	key: locald
	root: ~/.exo/sidecars
	remote: git@github.com:wycats/locald-exosuit-state.git
	auto-push: if_remote

Next:
	exo sidecar bootstrap --discover
```

### Applying discovery

Discovery is advisory until the user explicitly applies it.

The normal application path is:

```sh
exo sidecar bootstrap --discover
```

For the current local-file implementation slice, pass the registry explicitly:

```sh
exo sidecar bootstrap --discover --registry-file .exosuit/sidecars.toml
```

Bootstrap acceptance writes local sidecar configuration and, when remote acceptance is satisfied, configures the sidecar git remote in the local sidecar repository. It does not create a GitHub repository. It is not merely permission to display the proposal.

Exact project matches can apply discovered key/root/auto-push configuration and configure the discovered remote because the registry explicitly names the repository. Template/default matches can apply key/root/auto-push configuration, but configuring the rendered remote requires explicit remote acceptance:

```sh
exo sidecar bootstrap --discover --accept-discovered-remote
```

If a local sidecar binding already exists, it wins over discovered key/root values. Discovery may still provide missing remote guidance, but bootstrap must not rewrite the existing binding.

If the sidecar repository already has a remote, that remote wins. `--accept-discovered-remote` means “configure the discovered remote when the sidecar repo has no remote”; it does not mean “replace an existing remote.” Replacement requires a future explicit replace command outside this RFC.

If the discovered remote does not exist on GitHub, `exo` reports that fact and gives the user the next manual action.

### Ongoing status

After bootstrap, `exo sidecar status` becomes discovery-aware. If the sidecar has no remote but discovery finds a candidate, status renders the source and concrete next actions:

```text
Sidecar linked: locald
Sidecar repo has no remote.

Discovered candidate:
	Source: github.com/wycats/wycats:.exosuit/sidecars.toml
	Remote: git@github.com:wycats/locald-exosuit-state.git

Next:
	exo sidecar repo remote --url git@github.com:wycats/locald-exosuit-state.git
	exo sidecar repo push
```

Once the remote exists, discovery hints disappear and status returns to normal remote health reporting:

```text
Sidecar linked: locald
Remote: git@github.com:wycats/locald-exosuit-state.git
Sync: clean
```

### Agent flow

Agents should not ask “where should sidecar state live?” before trying discovery. The intended loop is:

1. Run `exo status`.
2. If sidecar state has no remote, run `exo sidecar discover`.
3. If confidence is high and the next action is explicit, follow it.
4. If confidence is low, no registry exists, or applying a remote requires acceptance, ask the user with the exact missing decision.

### Failure states

Discovery degrades to actionable guidance:

| Situation             | User experience                                                                             |
| --------------------- | ------------------------------------------------------------------------------------------- |
| No GitHub remote      | Show manual workspace Git remote setup, such as `git remote add origin <github-url>`.       |
| No authenticated user | Try the remote-owner profile read-only and explain authentication would improve confidence. |
| No profile registry   | Show the expected `.exosuit/sidecars.toml` location and minimal schema.                     |
| Malformed registry    | Name the profile repo path and TOML parse error.                                            |
| Unsafe value          | Reject the specific field and explain the safety rule.                                      |
| Existing remote       | Do not replace it; show current remote and explicit replace path if supported.              |

## Design

Discovery starts from the workspace GitHub remote, extracts host/owner/repo, observes existing local sidecar state, and probes registry sources for a versioned registry file at `.exosuit/sidecars.toml`.

Local sidecar state is authoritative per field:

- An existing local sidecar binding controls key/root/auto-push. Discovery must not rewrite it.
- An existing sidecar git remote controls the sidecar remote. Discovery must not replace it in v1.
- If a binding exists but the sidecar repository has no remote, discovery may still propose a remote while leaving the binding intact.

Registry sources are probed in this order:

1. Explicit local file override from `--registry-file`. This is the current implementation slice and the diagnostics/test override. When supplied, no network profile probes run. The report source is `local-file`.
2. Authenticated user profile registry. If a GitHub identity is available, discovery fetches `LOGIN/LOGIN:.exosuit/sidecars.toml`. The report identity source is `authenticated-user`.
3. Remote-owner profile registry. Discovery falls back to the repository owner when the authenticated-user registry is unavailable or does not produce a proposal. If the remote owner is a user, discovery fetches `OWNER/OWNER:.exosuit/sidecars.toml`. If the remote owner is an organization, discovery fetches `OWNER/.github:.exosuit/sidecars.toml`. If the owner account type is unavailable, discovery tries `OWNER/.github` first and `OWNER/OWNER` second.

Every probe is recorded in the discovery report. Missing registries, unavailable authentication, fetch errors, and registries with no matching entry permit fallback to the next source. Registry parse errors and unsafe registry values are terminal for discovery because falling through would hide a broken authoritative registry.

The first valid registry source that produces an applicable proposal wins. If no source produces a proposal, discovery returns the best failure classification with the ordered probe list attached.

The registry is TOML, not executable code. A v1 registry can define defaults, exact project entries, and owner templates:

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

Resolution precedence:

1. Explicit command arguments for the current invocation.
2. Existing local project binding for key/root/auto-push.
3. Existing sidecar git remote for the remote URL.
4. Selected registry source, using exact project entries before owner templates before defaults.
5. Built-in local-only defaults.

Discovery never rewrites a local binding, and it never replaces an existing sidecar remote in v1. `--accept-discovered-remote` only authorizes configuring a discovered remote when the sidecar repository currently has no remote.

Org-owned repositories use the same probe order as user-owned repositories: authenticated user first, then remote owner. The remote-owner probe uses `OWNER/.github` when the owner is known to be an organization. This lets a developer's personal registry override org defaults while preserving org-wide configuration as the fallback.

## Command Surface

### `exo sidecar discover`

Pure/read-only command that returns a `SidecarDiscoveryReport` with:

- ordered source attempts and identity information,
- confidence and failure classification,
- proposed key/root/remote/auto-push values,
- whether applying the proposal would mutate local config,
- and suggested next actions.

### `exo sidecar bootstrap --discover`

Consumes a discovery report during bootstrap. Exact project matches can apply discovered local sidecar configuration and configure the discovered remote when no existing sidecar remote is present. Template/default matches can apply discovered local key/root/auto-push configuration; configuring their rendered remote requires `--accept-discovered-remote`.

### `exo sidecar status`

When sidecar state has no remote, status includes discovery-aware steering if discovery finds a candidate remote.

## Discovery Report Contract

All discovery surfaces share a single serializable report. The report is the contract between the CLI, machine channel, steering, and VS Code UI.

```json
{
  "kind": "sidecar.discovery",
  "ok": true,
  "repository": {
    "host": "github.com",
    "owner": "wycats",
    "repo": "locald",
    "remote": "git@github.com:wycats/locald.git"
  },
  "identity": {
    "source": "authenticated-user",
    "login": "wycats"
  },
  "checked": [
    {
      "source": "github-profile",
      "identity_source": "authenticated-user",
      "profile_repo": "wycats/wycats",
      "path": ".exosuit/sidecars.toml",
      "status": "loaded-match",
      "match": "exact-project"
    }
  ],
  "registry": {
    "source": "github-profile",
    "profile_repo": "wycats/wycats",
    "path": ".exosuit/sidecars.toml",
    "version": 1,
    "attempt_index": 0
  },
  "match": {
    "kind": "exact-project",
    "key": "github.com/wycats/locald"
  },
  "confidence": "high",
  "proposal": {
    "key": "locald",
    "root": "~/.exo/sidecars",
    "remote": "git@github.com:wycats/locald-exosuit-state.git",
    "auto_push": "if_remote",
    "would_mutate_config": true,
    "requires_remote_acceptance": false
  },
  "next_actions": [
    {
      "label": "Bootstrap from discovered sidecar config",
      "command": "exo sidecar bootstrap --discover",
      "intent": "act",
      "confidence": 0.95
    }
  ]
}
```

### Report fields

- `kind`: always `sidecar.discovery` for discovery reports.
- `ok`: `true` when discovery produced a valid report. Failed discovery still returns a report shape with `ok: false` and `failure`.
- `repository`: normalized GitHub remote being discovered for.
- `identity`: effective identity used for the selected registry source, when any.
- `checked`: ordered registry source attempts. This field is required because one scalar registry source cannot explain auth fallback, remote-owner fallback, or local-file override behavior.
- `registry`: registry source that produced the effective proposal. It is omitted when no registry loaded successfully or when local state alone explains the report.
- `match`: how local state or the selected registry produced the proposal.
- `confidence`: `high`, `medium`, `low`, or `none`.
- `proposal`: normalized sidecar configuration proposal. Missing fields are omitted rather than represented by empty strings.
- `next_actions`: machine-readable suggested commands, using the same intent/confidence vocabulary as steering.

### Checked attempts

Each checked registry source records the source, identity basis, concrete location, status, and any match or failure details:

```json
{
  "source": "github-organization-profile",
  "identity_source": "remote-owner-unknown",
  "profile_repo": "my-org/.github",
  "path": ".exosuit/sidecars.toml",
  "status": "not-found"
}
```

Attempt statuses are stable machine values:

| Status            | Meaning                                                                           | Fallback behavior                               |
| ----------------- | --------------------------------------------------------------------------------- | ----------------------------------------------- |
| `skipped`         | Source was not applicable, such as missing authentication.                        | Continue.                                       |
| `not-found`       | Registry path was absent.                                                         | Continue.                                       |
| `fetch-error`     | Registry could not be fetched because of transport or permission failure.         | Continue unless no later source can be checked. |
| `loaded-no-match` | Registry loaded and parsed but produced no applicable proposal.                   | Continue.                                       |
| `loaded-match`    | Registry loaded, parsed, and produced the selected proposal.                      | Stop; this source wins.                         |
| `parse-error`     | Registry TOML or version was invalid.                                             | Stop; report `registry-parse-error`.            |
| `unsafe-value`    | Registry contained an unsafe value, invalid URL, or unknown executable-ish field. | Stop; report `unsafe-registry-value`.           |

### Identity sources

| Source                      | Meaning                                            | Confidence impact                                                     |
| --------------------------- | -------------------------------------------------- | --------------------------------------------------------------------- |
| `authenticated-user`        | Authenticated GitHub user profile was used.        | Can be `high` when registry match is exact.                           |
| `remote-owner-user`         | Remote owner was identified as a user.             | Can be `medium` or `high` for exact public registry matches.          |
| `remote-owner-organization` | Remote owner was identified as an organization.    | Can be `medium` or `high` for exact public registry matches.          |
| `remote-owner-unknown`      | Remote owner account type could not be determined. | At most `medium`; report must say which profile repo probe succeeded. |
| `unavailable`               | No identity or owner candidate was available.      | `none`; no registry lookup occurs.                                    |

### Registry sources

| Source                        | Meaning                                                       |
| ----------------------------- | ------------------------------------------------------------- |
| `local-file`                  | Registry came from explicit `--registry-file` input.          |
| `github-profile`              | Registry came from a user profile repository (`owner/owner`). |
| `github-organization-profile` | Registry came from an organization `.github` repository.      |
| `none`                        | No registry was found.                                        |
| `error`                       | Registry lookup or parsing failed.                            |

### Match kinds

| Kind             | Meaning                                                              | Remote acceptance                                                                          |
| ---------------- | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `exact-project`  | `projects.<host/owner/repo>` matched the current repository.         | Remote may be accepted by `--discover` because the registry names this repository exactly. |
| `owner-template` | `owners.<owner>.remote_template` matched.                            | Requires `--accept-discovered-remote`.                                                     |
| `defaults`       | `defaults.remote_template` or `defaults.root` produced the proposal. | Requires `--accept-discovered-remote` when a remote is proposed.                           |
| `none`           | No registry entry produced a proposal.                               | No remote is proposed.                                                                     |
| `error`          | Registry exists but could not be used.                               | No mutation allowed.                                                                       |

### Application acceptance semantics

Bootstrap applies only the parts of a proposal that are not already controlled by local state:

| Discovery state                          | `exo sidecar bootstrap --discover` behavior                                        | Remote behavior                                                                                   |
| ---------------------------------------- | ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `exact-project` match, no local binding  | Configure discovered key/root/auto-push.                                           | Configure discovered remote when no sidecar remote exists.                                        |
| `owner-template` match, no local binding | Configure discovered key/root/auto-push.                                           | Do not configure rendered remote unless `--accept-discovered-remote` is present.                  |
| `defaults` match, no local binding       | Configure discovered key/root/auto-push.                                           | Do not configure rendered remote unless `--accept-discovered-remote` is present.                  |
| Existing local binding                   | Keep existing key/root/auto-push; do not rewrite local binding from discovery.     | May configure a discovered remote only when no sidecar remote exists and acceptance is satisfied. |
| Existing sidecar remote                  | Keep existing key/root/auto-push behavior.                                         | Keep existing remote; do not replace it.                                                          |
| Replacement requested                    | Refuse in v1 and show the existing remote plus the unsupported replacement intent. | No mutation.                                                                                      |

`--accept-discovered-remote` configures the sidecar git remote in the local sidecar repository when the sidecar repository has no remote. It is not a preview-only acknowledgement, and it is not permission to replace an existing remote.

### Failure classifications

Failed discovery returns `ok: false` with a stable classification:

```json
{
  "kind": "sidecar.discovery",
  "ok": false,
  "checked": [
    {
      "source": "github-profile",
      "identity_source": "authenticated-user",
      "profile_repo": "wycats/wycats",
      "path": ".exosuit/sidecars.toml",
      "status": "parse-error"
    }
  ],
  "failure": {
    "classification": "registry-parse-error",
    "message": "Failed to parse .exosuit/sidecars.toml",
    "source": "github.com/wycats/wycats:.exosuit/sidecars.toml"
  },
  "next_actions": [
    {
      "label": "Inspect sidecar registry",
      "command": "exo sidecar discover --verbose",
      "intent": "orient",
      "confidence": 0.7
    }
  ]
}
```

Required classifications:

| Classification          | Meaning                                                                                                |
| ----------------------- | ------------------------------------------------------------------------------------------------------ |
| `no-github-remote`      | The workspace has no supported GitHub remote.                                                          |
| `identity-unavailable`  | No authenticated identity and no usable remote owner fallback.                                         |
| `registry-not-found`    | Profile registry file was absent.                                                                      |
| `registry-fetch-error`  | GitHub/profile lookup failed for transport or permission reasons.                                      |
| `registry-parse-error`  | Registry TOML was invalid or used an unsupported version.                                              |
| `unsafe-registry-value` | Registry contained a command, unsupported URL scheme, local filesystem remote, or invalid field value. |
| `no-matching-entry`     | Registry loaded successfully but produced no project/template/default proposal.                        |

### Human output rules

Human output must be a rendering of the report, not an independent code path. It must include:

1. repository identity,
2. checked registry sources and selected registry or failure source,
3. match kind and confidence,
4. proposed sidecar fields,
5. whether applying the proposal mutates config or requires remote acceptance,
6. next actions.

Unknown or failed discovery must never collapse into a raw network error. The report should explain what was checked and what the user or agent can do next.

### Machine-channel and VS Code expectations

Machine-channel responses for `sidecar discover`, discovery-aware `sidecar status`, and discovery-aware bootstrap preview must include the report under a stable `discovery` field. VS Code must render the `discovery` field directly rather than scraping human text. Sidebar quick actions must use the report's `next_actions` entries.

## Trust and Safety

- Parse TOML only.
- Reject command fields, shell fragments, unsupported URL schemes, and local filesystem remotes.
- Accept only `https://github.com/...` and `git@github.com:...` remotes in v1.
- Show registry source and confidence before applying discovered configuration.
- Do not create GitHub repositories in v1.
- Do not push to newly discovered remotes unless the normal sidecar push path or configured auto-push policy allows it.

## Registry Schema and Resolution Cases

### Schema version

`version = 1` is required. Missing `version`, non-integer `version`, or unsupported versions are `registry-parse-error` failures. Discovery must not try to infer a schema version from table shape.

### Registry document

The top-level registry document is:

```toml
version = 1

[defaults]
root = "~/.exo/sidecars"
remote_template = "git@github.com:{owner}/{repo}-exosuit-state.git"
auto_push = "if_remote"

[projects."github.com/wycats/locald"]
key = "locald"
root = "~/.exo/sidecars"
remote = "git@github.com:wycats/locald-exosuit-state.git"
auto_push = "if_remote"

[owners."wycats"]
root = "~/.exo/sidecars"
remote_template = "git@github.com:wycats/{repo}-exosuit-state.git"
auto_push = "if_remote"
```

All tables are optional except `version`. A registry with only `version = 1` is valid but produces `no-matching-entry` for every repository.

### Field contract

| Field             | Location                             | Type    | Semantics                                                                                         |
| ----------------- | ------------------------------------ | ------- | ------------------------------------------------------------------------------------------------- |
| `version`         | top level                            | integer | Registry schema version. Must be `1`.                                                             |
| `root`            | `defaults`, `projects.*`, `owners.*` | string  | Local sidecar root. `~` expands to the current user's home directory. Relative paths are invalid. |
| `remote_template` | `defaults`, `owners.*`               | string  | Remote URL template. May use `{host}`, `{owner}`, `{repo}`, and `{key}`.                          |
| `key`             | `projects.*`                         | string  | Exact sidecar key for this repository.                                                            |
| `remote`          | `projects.*`                         | string  | Exact sidecar remote URL. Templates are not expanded in `remote`.                                 |
| `auto_push`       | `defaults`, `projects.*`, `owners.*` | string  | One of `never`, `if_remote`, or `always`.                                                         |

Unknown fields are rejected with `unsafe-registry-value`. This keeps the registry small, auditable, and non-executable.

### Project keys

Project table keys use the normalized repository identity:

```text
{host}/{owner}/{repo}
```

For GitHub v1, `host` must be `github.com`. Owner and repo preserve GitHub casing in display, but matching is case-insensitive.

### Template expansion

Templates can use only these variables:

| Variable  | Value                                        |
| --------- | -------------------------------------------- |
| `{host}`  | Normalized Git host, currently `github.com`. |
| `{owner}` | Repository owner.                            |
| `{repo}`  | Repository name without `.git`.              |
| `{key}`   | Resolved sidecar key.                        |

Unknown template variables are `unsafe-registry-value`. Missing required variables are `registry-parse-error` because the registry cannot produce a complete proposal.

The resolved sidecar key defaults to the repository name when no exact project `key` is provided.

### URL safety

Accepted remote URL forms in v1:

```text
git@github.com:<owner>/<repo>.git
https://github.com/<owner>/<repo>.git
https://github.com/<owner>/<repo>
```

Rejected values:

- local paths (`../state.git`, `/tmp/state.git`, `file://...`),
- non-GitHub hosts,
- shell fragments or command-like values,
- scp-like URLs for non-GitHub hosts,
- URLs with unsupported schemes.

### Resolution cases

Discovery resolves local state and the selected registry in this order:

| Case                           | Inputs                                                                                  | Output                                                                                                      | Confidence |
| ------------------------------ | --------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | ---------- |
| Explicit local binding exists  | Project already has sidecar key/root in local config.                                   | Report existing binding and skip registry override.                                                         | `high`     |
| Existing sidecar remote exists | Sidecar repo already has a remote.                                                      | Report current remote; no discovered remote proposal.                                                       | `high`     |
| Exact project match            | `projects."host/owner/repo"` exists.                                                    | Use project `key`, `root`, `remote`, and `auto_push`, falling back to defaults for omitted optional fields. | `high`     |
| Owner template match           | `owners."owner"` exists and no exact project match exists.                              | Use owner `remote_template`, `root`, and `auto_push`, falling back to defaults.                             | `medium`   |
| Defaults match                 | `defaults.remote_template` or `defaults.root` exists and no project/owner match exists. | Use default root and optional rendered remote template.                                                     | `low`      |
| Registry exists with no match  | Registry has no usable defaults/project/owner entry.                                    | `ok: false`, `no-matching-entry`.                                                                           | `none`     |
| No registry                    | Profile file missing.                                                                   | `ok: false`, `registry-not-found`.                                                                          | `none`     |
| Invalid registry               | Parse, version, field, template, or URL safety failure.                                 | `ok: false` with stable failure classification.                                                             | `none`     |

Existing local binding and existing sidecar remote are evaluated per field, not as a single early return. A project with an existing local binding and no sidecar remote keeps its local binding while still using discovery to propose a remote. A project with an existing sidecar remote reports that remote and suppresses discovered remote proposals.

### Precedence examples

Exact project entry wins over owner/default templates:

```toml
version = 1

[defaults]
remote_template = "git@github.com:{owner}/{repo}-state.git"

[owners."wycats"]
remote_template = "git@github.com:wycats/{repo}-exo.git"

[projects."github.com/wycats/locald"]
remote = "git@github.com:wycats/locald-exosuit-state.git"
```

For `github.com/wycats/locald`, discovery proposes `git@github.com:wycats/locald-exosuit-state.git`.

Owner template wins over defaults when there is no exact project entry:

```toml
version = 1

[defaults]
remote_template = "git@github.com:{owner}/{repo}-state.git"

[owners."wycats"]
remote_template = "git@github.com:wycats/{repo}-exo.git"
```

For `github.com/wycats/locald`, discovery proposes `git@github.com:wycats/locald-exo.git`.

Defaults produce a low-confidence proposal when no project or owner entry exists:

```toml
version = 1

[defaults]
root = "~/.exo/sidecars"
remote_template = "git@github.com:{owner}/{repo}-exosuit-state.git"
auto_push = "if_remote"
```

For `github.com/wycats/locald`, discovery proposes key `locald`, root `~/.exo/sidecars`, and remote `git@github.com:wycats/locald-exosuit-state.git`.

### Required test fixtures

The resolver must have fixtures for:

1. exact project match,
2. owner template fallback,
3. defaults fallback,
4. no registry,
5. registry with no matching entry,
6. malformed TOML,
7. unsupported schema version,
8. unsafe remote URL,
9. unknown template variable,
10. explicit local binding overriding registry values,
11. existing local binding plus missing sidecar remote preserving the binding while proposing a remote,
12. existing sidecar remote suppressing discovered remote proposals,
13. `--registry-file` using `local-file` and suppressing network profile probes,
14. authenticated-user registry winning before remote-owner registry,
15. authenticated-user miss falling back to remote-owner registry,
16. unknown remote-owner account type trying `OWNER/.github` before `OWNER/OWNER`,
17. parse and unsafe-value failures stopping fallback while not-found/no-match/fetch-error continue.

## Locald Dogfood Success Criteria

The GitHub profile fetch phase is complete when the locald repository can use this flow without an explicit registry file:

1. `exo sidecar discover` detects the GitHub remote for `github.com/wycats/locald`.
2. Discovery fetches the authenticated user's profile registry from GitHub without `--registry-file`.
3. The discovery report includes ordered `checked` attempts and the selected registry source.
4. The locald registry entry produces a concrete key/root/remote/auto-push proposal.
5. `exo sidecar bootstrap --discover` applies the discovered local sidecar configuration.
6. Remote acceptance configures the sidecar git remote without replacing any existing remote.
7. `exo sidecar status` surfaces discovery guidance while the sidecar has no remote or has not been pushed.
8. `exo sidecar repo push` pushes the sidecar repository to the configured remote.
9. Final `exo sidecar status` reports a clean remote-backed sidecar and no longer shows discovery hints.
10. The full flow emits machine-channel discovery data that VS Code can render without scraping human text.

## Implementation Plan

Size: L

Critical path:

1. Define the discovery report contract and registry schema.
2. Implement GitHub remote parsing and profile repository resolution.
3. Implement registry fetch abstraction with `local-file`, authenticated-user profile, and remote-owner profile sources.
4. Implement TOML resolver behind testable traits.
5. Add `exo sidecar discover` with ordered `checked` attempts in the report.
6. Wire discovery into `sidecar status` and bootstrap flags.
7. Extend machine-channel and VS Code surfaces to render discovery source and suggested actions.
8. Add fixtures for registry resolution, fetch-source fallback, local binding precedence, existing remote refusal, and terminal registry failures.
9. Dogfood the flow in locald and record the final clean status.

## Alternatives

- Require users to pass `exo sidecar repo remote --url ...` manually every time. This keeps the system simple but fails the dogfood goal of agent-recoverable sidecar publishing.
- Store discovery configuration in each repository. This duplicates policy and does not help fresh clones before project-local state exists.
- Auto-create GitHub repositories from templates. This is too much authority for v1 and should remain a later explicit workflow.

## Unresolved Questions

- Should Exosuit support a dedicated registry repository such as `OWNER/exosuit-sidecars` in addition to profile repositories?
- Should discovered registry metadata be cached locally with source and timestamp?

## Future Possibilities

- Additional discovery providers beyond GitHub.
- Explicit repository creation flow once GitHub authorization and consent UX are designed.
- VS Code quick actions for applying a discovered remote.
