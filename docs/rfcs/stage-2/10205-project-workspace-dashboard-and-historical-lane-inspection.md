<!-- exo:10205 ulid:01kzccq0xs3cvc31jpds3yqvj1 -->

# RFC 10205: Project Workspace Dashboard and Historical Lane Inspection

**Status**: Stage 2 (Draft)
**Feature**: lane-centered-workbench
**Related**: RFC 10196, RFC 10202, RFC 10203, RFC 10204

## Summary

The lane workbench should let a person understand the project around the current
stream of work without changing what this workspace is doing.

Today the cockpit is intentionally centered on the focused lane. That makes the
current plan legible, but it gives the lane rail two incompatible jobs. The rail
is both a list of project history and a control for changing workspace focus.
Because focus is mutable execution state, lanes whose phases are complete or
pending are disabled. A person can see that those lanes exist but cannot open
them to understand what happened or what is coming next.

This RFC proposes a project workspace dashboard and a separate lane-inspection
interaction. The dashboard presents the workspaces that Exo knows belong to the
project, the lane and phase each workspace is advancing, and enough fresh Git
context to distinguish those views. Selecting any lane opens a read-only account
of its intent, plan, progress, and outcome. Focusing a lane remains a distinct
operation available only when the lane and phase are eligible for execution in
the current workspace.

The result is a cockpit in which project navigation is safe to explore. Looking
at another worktree or a finished lane does not silently redirect the agent,
change the active phase, or alter the plan.

## Motivation

RFC 10203 established the local workbench as a workspace-bound view of one Exo
project. RFC 10204 made the focused lane's plan collaboratively editable without
turning the browser into a command console. Those boundaries are useful: the
launch identifies a workspace, and the focused lane answers what that workspace
is doing now.

A project, however, is larger than its currently focused lane. Linked worktrees
may be advancing different streams. Completed lanes contain the rationale and
outcomes that explain how the project arrived here. Prepared lanes show which
campaigns have been made concrete without claiming that they have started.

The current snapshot exposes a summary of every lane, but it publishes detailed
goals and tasks only for the focused lane's phase. The cockpit therefore disables
lanes whose phases are not active, because its only selection operation is lane
focus. That is correct for mutation safety and frustrating for navigation. A
person who clicks a completed lane is usually asking to read history, not to
resume execution inside a completed phase.

The same gap appears across worktrees. Exo already treats linked worktrees as
workspace-local views of shared project state, and workspace focus is already
isolated by workspace root. The browser still presents only the workspace that
launched it. When several worktrees are active, branch names and separate links
must stand in for a project-level explanation of how those workspaces relate.

A project workspace dashboard closes both gaps without weakening workspace
isolation. It makes the project's known views and history inspectable while
leaving execution authority exactly where it is today.

## Guide-Level Experience

### The focused lane remains the place for execution

Opening the workbench still establishes an exact project and workspace. The
focused lane remains the primary execution surface for that workspace: its
intent, phase, goals, tasks, progress, and available planning actions appear as
they do today.

Project navigation also offers a workspace dashboard. Entering that dashboard
is ordinary navigation. It does not clear the focused lane or replace the
workspace-bound session. Returning to the focused lane is an explicit, stable
action rather than a reconstruction from browser history.

This RFC does not decide whether the workspace dashboard eventually becomes the
default project home. It establishes the dashboard as a first-class project
surface that later RFC and pull-request views can extend.

### Workspaces are project views

The dashboard presents each currently known worktree as a workspace within the
project. The current workspace is visually primary. A sibling workspace summary
shows a human label, its branch or detached revision, its focused lane and active
phase when known, Git cleanliness, and when those facts were observed.

No structured browser field exposes a raw filesystem path. A workspace is
identified by the same opaque workspace key used by the workbench session.
Project membership comes from Git's worktree registry. Before Exo records a live
observation, it resolves the checkout and requires the same project identity and
state root as the current session. Human-authored progress and outcome evidence
remains verbatim, so authors may still refer to paths in the prose they record.

