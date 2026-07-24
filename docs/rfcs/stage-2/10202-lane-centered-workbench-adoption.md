<!-- exo:10202 ulid:01kxe1fzcamzgrk6hnqcbb6byd -->

# RFC 10202: Lane-Centered Workbench Adoption

- **Supersedes**: RFC 0056

**Feature**: lane-centered-workbench

## Summary

Exo will represent each durable stream of active work as a **workbench lane**.
A lane gives a stream a stable identity, records the intent that began it, and
connects it to the phase that supplies its planning and ownership boundary.
Each workspace may focus a different lane without changing the lanes visible
to the rest of the project.

This revision defines the first implementation contract. It deliberately proves
one narrow capability:

> An agent can create, focus, and resume a lane from Exo project state without
> relying on chat history.

The first proof adds portable lane identity, workspace-local focus, a shared
`exo lane` command surface, and a compact focus-oriented VS Code view. It does
not yet model attachments, observations, review state, validation provenance,
parking, closure, or outcomes. Those concepts remain part of the broader
[lane-centered workbench design package](../../design/lane-centered-workbench/README.md),
but they are not prerequisites for making lane continuity real.

## Motivation

Exo already has durable objects for planning and execution. Phases organize a
band of work, goals and tasks describe what must be accomplished, Git records
source history, and pull requests carry public review. What the project lacks
is a durable identity for the stream that joins those artifacts together.

Without that identity, users and agents reconstruct active work indirectly. A
branch name may hint at the topic. A worktree may reveal where files are
checked out. A conversation may remember why the work started. None of those
surfaces can reliably answer which intent the work serves or which stream this
workspace is currently advancing. The reconstruction becomes especially
fragile when several worktrees are active at once.

A lane addresses that gap without taking over the jobs of the existing
artifacts. It is not a branch, worktree, pull request, phase, validation lane,
or chat session. It is the durable subject to which those things may eventually
relate. The initial implementation keeps that subject intentionally small:
identity, intent, execution state, phase association, and workspace focus.

## Guide-Level Explanation

A lane begins in the `prepared` state and belongs to an existing phase:

```text
exo lane create "OAuth cleanup" \
  --intent "Remove the legacy token exchange without changing refresh behavior" \
  --phase oauth-hardening
```

Creation returns a stable lane ID. Titles are descriptive labels and need not
be unique; commands accept an exact ID or an unambiguous ID prefix.

Preparation does not start the phase. The user or agent starts the phase
through the existing phase workflow, preserving its ownership and sequencing
rules. Once the phase is in progress, the lane can begin executing:

```text
exo phase start oauth-hardening
exo lane start 01K...
```

Starting the lane changes its state from `prepared` to `executing` and focuses
it in the current workspace. Focusing a lane also focuses its associated phase,
so existing goal and task commands continue to operate against the planning
context the lane names.

Focus belongs to a workspace rather than to the project as a whole. Two linked
worktrees see the same portable lane records, but each may focus a different
lane:

```text
exo lane focus 01K...
exo lane current
```

A later process, editor session, or agent can recover the focused lane, its
intent, its state, its phase, and that phase's goals from Exo. No conversation
history or branch-name convention is involved.

## Reference-Level Explanation

### Lane identity and lifecycle

The first proof introduces one portable reactive table,
`workbench_lanes_data`. Each row has this logical shape:

| Field | Contract |
| --- | --- |
| `text_id` | Stable portable identity, unique within the project |
| `title` | Non-empty human-readable label; not required to be unique |
| `intent` | Non-empty account of what the stream is trying to accomplish |
| `state` | `prepared` or `executing` |
| `execution_phase_id` | Required foreign key to an existing phase |
| `created_at` | RFC 3339 creation time |
| `updated_at` | RFC 3339 time of the latest lane mutation |

The phase foreign key uses restrictive deletion. A phase with lanes cannot be
removed accidentally; `phase remove` reports an actionable precondition rather
than allowing SQLite to erase a durable work identity. Moving the phase within
the plan does not change the lane.

The two-state lifecycle is intentionally incomplete. `prepared` means the
stream has durable intent but has not begun execution. `executing` means work
has begun. A lane may remain executing while its associated phase moves through
review or completion because lane closure is not part of this proof. Later
work will add parking, repair, readiness, closure, and outcome semantics rather
than overloading either of these states.

