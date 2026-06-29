# Lane-Centered Workbench

## Product and system proposal

**Thesis:** a lane is an observable execution stream. It is not a prettier name for a branch, worktree, pull request, task list, phase, validation lane, or chat thread. It is the product object that connects those artifacts while preserving the difference between durable project truth, branch-local workspace reality, observed evidence, and situated human judgment.

The Lane-Centered Workbench makes concurrent agentic work visible and manageable. A user should be able to open a project and immediately understand which streams of work are active, what needs attention, what is ready for review or merge, what is blocked, and why the system believes each status claim.

The CLI, MCP transport, editor surface, and future workbench UI are adapters over the same project state. They should not create parallel truths.

---

## Problem

Agentic project work rarely proceeds as one linear chat thread. It branches into implementation, research, review, repair, parked experiments, RFC decisions, and follow-up lessons. Today those threads often live across partially connected surfaces:

- Git branches and worktrees show code isolation but not intent.
- Pull requests show review state but not the work stream that produced them.
- RFCs capture decisions but not always which active work is proving or changing them.
- Chat sessions contain useful context but are not durable project truth.
- Task lists track inventory but usually do not encode evidence, provenance, or closure.

The missing product object is the active stream of work itself.

A lane fills that gap. It gives the project a durable, inspectable identity for one stream of change while keeping branches, worktrees, PRs, RFCs, goals, validation, and agent sessions linked but distinct.

---

## Product promise

When a user opens a project, the workbench should answer:

- What lanes exist?
- Which lane is focused in this workspace?
- Which lane needs attention first?
- What is the agent working on?
- What is blocked?
- What is waiting on review?
- What is ready to close, merge, or accept?
- What user feedback or review comments are unresolved?
- What decisions or lessons should survive after a lane closes?
- Why does the system believe each status claim?

The agent should be able to answer the same questions from tools, not from chat memory.

---

## Core model

### Project

A project is the durable identity and state boundary. It owns the long-lived story of the work: mission, durable decisions, steering rules, ideas, persistence policy, cross-lane history, and global workbench state.

A project is normally anchored by the git common directory. Different worktrees can share one project identity while retaining distinct workspace views.

### Lane

A lane is a branch/worktree/PR-sized stream of change. It is the primary unit of active work.

For users, **lane** is the visible product word for work streams. A git worktree is an implementation mechanism. A branch is a version-control handle. A pull request is a review artifact. An RFC is a decision artifact. The lane is the product object that connects them.

The repo may also use the word “lane” for other concepts, especially Exohook validation lanes. Those are not the same thing. Product UI can say **lane** when the surrounding workbench context is clear, but implementation and documentation should disambiguate when needed: **workbench lane** or **work lane** for this proposal, **validation lane** for Exohook/check enforcement. CLI/API naming should verify the existing namespace before claiming `exo lane` as the final command surface.

A lane owns or links:

- source handle: issue, RFC, idea, user request, incident, or experiment;
- workspace root when checked out;
- branch and base branch when known;
- linked PR and review state when known;
- linked RFCs and decision relationships when known;
- active goals and tasks;
- progress logs;
- validation status;
- signals from users, agents, GitHub, or the system;
- merge readiness;
- parked or abandoned reason;
- durable lessons promoted back to project memory.

### Goal

A goal is a Prepare → Execute → Review sized unit inside a lane.

The goal is large enough that preparation matters, small enough that execution produces one reviewable result, and meaningful enough that completion is visible progress.

A goal owns hypothesis, tasks, validation criteria, evidence, divergences, review result, and follow-up lessons.

### Session

A session is one human-agent run inside a lane. It owns short-lived conversational and operational context: current user intent, active tool availability, recent observations, immediate next move, context pressure, and handoff needs.

Session memory is never the source of truth for project or lane state. Sessions read and write through lane-aware tools.

---

## Layered state model

The workbench separates three layers that are often conflated.

```text
Project state
  Shared durable facts:
  lanes, goals, tasks, RFC identities, PR refs, decisions,
  signals, validations, activity, cross-lane memory

Workspace view
  Branch-local file reality:
  checkout root, branch, commit, dirty tree, visible documents,
  local generated files, editor/workspace context

Observations
  Provenance-bearing claims:
  Git status, PR status, CI result, review comment,
  validation run, tool runtime, document visibility
```

The same lane can be viewed from different workspaces. Absence from one worktree is not evidence that a project-level fact is obsolete. A status claim should therefore carry provenance: who or what observed it, when, in which workspace, at which commit, through which runtime, and against which workspace content state.

---

## Product vocabulary