The dashboard distinguishes live observations from remembered registrations.
An observation is live for five minutes. After that it is stale until an Exo
request from that workspace refreshes it. A Git registration whose checkout can
no longer be validated is unavailable. The registration remains visible while
Git retains it, including when Git marks it prunable; removing the Git worktree
registration removes the workspace from the dashboard. This keeps retention
attached to the durable local worktree model rather than an arbitrary elapsed
time.

The dashboard is informational in this RFC. It does not run commands in a
sibling worktree, transfer phase ownership, or move focus on another workspace's
behalf.

### Selecting a lane means inspecting it

Every lane in the project is selectable for inspection, including lanes attached
to completed or pending phases. Selection opens a read-only lane view containing
the lane's intent, phase, goal and task progression, recent progress evidence,
and recorded completion outcome when one exists.

Inspection is browser navigation state. It does not write Exo state and is not
projected to sibling worktrees. Reloading may preserve the inspected lane in
same-entry browser state, but Exo continues to report the workspace's actual
focused lane independently.

The lane list uses two distinct signals for those states. A bullseye marks the
lane focused for execution in this workspace. The selected-row treatment marks
the lane currently being inspected in this browser. A focused lane may also be
selected, but selection by itself never communicates execution state.

The inspected view names its relationship to the current workspace. A lane may
be focused here, active in another known workspace, prepared for a pending
phase, or historical because its phase has completed. These labels describe
observed relationships; they are not new lane lifecycle values.

When an inspected lane is eligible to become the current workspace's execution
stream, the view may offer a separate Focus in this workspace command. That
command retains the existing lane-focus preconditions and mutation behavior.
Completed-phase lanes never offer it. Pending-phase lanes remain inspectable but
cannot be focused until their phase starts.

### History explains the present

A completed lane should read as an outcome, not as a disabled task list. Its
summary leads with what the lane set out to do, what completed, the approved
goal or task outcomes that remain available, and the phase completion evidence.
Detailed task progress can be disclosed without making the first view a raw log.

The lane rail keeps every lane from the latest authoritatively completed phase
visible immediately above the focused and other non-completed lanes. This keeps
the just-finished campaign available as the current work's nearest context while
the focused lane remains visibly distinct. The previous campaign is selected by
the greatest non-null phase completion time, with stable phase identity breaking
an exact timestamp tie. Lanes from older completed phases and lanes whose phase
has no authoritative completion time remain available under Earlier lanes.
Creation order, lane order, task completion, and outcome presence never stand in
for phase completion evidence.

This is historical inspection rather than restoration. Reopening a completed
lane does not make old controls look available, and it does not suggest that the
phase can be resumed. Follow-up work begins in a new eligible lane and may link
back to the historical one.

### Navigation adapts without changing meaning

On wide screens, project navigation can remain persistently visible. On narrow
screens, the same navigation appears as an anchored popover. The content and
selection semantics remain identical in both forms: selecting a row inspects,
while an explicitly named focus action mutates execution state.

The dashboard and inspected-lane views preserve the current cockpit's quiet,
work-oriented visual language. Workspace summaries are compact comparisons, not
decorative cards, and status is communicated through concise labels and
evidence rather than repeated large text.

## Design Direction

### A browser-safe project projection

`workbench.snapshot` schema version 4 includes a browser-safe `project_workspaces`
projection rather than asking the frontend to join lane, phase, Git, and runtime
facts itself. Each summary includes an opaque workspace key, a path-free label,
Git identity and optional cleanliness, the focused lane and active phase when
known, an observation time, and `live`, `stale`, or `unavailable` availability.
Detailed lane history remains a separate read, so adding another completed phase
does not make every ordinary snapshot proportionally larger.

Each lane summary also carries nullable `phase_completed_at` evidence from the
canonical phase record. Exo emits the authoritative completion time in RFC 3339
form when it exists and emits `null` otherwise. The snapshot does not infer a
completion time from phase status, lane creation order, completed tasks, progress
logs, or recorded outcomes. Consumers use this field to identify the immediately
previous completed campaign and collapse completion history whose order is older
or unknown.

