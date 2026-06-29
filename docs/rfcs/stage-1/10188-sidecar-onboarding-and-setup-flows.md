<!-- exo:10188 ulid:01ksb15fajd66mwftscgx867x9 -->

# RFC 10188: Sidecar Onboarding and Setup Flows

## Summary

Sidecar setup needs an explicit onboarding model layered on top of GitHub Profile Sidecar Discovery. Discovery tells Exosuit where sidecar state should live when a registry exists; onboarding covers what to do when the registry, state repository, local binding, or user permissions are missing.

This RFC specifies persona-specific sidecar setup flows, setup-state terminology, approval requirements, and UI/CLI responsibilities for turning a local sidecar into portable shared state.

## Motivation

RFC 10187 defines GitHub profile sidecar discovery as a read-only discovery mechanism. Dogfooding the Plan sidebar Sidecar Status pane showed that discovery alone is not enough for first-time setup.

A project can be sidecar-linked locally while still lacking:

- a sidecar repository remote,
- a GitHub profile registry file,
- a registry entry for the current repository,
- a sidecar-state repository,
- or user permission to create/read any of the above.

Presenting these as raw failures such as `registry-not-found` is technically accurate but not the right user-facing model. The user-facing model is: portable sidecar setup is incomplete, and Exosuit can guide the user through the next safe action.

## Relationship to RFC 10187

RFC 10187 remains the discovery substrate. It answers: “Given a GitHub repository and available registries, what sidecar configuration is recommended?”

This RFC answers: “Given a user, project, permissions, and setup state, what should Exosuit ask the user to do next?”

This RFC does not replace RFC 10187. It extends the sidecar story with onboarding, setup, approval, and persona-specific flows.

Discovery recommends. Setup planning proposes mutations. Setup execution applies approved mutations.

## Authority Defaults

Canonical setup is owned by the repository owner by default.

- User-owned repository: default profile owner is the repository owner.
- Organization-owned repository: default profile owner is the organization, and setup must use the organization profile registry.
- Authenticated-user profile setup for a repository owned by someone else is a personal override and must be explicit.
- Explicit `--profile-owner` overrides the default authority.

State repositories are private by default. Setup must not infer public sidecar-state visibility from a public source repository.

## Personas

## Project Owner / Primary Maintainer

The project owner can create the canonical sidecar-state repository and profile registry entry.

Primary flow:

1. Exosuit detects local sidecar state with no remote and no usable registry entry.
2. The UI frames this as “Portable sidecar setup needed.”
3. Exosuit shows a setup plan.
4. The user approves.
5. `exo sidecar setup` creates or updates missing GitHub resources and configures the local sidecar remote.
6. The pane refreshes to a healthy or next-action state.

## Contributor Joining a Configured Project

A contributor should consume existing sidecar configuration, not create canonical infrastructure.

Primary flow:

1. Exosuit discovers an existing profile or organization registry entry.
2. The UI shows the discovered source and proposed sidecar configuration.
3. The user can bootstrap/link from the discovered configuration.
4. If access to the state repository is missing, Exosuit explains the access problem and offers an inspect/request-access path rather than creating a new canonical repository.

## Maintainer With Existing State Repository but Missing Registry

The sidecar repository exists, but discovery cannot find a registry entry.

Primary flow:

1. Exosuit detects or is told the state repository remote.
2. The setup plan only updates the registry entry and local remote when needed.
3. The UI should not imply the state repository will be created if it already exists.

## Organization-Owned Repository

Organization repositories introduce an authority choice.

Possible registry authorities:

- the authenticated user profile registry,
- the repository owner organization profile registry,
- a repository-owner user profile registry when the owner is a user.

The UI must show which authority is being used. If more than one authority is plausible, setup requires an explicit choice.

## AI Agent / Automation

Agents may diagnose setup state and generate setup plans, but they must not silently create GitHub resources.

Agent flow:

1. inspect setup state,
2. propose concrete changes,
3. ask for approval,
4. execute approved setup,
5. report exact mutations.

## CI / Headless

Headless flows require explicit flags and must not prompt.

Supported shape:

- `exo sidecar setup --dry-run`
- `exo sidecar setup --profile-owner <owner>`
- `exo sidecar setup --state-repo <repo>`
- `exo sidecar setup --remote-url <url>`
- `exo sidecar setup --replace-remote`

## Setup States

The Sidecar Status pane should present normalized setup states rather than raw low-level failures. These are stable setup-state values in the machine contract; UI text is a rendering of these states.

### `not-linked`

The project uses local/repo Exosuit state and has no sidecar binding.

Primary message: “This project is not using sidecar state.”
Primary action: initialize or bootstrap sidecar state.

### `linked-local`

The project has a local sidecar binding, but portability is incomplete.

Primary message: “Sidecar state is local only.”
Primary action: set up portable sidecar.

### `remote-missing`

The sidecar repository exists locally but has no git remote.

If discovery finds a concrete remote, the primary action is to add that remote.
If discovery does not find a remote because registry infrastructure is missing, the primary action is setup.

### `registry-missing`

No profile or organization registry file exists at the expected location.

Primary message: “No sidecar registry found.”
For owners, primary action: create sidecar registry entry.
For contributors, primary action: inspect/request setup from the owner.

### `registry-entry-missing`

A registry exists, but it does not contain an entry or template that matches the current repository.

Primary message: “Sidecar registry does not know this repository.”
Primary action for owners: add this repository to the registry.

### `state-repo-missing`

A registry entry or setup convention points to a sidecar-state repository that does not exist.