No migration attempts to infer lanes from existing phases, branches, or
worktrees. Existing projects begin with zero lanes.

### Portable state and workspace focus

Lane rows are canonical project state. They participate in reactive revision
tracking and deterministic SQL dump and import. Dumps identify the associated
phase by its stable text ID rather than by SQLite row ID. The imported lane
therefore preserves identity without depending on one database's row numbers
or one checkout's path.

The focused lane is stored separately in
`workspace_lane_focus_data`. A focus row contains the normalized workspace
root, the focused lane, and its update time. It is reactive machine-local state:
it is never written to repository or sidecar SQL projections, and public
command output never exposes the workspace path.

This split permits every linked worktree to observe the same lane identities
while choosing its own focus. Removing or recreating one worktree cannot change
the focus of another.

### Relationship to phases

Phases remain the implemented planning, sequencing, and ownership authority.
The first lane model does not move goals or tasks into a new hierarchy. A lane
names one execution phase, and lane detail reads derive goal summaries from
that phase.

The two workspace focus relations obey one invariant:

> When a workspace has a focused lane, its active phase is the phase associated
> with that lane.

`lane focus` and `lane start` update both relations in one canonical SQLite
transaction. `phase focus` and `phase start` preserve an existing lane focus
when it belongs to the selected phase and clear it when it belongs to another
phase. Finishing a phase clears the current workspace's focus when it points
to a lane under that phase, but it does not delete or close the portable lane.
Supported commands therefore cannot publish a half-updated focus or leave a
completed phase presented as the current execution stream.

Reads do not silently repair inconsistent legacy or manually edited rows. They
return the observed lane and a stable `lane.phase_focus_mismatch` diagnostic,
allowing the caller to focus the intended lane or phase explicitly.

Creating or starting a lane is a phase-scoped mutation and uses the existing
phase-ownership check. A workspace may focus a lane without taking phase
ownership, matching the distinction already made by `phase focus`.

### Command surface

The public namespace is `exo lane`. Exohook continues to use *validation lane*
for check groupings in the separate `exohook` executable. Cross-system prose
uses *workbench lane* when the distinction is not otherwise clear.

The command surface is:

| Command | Effect | Behavior |
| --- | --- | --- |
| `lane create <title> --intent <text> --phase <id>` | Write | Create a prepared lane under a pending or in-progress phase |
| `lane list` | Pure | List portable lanes with phase and current-workspace focus summaries |
| `lane show <id>` | Pure | Show one lane, its phase, and phase goal summaries |
| `lane current` | Pure | Show the current workspace's focused lane, or `null` |
| `lane focus <id>` | Write | Require a pending or in-progress phase, then atomically focus the lane and phase in this workspace |
| `lane start <id>` | Write | Require an in-progress phase, transition the lane to executing, and focus it |

Pure commands use replayable-read recovery. The three writes mutate only
canonical SQLite state and use atomic-project-state recovery, so the mutation,
deterministic event, and replayable core response commit together. The normal
post-write path remains responsible for portable projection and sidecar
persistence.

All adapters are generated or dispatched from this CommandSpec surface. The
CLI, JSON machine channel, MCP `exo-run`, and VS Code extension do not maintain
parallel lane semantics.

### Machine representation

Lane summaries expose stable IDs, title, intent, state, timestamps, associated
phase ID/title/status, and `focused_here`. `lane show` and `lane current` also
include summaries of the associated phase's goals. These are the facts needed
to recognize the stream and recover its current planning context.

`lane current` is a successful read when no lane is focused and returns
`lane: null`. Inconsistent focus returns data plus the stable diagnostic rather
than mutating state during a read. Public results omit workspace roots,
database paths, sidecar paths, and runtime identity details.

### VS Code

The first editor surface is a focus client rather than a lane manager. A
**Work Lanes** tree appears first in the existing Exosuit Run container while
the Epoch Context, RFC Pipeline, and Phase Details views remain available.

The tree reads `lane list` through the existing machine channel and TraceCache.
Each row shows the lane title, its `prepared` or `executing` state, its
associated phase, and whether it is focused in this workspace. A target-icon
action and a command-palette Quick Pick invoke `lane focus`.

The extension does not store lane focus in VS Code workspace state. The focus
action succeeds only when Exo has committed the canonical workspace relation,
and reactive invalidation refreshes the tree from that result. Loading, no-lane,
focus-mismatch, and transport-error states are rendered as states rather than
guessed from stale editor memory.