The current workspace continues to be sampled during snapshot composition. The
daemon also records a bounded observation whenever it accepts a request from a
validated sibling workspace. Discovering a Git-registered sibling records its
branch or detached revision without running an eager dirty-tree sweep across the
project; cleanliness remains unknown until that workspace is observed directly.
A sibling summary never changes that workspace or opens a database through its
path.

Project workspace observations are machine-local. They are not portable project
policy and do not belong in repository or sidecar SQL projections. Durable lane,
phase, goal, task, and outcome records remain shared project state.

### Lane details are loaded on demand

The browser obtains non-focused lane details through the pure daemon-backed
`workbench inspect <lane-id>` command. The HTTP adapter exposes the same read as
the closed `lane_inspect` browser operation guarded by the `workbench.inspect`
session capability. The adapter supplies the session's validated workspace root;
neither the browser nor the command accepts a project, database, state-root, or
filesystem-path selector.

Inspection is a compatible read refinement for sessions that already hold
`workbench.snapshot`. When a daemon restores a durable session minted before this
capability existed, it adds `workbench.inspect` to that snapshot-capable grant
and persists the upgraded capability set on renewal. This migration does not add
focus or planning authority to a session that lacked it.

This transport choice follows measurements from the project that produced this
RFC. Its ordinary workbench snapshot was 15,431 bytes. Individual phase-detail
reads ranged from 9,511 to 58,714 bytes, and eagerly including the five lane
phases would have added roughly 191 KB before workbench-specific shaping. An
on-demand read keeps snapshot refresh proportional to the project overview while
making each lane available through one bounded request.

`workbench inspect` returns `workbench.lane_inspection` schema version 2. The
result contains the observation time and workbench revision; path-free project,
daemon, and current-workspace identities; the complete lane identity and intent;
the lane's phase, goals, and tasks; and an inspection relationship derived from
current state. The relationship distinguishes the lane focused here, another
lane whose in-progress phase can be focused here, a prepared lane whose phase has
not started, and a historical lane whose phase is complete. It also carries an
explicit `can_focus_here` boolean. Sibling-workspace attribution is added with
the workspace dashboard rather than inferred from stale focus rows in this read.

The historical projection reuses the canonical phase, goal, task, progress, and
outcome records. It includes approved goal and task outcomes when present. Task
progress uses the workbench's existing bound of eight recent progress entries
and 16 KiB per task; outcomes use the same 16 KiB text bound. A truncation marker
on each bounded field makes omission visible. The browser does not synthesize a
second archive model, expose arbitrary task-log kinds, or add a structured field
containing a private path.

### Inspection and focus are separate state machines

The browser tracks which project surface and lane the person is inspecting.
Exo tracks which lane the workspace is focused on. The UI may display both at
once, but it never treats one as evidence of the other. Opening a lane records
its ID in same-entry browser history and loads its inspection result. The
browser also mirrors the public session selector and current inspection in
tab-local session storage so framework hydration cannot discard the current
entry during reload; the independent HttpOnly cookie remains the session's
authority. Back, forward, and reload restore the navigation choice without
writing Exo state.

A snapshot refresh therefore cannot pull the person away from a historical view
merely because the focused lane changed. When the workbench revision advances,
the browser refreshes the inspection against the new revision. It discards a
late inspection response from an older daemon or revision. If the inspected lane
disappears, the browser returns to the workspace dashboard with a specific
explanation; a temporary read failure preserves the last valid inspection and
offers retry.

Focus remains a separately named mutation bound to the current session and
workspace. The server rechecks the lane and phase preconditions at execution
time, so an inspection result is never authority to focus stale state. On
success, the browser clears the inspection navigation and returns to the focused
execution surface. Inspection itself needs no request ledger because it has no
effect.

### Compatibility

RFC 10203's workspace-bound host, session, and launch contract remains
unchanged. RFC 10204's planning operations continue to require the exact focused
phase and remain unavailable while a historical or sibling view is being
inspected.

