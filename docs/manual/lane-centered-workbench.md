# Lane-Centered Workbench

Workbench lanes give durable identity to active streams of work. A lane records
why the stream exists, associates it with an Exo phase, and lets each workspace
choose which stream is currently in focus.

Lanes are project state. Every linked worktree sees the same lane collection,
while lane focus belongs to the individual workspace.

## Inspect Lanes

List the project's lanes:

```text
exo lane list
```

Show one lane and its associated phase:

```text
exo lane show <lane-id>
```

Show the lane focused in the current workspace:

```text
exo lane current
```

When the workspace has no focused lane, the machine-readable result contains
`lane: null`.

## Create a Lane

A lane begins in the `prepared` state and must belong to an existing pending or
in-progress phase:

```text
exo lane create "OAuth cleanup" \
  --intent "Remove the legacy token exchange without changing refresh behavior" \
  --phase oauth-hardening
```

Creation records durable intent. It does not start the lane or focus it.

## Focus and Resume Work

The lane's phase must be in progress before the lane can be focused:

```text
exo phase start oauth-hardening
exo lane focus <lane-id>
```

Focusing a lane atomically focuses its associated phase in the current
workspace. It does not take phase ownership and does not change the lane's
`prepared` or `executing` state.

A later CLI process, agent session, or editor can run `exo lane current` and
recover the same lane, intent, phase, and phase goals without relying on chat
history or a branch name.

## Start a Prepared Lane

Starting a lane transitions it from `prepared` to `executing` and focuses it:

```text
exo lane start <lane-id>
```

The associated phase must already be in progress. Starting a lane is a
phase-scoped mutation and therefore follows the phase ownership rules.

## Remove a Prepared Lane

Only a prepared lane can be removed:

```text
exo lane remove <lane-id>
```

Removal requires phase ownership. Exo clears workspace focus rows for that lane
and removes the portable lane in one transaction. An executing lane cannot be
removed through this command.

## Phase Interaction

Lane focus and active-phase focus remain consistent:

- Focusing or starting a lane focuses its in-progress phase.
- Focusing a different or pending phase clears a mismatched lane focus.
- Finishing a phase clears lane focus for every workspace whose focused lane
  belongs to that phase.
- Finishing a phase does not delete or close its lanes.
- A phase referenced by a lane cannot be removed until that reference is
  resolved.

## Linked Worktrees

Portable lane rows are shared project state. Two linked worktrees therefore see
the same lane IDs, titles, intents, states, and phase associations.

Focus is machine-local workspace state. Focusing a lane in one worktree does not
change the focused lane in another worktree. Workspace paths are not included in
portable SQL projections or public command output.

## VS Code

The **Work Lanes** tree is the first view in the Exosuit Run container. It shows
each lane's title, state, associated phase, and whether it is focused in the
current workspace.

Use the row's target action or the **Exosuit: Focus Work Lane** command to focus
a lane. Creation, starting, and removal remain available through `exo lane` and
`exo-run`.

VS Code does not keep a second focus value. The tree reads canonical Exo state
and refreshes after Exo commits a focus change.

## Current Boundary

The current workbench supports durable lane identity, `prepared` and
`executing` states, workspace-local focus, phase integration, linked-worktree
continuity, and the focus-oriented VS Code client.

It does not yet provide parking, closure, accepted outcomes, attachments to
branches or review artifacts, observation-backed status, validation provenance,
or a non-VS Code workbench UI.

This manual page codifies the behavior established by
[RFC 10202: Lane-Centered Workbench Adoption](../rfcs/stage-3/10202-lane-centered-workbench-adoption.md).