| Internal term | Primary UI label | Use |
|---|---|---|
| Workbench lane | Lane | The active stream of work. Use “workbench lane” or “work lane” when disambiguating from validation lanes. |
| Validation lane | Validation lane | Existing Exohook/check grouping concept. Do not conflate with workbench lanes. |
| Lane lifecycle | Status | Proposed, prepared, executing, reviewing, repairing, ready, parked, closed. |
| Blocked condition | Blocked | Attention reason or health/signal condition. Not a lifecycle/status value. |
| Health facets | Health | Work, review, validation, tool runtime, state sync, signals. |
| Perception event | Signal | User feedback, system observation, review comment, or completion claim. |
| Workflow confirmation | Outcome review | A semantic close/accept/revise decision. |
| Portability health | State sync | Whether state is durable, checkpointed, and portable. |
| Observation provenance | Why this status? | Normal inspector affordance, not a debug-only feature. |
| Workspace overlay | Workspace view | What this checkout, branch, or worktree can currently see. |
| Runtime identity | Tool runtime | The daemon, proxy, binary, protocol, and database identity serving this workspace. |

---

## System principles

1. **State before surface.** UI adapters render canonical state; they do not own it.
2. **Claims require provenance.** A status must say where it came from and how fresh it is.
3. **Observable facts should be observed.** Do not ask the user for facts the system can inspect.
4. **Situated judgment should be asked for.** Taste, priority, timing, and acceptance remain collaborative.
5. **Structured steering state belongs in state.** Do not bury active project truth in chat.
6. **Prose decisions remain documents.** RFCs are indexed and linked, not converted into pseudo-state.
7. **Continuity must not become ceremony.** A lane earns its keep by reducing reconstruction, coordinating concurrent work, or preserving evidence.
8. **Action before explanation.** The UI should first answer “what needs my attention?” and “what should happen next?”
9. **Destructive cleanup is exceptional.** Dirty workspace cleanup should inspect, classify, recommend, and wait for confirmation.
10. **Adapters share one semantic contract.** CLI, MCP, VS Code, agent tools, and future workbench surfaces invoke the same operations.

---

## State model sketch

The sketches below describe conceptual state shape, not final migrations. They intentionally omit path-derived local `ProjectId` from portable rows. Commands are scoped by the resolved project database. If an implementation stores local project identity internally for joins or caches, repo/sidecar SQL dumps must either omit that field or remap it at import so portable lane state is not tied to the checkout path that exported it.

### Lane

```text
lanes
  id
  text_id
  title
  lifecycle
  source_kind
  source_ref
  branch
  base_branch
  pr_url
  active_goal_id
  last_activity_at
  parked_reason
  closed_reason
```

`text_id` is the portable lane identity. It should use the same stable-id posture as other projected Exo entity tables so SQL dumps can omit local rowids and still reconstruct lane rows and foreign keys.

Lane focus is intentionally not stored as a singleton field on the lane. Focus is workspace-local: linked worktrees share project state, but each workspace may have its own focused lane.

### Lane workspace checkout

```text
lane_workspace_checkouts
  id
  lane_id
  workspace_key
  branch
  checkout_root_kind       # local | sidecar | repo | unknown
  checked_out_at
  last_seen_at
```

This relation records where a lane is checked out. It is separate from focus: a lane can be checked out but not focused, focused in one workspace and observed from another, or checked out in multiple linked worktrees. Portable projections should carry only stable, non-path workspace keys or reviewed summaries; raw checkout paths remain local observation data.

### Workspace lane focus

```text
lane_workspace_focus
  workspace_key
  focused_lane_id
  focused_at
  focused_by
```

This relation enforces one focused lane per workspace while allowing the same lane to be observed or focused from more than one workspace without overwriting another workspace's focus. It is workspace-local runtime state, analogous to workspace-active phase focus. Portable projections should not dump one machine's focused lane into another checkout.

### Observation

```text
lane_observations
  id
  lane_id
  workspace_key
  branch
  commit_sha
  dirty_state             # clean | dirty_source | dirty_generated | conflicted | unknown
  workspace_fingerprint   # optional content/fileset fingerprint when available
  observer_kind           # cli | vscode | agent | daemon | github_sync | validation
  observer_id
  observed_fact_kind      # git_dirty | pr_status | ci_status | review_comment | rfc_visible | validation | runtime
  observed_fact_json
  observed_at
  expires_at
  confidence              # observed | inferred | stale | conflicted
```

Validation observations should record enough workspace content identity to know what was actually checked. A commit SHA is not enough when the workspace has uncommitted source edits.

Most observations are workspace-local facts and should live in local-only tables excluded from repo and sidecar SQL dumps. If a later implementation needs portable observation summaries, it should add an explicit projected summary table or a reviewed filtered-dump rule rather than relying on a `portable` boolean inside a table that the current dump machinery would otherwise serialize wholesale.

### Signal

Signals should extend or bridge Exo's shared perception inbox, not fork it. The table below describes the lane-scoped perception shape that the UI calls **signals**; implementation should reuse the existing inbox/perception channel wherever possible so existing inbox surfaces, completion guards, and steering delivery can see unresolved lane feedback.