The historical-inspection slice remains a separate schema-version-2 result.
Adding authoritative phase completion evidence advances the ordinary
`workbench.snapshot` contract from schema version 3 to version 4 and the shared
lane summary in `workbench.lane_inspection` from version 1 to version 2, with
checked Rust and TypeScript fixtures.
The embedded cockpit and daemon ship together and fail closed on a mismatched
snapshot version. The daemon, CLI, MCP, and browser continue to share one project
authority.

This RFC introduces no new lane lifecycle, phase lifecycle, ownership transfer,
or portable workspace path.

## Relationship to Existing RFCs

RFC 10196 defines how one project reconciles canonical documents with
worktree-local observations. This RFC applies the same distinction to cockpit
presentation: durable project state is shared, while checkout identity and Git
observations remain workspace-local.

RFC 10202 establishes lanes as the product object connecting workspace, plan,
decision, and delivery artifacts. This RFC makes multiple lanes and workspaces
legible at project scope without collapsing them into branches.

RFC 10203 continues to own local hosting, session authority, workspace
validation, snapshots, and lane focus. This RFC extends its read model and
separates inspection from focus.

RFC 10204 continues to own interactive task planning and outcome approval for
the focused phase. Historical inspection is deliberately non-mutating and does
not broaden that planning capability.

A subsequent project-flow RFC may add RFC motion, pull-request attention, and
acknowledged coordination to the project dashboard. Those concerns should build
on the workspace and inspection model here rather than entering this first
dashboard contract.

## Scope

This RFC covers a browser-safe project workspace dashboard, summaries of known
linked worktrees, read-only inspection of every lane, clear separation between
inspection and execution focus, truthful freshness presentation, and responsive
project navigation.

It does not cover cross-project navigation, remote access, commands executed in
sibling worktrees, phase ownership transfer, a new lane lifecycle, RFC or
pull-request aggregation, inbox acknowledgement, coordination history, durable
locald routing, or replacement of the existing focused-lane planning surface.

## Drawbacks

The dashboard adds another level to a product that has benefited from beginning
with one focused lane. Its value depends on keeping the project summary compact
and preserving an obvious route back to current execution.

Workspace observations can become stale. Representing that uncertainty is more
honest than hiding sibling worktrees, but it adds states that the UI and tests
must handle deliberately.

Separating inspection from focus creates two nearby selections that can be
confused if the interface relies on highlighting alone. The product must use
clear language and distinct actions so that ordinary browsing never feels like
a pending mutation.

A versioned project projection and historical read increase the workbench
protocol surface. Keeping those reads bounded and composed from canonical Exo
state is necessary to avoid building a parallel project model for the browser.

## Alternatives

The cockpit could keep completed and pending lanes disabled. That preserves the
smallest interaction model but makes the project's visible history unusable.

Selecting a lane could always focus it. This avoids a separate inspection state
but would either permit invalid focus transitions or make historical lanes
permanently inaccessible.

The dashboard could identify workspaces only by branch. Branches are useful
handles but do not identify detached worktrees, multiple checkouts of one
branch, workspace-local focus, or observation freshness.

The browser could receive raw workspace paths and query each checkout directly.
That would expose machine-local data, duplicate Exo's authority, and weaken the
session's project boundary.

The project snapshot could eagerly embed every log and outcome. That simplifies
navigation at small scale but makes payload growth and privacy harder to reason
about. A bounded summary plus canonical drill-in is the more durable direction.

## Unresolved Questions

The first dogfood pass should determine whether the outcome-first historical
summary needs a dedicated link to the complete canonical phase read. The bounded
result always says when progress or outcome text was truncated, so adding that
link does not require changing the inspection authority model.

A later project-flow proposal will decide whether the workspace dashboard becomes
the default project home and how RFC and pull-request views compose with it.

## Stage and Readiness

This RFC is a Stage 2 draft because workspace summaries and read-only lane
inspection are accepted as the project-navigation foundation. Measured payload
evidence selects a closed on-demand read, the inspection result has an explicit
versioned and bounded schema, and browser navigation is testable independently
from workspace focus.

Stage 2 implementation begins with historical and prepared lane inspection and a
schema-version-4 sibling-workspace projection. The RFC must not become a Stage 3
candidate until the responsive project dashboard has been implemented and
dogfooded across linked worktrees.