Primary message: “Sidecar state repository does not exist.”
Primary action for owners: create state repository.

### `access-missing`

The sidecar-state repository exists but the current user cannot access it.

Primary message: “Sidecar state repository exists, but access is missing.”
Primary action: inspect permissions/request access.

### `healthy`

The project is sidecar-linked, the sidecar repo has the expected remote, and sync state is known.

Primary message: “Sidecar state is portable.”
Primary actions are ordinary sync actions such as commit/push.

### `dirty` / `needs-push`

The setup is complete, but local sidecar state needs persistence or synchronization.

Primary actions: commit sidecar state, push sidecar state.

## Setup Plan Contract

The central implementation primitive is a pure machine-channel setup-plan operation. CLI dry-run is a projection of this same operation, not the architecture boundary.

The setup plan has this stable shape:

```ts
interface SidecarSetupPlan {
   kind: "sidecar.setup.plan";
   setupState: SidecarSetupState;
   authority: {
      kind: "user" | "organization" | "explicit";
      profileOwner: string;
      profileRepo: string;
      registryPath: ".exosuit/sidecars.toml";
   };
   repository: {
      host: "github.com";
      owner: string;
      repo: string;
      projectKey: string;
   };
   sidecar: {
      key: string;
      root: string | null;
      currentRemote: string | null;
      proposedRemote: string | null;
   };
   mutations: SidecarSetupMutation[];
   diagnostics: SidecarSetupDiagnostic[];
   approvalSummary: string;
   planDigest: string;
}

type SidecarSetupState =
   | "not-linked"
   | "linked-local"
   | "remote-missing"
   | "registry-missing"
   | "registry-entry-missing"
   | "state-repo-missing"
   | "access-missing"
   | "healthy"
   | "dirty"
   | "needs-push";

interface SidecarSetupMutation {
   kind:
      | "create-profile-repo"
      | "create-state-repo"
      | "upsert-registry-entry"
      | "configure-local-remote";
   status: "would-create" | "exists" | "would-update" | "unchanged" | "blocked";
   target: string;
   detail: string;
}
```

The plan operation is read-only. Setup execution must consume or revalidate the approved plan before mutation. Execution must refuse stale plans whose digest no longer matches the current resource state.

`exo sidecar setup --dry-run --format json` must faithfully render the same setup plan for CLI, CI, and debugging workflows.

## UI Requirements

The UI should show setup as a plan before mutation.

For owner setup, the approval dialog should list concrete resources:

- GitHub sidecar-state repository to create or reuse,
- profile or organization registry to create or update,
- registry entry key/remote/root,
- local sidecar remote change.

The UI must distinguish:

- creating a new remote repository,
- adding a registry entry,
- configuring a local remote,
- bootstrapping from an already discovered remote,
- requesting access.

The UI must not collapse all missing-resource cases into `registry-not-found`.

## CLI Requirements

`exo sidecar setup` is the CLI execution/projection of the setup-plan contract.

It must:

- support `--dry-run`,
- expose the same setup plan returned by the machine-channel setup-plan operation,
- require approval when invoked through the machine channel,
- be idempotent,
- avoid replacing existing remotes unless `--replace-remote` is present,
- include SHA when updating existing GitHub Contents API files,
- structurally upsert registry entries instead of appending duplicate TOML tables,
- skip GitHub writes when registry content is unchanged,
- report exactly which resources were created, updated, skipped, or configured.

## Approval Model

Setup is an exec-level operation because it can create remote GitHub resources and mutate local git configuration.

Approval is plan-bound. Required approval text must identify:

- profile owner,
- profile registry path,
- state repository name,
- remote URL,
- local sidecar root,
- whether any existing remote would be replaced.

The approval payload must include the plan digest and exact mutations. If the plan changes before execution, setup must request approval again.

## Non-Goals

- This RFC does not change the registry schema from RFC 10187.
- This RFC does not make discovery mutate state.
- This RFC does not grant repository access or manage GitHub permissions beyond creating/updating resources the authenticated user can already write.
- This RFC does not require the UI to implement access-request workflows immediately; it only requires that missing access be distinguished from missing resources.

## Implementation Plan

1. **M — Extract pure setup planner**
   - Create a shared setup planner that returns `SidecarSetupPlan`.
   - Use it from machine channel, CLI dry-run, and execution.

2. **M — Machine-channel setup-plan operation**
   - Add a pure sidecar setup-plan operation returning `SidecarSetupPlan`.
   - Add machine-channel contract coverage.

3. **S — Setup-state view model**
   - Add a normalized setup-state layer over binding/repository/discovery.
   - Preserve raw diagnostics for drill-in.

4. **M — CLI setup hardening**
   - Keep `exo sidecar setup` idempotent.
   - Ensure dry-run and machine-channel confirmation cover all mutation paths.
   - Add tests for existing registry, existing state repo, existing matching remote, conflicting remote, and missing permission.

5. **M — UI onboarding copy and approval preview**
   - Replace raw `registry-not-found` as the top-level message with persona-aware setup states.
   - Render `SidecarSetupPlan` before execution.

6. **M — Persona flow tests**
   - Owner happy path.
   - Contributor with registry and access.
   - Contributor without access.
   - Org-owned repo with org profile registry.
   - Existing local binding with missing remote.

7. **S — Documentation**
   - Document setup flows and examples in sidecar docs.

## Future Possibilities

- `exo sidecar setup --only registry|repo|remote` for partial repair flows.
- First-class access-request workflow for contributors who can discover but cannot read or write the state repository.
- Organization policy that configures sidecar setup defaults outside profile registries.