```text
inbox_data / perception_events
  id
  entity_type            # current: goal | task | rfc | phase | epoch | project
  entity_id
  lane_id                # requires schema migration before lane-scoped signals land
  source                 # current style uses hyphenated values such as user-feedback
  intent                 # claim | concern | inquiry | fyi
  priority               # immediate | next_touch | when_relevant
  confidence             # high | low | null
  agent_id
  subject
  body
  status                 # pending | acknowledged | resolved | archived
  created_at
  resolved_at
```

Lane, PR, validation, and workspace signal scopes require an inbox/perception schema migration before writes use those scopes. The first implementation should either keep signals on currently accepted entity types or land the enum/source migration alongside lane signal writes.

If a separate materialized `lane_signals` view is useful for UI performance, it should be derived from the perception channel rather than becoming a second source of truth.

### Tool runtime

```text
lane_runtime_snapshots
  id
  lane_id
  workspace_key
  daemon_id
  binary_identity
  protocol_version
  database_path
  runtime_path
  observed_at
  health                 # healthy | stale | drifted | unavailable
```

### State sync

```text
lane_state_sync
  lane_id
  local_durable
  checkpoint_pending_count
  sidecar_ahead_count
  remote_status          # none | synced | ahead | behind | diverged | unreachable
  writer_status          # owned | no_owner | another_writer | stale_reclaimed
  last_barrier_at
```

---

## Status model

Lifecycle remains the user-facing status:

```text
proposed → prepared → executing → reviewing → repairing → ready → parked → closed
```

**Blocked is not a lifecycle value.** It is an attention reason or health/signal condition. A blocked lane is still prepared, executing, reviewing, repairing, ready, parked, or closed. If work is intentionally set aside, use `parked` with a parked reason. If work remains active but cannot advance until a decision, review, signal, or dependency is resolved, keep its lifecycle status and derive `blocked` as the attention reason.

Health facets are independent:

- **Work:** local dirty, conflicted, clean, or unknown state.
- **Review:** PR and review artifact state.
- **Validation:** tests, checks, evidence, workspace fingerprint, dirty state, or accepted exceptions.
- **Tool runtime:** whether the surface is reading the right state.
- **State sync:** local durability, checkpoint, remote, and ownership.
- **Signals:** unresolved user feedback, review comments, or completion claims.

The overview should not make the user interpret all facets at once. It should derive one primary attention reason, show the next safe action, and make supporting facets inspectable.

A useful first derivation can be simple:

```text
if closure is attempted and unresolved scoped signals exist:
  attention = closure blocked
else if tool runtime is stale or unavailable:
  attention = runtime attention needed
else if state sync is diverged, blocked by another writer, or has checkpoint debt that affects the lane:
  attention = state sync attention needed
else if an immediate signal or dependency prevents the next action:
  attention = blocked
else if required PR or CI observations are expired or unavailable:
  attention = status stale
else if PR review requests changes or CI fails on current head:
  attention = repair needed
else if validation passed on an older commit than the workspace head:
  attention = validation stale
else if validation passed at the same commit but relevant source files are now dirty:
  attention = validation stale
else if validation fingerprint does not match the current workspace fingerprint:
  attention = validation stale
else if the lane is parked and the PR is waiting for review:
  attention = waiting for review
else if the lane has no workspace but has active work:
  attention = workspace needed
else if the lane lifecycle is ready:
  attention = ready for outcome review
else:
  attention = on track
```

---

## Interface contract

The lane system should expose one semantic operation layer through CLI, MCP, machine channel, VS Code, agent tools, and future workbench surfaces.

Minimum conceptual operations:

```text
workbench lane-list
workbench lane-show <id>
workbench lane-create <title> [--source ...]
workbench lane-focus <id>
workbench lane-park <id> --reason ...
workbench lane-attach-pr <id> <url>
workbench lane-log <id> --message ...
workbench lane-observe <id> --kind ...
workbench lane-explain-status <id>
workbench lane-close <id> --merged|--abandoned|--accepted
```

The final CLI namespace should be chosen after reconciling with existing Exo uses of the word lane, especially validation lanes. The product word remains lane, but the command spelling should fit the current Exo router: one namespace plus one operation token. A safe initial shape is `exo workbench lane-create`, `exo workbench lane-focus`, and related `workbench lane-*` operations, unless implementation first changes the router to support deeper command nesting.

Close is not merely a write. It should eventually be an **outcome review**: a semantic user decision about whether the proposed outcome is accepted, needs revision, is not complete yet, or should be discussed.

---

## UI model

The UI should feel like a serious operating surface, not an AI command center.

### Project overview

The first read should answer: what needs attention first?

The overview should include:

- an attention queue;
- lane table grouped by lifecycle;
- primary next action per lane;
- compact health facets;
- `Why this status?` affordance on every non-obvious claim.

### Lane detail

The lane detail should center the current goal and instrument reading:

- hypothesis;
- current move;
- evidence;
- divergences;
- live signals;
- validation state;
- next safe action.

### PR view

The PR view should treat review comments and CI failures as repair inputs that can become bounded lane goals. A PR is a review artifact, not the lane itself.

### RFC view

The RFC view should show how lanes motivate, implement, prove, block, or revise durable decision documents. RFC prose remains in files. Lane links and implementation evidence belong in state.

### State inspector

`Why this status?` is the trust surface. It should begin in product language:

```text
Validation is stale because the last successful test run was observed
at commit 91ab02f, while the lane workspace is now at a38c2e1 and
has uncommitted source changes.
```

Only after that should it expose provenance:

```text
Source
  observation obs_validation_stale_20260614_0946
  observed by exo.test.adapter_worker
  observed at 2026-06-14 09:46 PDT

Workspace view
  branch wycats/oauth-cleanup
  dirty state: source changes present
  workspace fingerprint: dirty-source:6c9f...

Tool runtime
  exo@7c7bf4d
  daemon daemon_04f9
  database {state_root}/cache/exo.db
  runtime healthy

State sync
  local durability current
  checkpoint pending: 3 rows
  remote status: ahead 1
  writer status: owned

Recommended action
  Re-run adapter and worker tests, then sync PR checks.
```

The raw record can be available, but it should not be the primary explanation.

---

## Degraded states

A lane-centered workbench will spend much of its time in imperfect states. That is not an edge case; it is the reason the product exists.

The UI should handle these as normal product states:

- lane has no workspace;
- lane has no PR because none is expected;
- GitHub is unavailable, so PR state is last-known;
- tool runtime is stale;
- state sync has checkpoint debt;
- another writer owns sidecar auto-persist;
- RFC metadata exists but the document is absent from this workspace view;
- validation is stale relative to current head;
- validation is stale because the workspace has uncommitted source edits after the last run;
- user believes work is done but unresolved signals remain.

Each degraded state should say what is known, what is unknown, and what action would improve the situation.

---

## Implementation path

The implementation should prove the lane model before trying to express the whole workbench UI. A useful first build is not a smaller version of every screen. It is one end-to-end path where a lane can be created, focused in a workspace, observed, explained, parked, and eventually closed through the same state contract from the CLI, the editor, and agent tools.

The minimum useful workflow is:

```text
create or focus lane in the current workspace
  → read project, workspace focus, lane, current goal, and workspace view
  → perform bounded work and log progress
  → observe workspace, validation, PR, runtime, and signals
  → explain the lane's attention state
  → repair, park, or close through explicit operations
```

The important part is not the number of commands. It is that every participant sees the same lane reality. The agent should not reconstruct the lane from chat. The user should not infer status from branch names. The editor should not render a parallel interpretation of the plan.

The first build succeeds when these surfaces agree even before the UI is sophisticated.

---

## Migration posture

The lane model can evolve from the current Exo vocabulary without pretending the old model was wrong.

A current phase can become a planning band, campaign, or source for one or more workbench lanes. A workspace-active phase becomes workspace-local lane focus. A goal keeps its PER-sized meaning, but moves under the lane that gives it branch, PR, RFC, validation, and signal context. A task remains the executable step inside a goal. An inbox item becomes a scoped signal through the existing shared-perception channel. PR metadata becomes a review artifact reference. RFC metadata remains an index over documents, with explicit lane relationships rather than a forced database model of the prose. Existing validation lanes remain validation lanes; do not migrate them into workbench lanes.

This translation matters because the system should not begin with a large semantic migration. The first implementation can introduce lanes alongside existing concepts, then move pressure onto lanes where the abstraction clearly improves real work: concurrent branches, parked PRs, stale validation, review repair, and workspace-specific document visibility.

---

## What proves the design

The design is ready for serious dogfooding when it changes behavior, not merely when it renders the right screens.

A user should open a project and know which lane needs attention without reading chat history. An agent should resume a focused lane by reading project, workspace focus, lane, goal, and session state in that order. A stale validation claim should explain exactly why it is stale, including dirty workspace state when relevant. A PR review comment should become a bounded repair goal instead of an ambient obligation. A parked PR should remain visible without blocking unrelated work. Closing a lane should record the accepted outcome, evidence, unresolved exceptions, and follow-ups. A missing RFC document in one workspace should not rewrite shared metadata. A stale runtime should be detected before status is presented as trustworthy. Destructive cleanup should never be the default recommendation without workspace inspection.

If those behaviors hold under ordinary project work, the visual workbench can grow. If they do not, more surface area will only make the system look more confident than it is.