Creation and start remain available through the CLI and `exo-run`. That is a
complete first editor role: observe the work streams available to this
workspace, see which one is current, and switch context. Rich creation,
attachment, status explanation, and lifecycle actions belong to the later
workbench UI.

## Compatibility and Migration

Lane adoption is additive. Existing epoch, phase, goal, task, inbox, RFC,
validation-lane, daemon, and sidecar behavior remains valid for projects with
no lanes. Commands that do not use lanes continue to use workspace-active phase
focus as they do today.

The migration adds empty tables and does not synthesize state from Git or the
plan. Portable lane rows follow the same policy-controlled SQL projection
contract as comparable project steering state. Workspace focus follows the
existing workspace-active-phase precedent and remains outside those dumps.

The first proof does not attach worktrees, branches, pull requests, RFCs,
signals, or validations to a lane. It also does not change goal or task foreign
keys. Those omissions are deliberate compatibility boundaries, not implicit
relationships to be inferred by adapters.

Consequently, this slice exposes no derived lane health such as blocked, ready,
passing, failing, fresh, or stale. It cannot classify a validation result using
only a commit SHA because it does not accept validation evidence at all.
Content or fileset fingerprints become a required part of the contract when a
later revision introduces observation-backed lane status.

## Drawbacks

This design introduces a new project object before the full workbench lifecycle
exists. An executing lane cannot yet be parked or closed, and its associated
phase remains the actual owner of mutations. Users will briefly encounter both
the adopted lane model and phase-centered execution.

The focus invariant also couples two workspace-local relations. That coupling
adds transactional work to phase focus and start, and inconsistent rows need an
explicit diagnostic path. The cost is justified because allowing the two
surfaces to disagree silently would make the first proof less trustworthy than
the phase model it extends.

Finally, `lane` has an established meaning in Exohook. The separate executable
and the consistent phrase *validation lane* make the public command practical,
but documentation must continue to disambiguate the concepts when they appear
together.

## Alternatives

### Use phases as lanes

Phases organize a sequence or campaign and carry the ownership boundary used
by current commands. Treating them as workspace-focused streams would either
make phase focus global again or overload phases with branch, review, and
outcome semantics. The lane instead references a phase while establishing the
identity that may eventually span several planning and review artifacts.

### Use branches or worktrees as lane identity

Git topology cannot preserve intent, and work may begin before a branch exists
or continue after it is deleted. Path-derived identity would also make portable
state depend on one machine. Branches and worktrees can become lane attachments
later without defining the lane.

### Use `exo workbench lane-*`

The current router can represent a `workbench` namespace with operations such
as `lane-create`, but that spelling makes the most common operations heavier
and less discoverable. Since validation lanes belong to a separate executable,
`exo lane` is the clearer product surface.

### Build the full lifecycle first

Attachments, signals, validation evidence, review repair, parking, and closure
are valuable, but designing them together would prevent the project from
testing the foundational identity and focus model. The narrow proof creates
real state that those later contracts can extend.

## Unresolved Questions

The first proof intentionally leaves several questions for later RFC revisions:
how a lane is parked or closed, how accepted outcomes are recorded, how
branches/worktrees/PRs/RFCs attach, how signals and validation observations
carry provenance, and how richer workbench surfaces explain status. None of
these questions changes the identity, portability, phase association, or
workspace-focus contracts defined here.

## Stage 3 Readiness

RFC 10202 is ready to become a Stage 3 Candidate when the implementation and
evidence establish all of the following:

- portable lane state survives deterministic dump/import with stable phase
  references and no workspace paths;
- linked worktrees share lane records while retaining independent lane focus;
- create, list, show, current, focus, and start agree across direct CLI, daemon,
  JSON machine channel, and MCP;
- lane and phase focus remain atomic, and inconsistent rows are diagnosed
  without read-time mutation;
- daemon replacement, editor reload, and a new agent session recover the same
  focused lane, intent, state, phase, and phase goals;
- the focus-only VS Code tree renders canonical state and never owns a second
  focus value;
- existing phase, goal, task, sidecar, outcome-ledger, RFC, and Exohook
  validation-lane suites remain green; and
- two-worktree dogfood demonstrates independent focus without duplicate state
  mutation or conversational reconstruction.

The Stage 3 reconciliation should describe the implementation that actually
lands, including any reviewed deviations from this draft.
