<!-- exo:10204 ulid:01kyrn7n5hzt25b2ay72130rd8 -->

# RFC 10204: Interactive Lane Planning and Outcome Approval

**Status**: Stage 2 (Draft)
**Feature**: lane-centered-workbench
**Related**: RFC 10202, RFC 10203

## Summary

The lane workbench should let a person shape and advance the tasks in the lane
they are looking at without turning the browser into a general-purpose Exo
command console.

This RFC adds a deliberately closed planning protocol to the local workbench.
A person may add a task under an existing goal, edit its title, reorder it
within that goal, start it, record progress, and review an outcome before
approving completion. Exo remains responsible for task identity, phase
ownership, atomic mutation, request recovery, projection, and sidecar
persistence. The browser expresses an intention; it does not become a second
plan authority.

The protocol is bound to the workspace, focused phase, and exact snapshot the
person acted from. A stale gesture changes nothing. Ambiguous retries preserve
their request identity. Completion review is non-mutating, and approval can
record only the exact outcome that Exo previously returned for review in the
same browser session.

## Motivation

RFC 10203 made the browser workbench useful as a shared orientation surface. It
shows the lane's intent, the workspace identity, and the focused phase's goals
and tasks. That is enough to answer what the workspace is doing, but not enough
to use the screen for planning together.

Today, changing the plan means leaving the workbench and translating the
visible structure back into an agent or CLI command. That separation is most
noticeable during collaborative planning: the human and agent can look at the
same lane, but the human cannot refine the tasks from the surface that made the
plan legible.

A broad command console would solve the mechanical problem while weakening the
product boundary. It would expose Exo syntax, confirmation inputs, local paths,
and commands whose effects do not belong in a browser capability. It would
also make the frontend responsible for understanding command behavior that Exo
already owns.

The useful middle ground is a small planning language. The browser should
express only the planning intentions its interface presents, while Exo resolves
and commits them through the same daemon, request ledger, SQLite transaction,
and persistence paths used by its other clients.

## Guide-Level Experience

### Planning where the work is visible

The focused phase remains the center of the workbench. Goals organize ordered
tasks, and the order shown in the browser is the order stored by Exo.

Each goal offers an add-task action. A task row offers the actions that are
valid for its current state: edit the title, move it up or down, start it,
record progress, or prepare completion. The first version uses explicit move
controls rather than drag and drop. This gives pointer and keyboard users the
same behavior and keeps the protocol in terms of a stable destination index.
Moving a task to a different goal is a different planning operation and is not
part of this RFC.

Text editing stays close to the object being changed. Title editing happens in
place. Adding a progress note opens a compact disclosure below the selected
task rather than making every row permanently tall. An unsent title, task, or
note remains local input; it is not reflected in the plan until Exo confirms a
commit.

Committed progress notes return in later snapshots as timestamped task progress.
The host projects only canonical `progress` entries; internal task-log kinds do
not become browser data. Each task carries at most its eight most recent updates
and 16 KiB of progress text. A `progress_truncated` marker tells the cockpit
that earlier or longer canonical history remains available through Exo. The
cockpit keeps the recent entries compact until a person opens them for review.

While a request is in flight, the relevant control is pending and cannot submit
a second intention. The browser does not optimistically rewrite the plan. On
success it refreshes from the authoritative snapshot, normally through the
existing write invalidation stream. If Exo reports that the snapshot is stale,
the workbench keeps unsent text where useful, refreshes, and asks the person to
apply the intention again against the new plan.

### Showing goal progress without inventing goal state

Goal state and task progress answer different questions. Exo may correctly
keep a goal `pending` while some of its tasks are already complete. Repeating
the word `Pending` beside every row hides that distinction.

The workbench therefore keeps canonical status untouched and derives a compact
presentation summary from the tasks:

- a pending goal with no started or completed task is shown as `Not started`;
- a goal with any started or completed task is shown with the active treatment
  and a summary such as `2 of 3 tasks complete`;
- when every task is complete but the goal itself is not, the summary reads
  `Tasks complete · Goal pending`;
- only a canonically completed goal receives the completed treatment.
- abandoned, skipped, deferred, or otherwise non-actionable goal and task
  statuses retain their explicit lifecycle label and expose no planning
  controls.

Task rows use the status icon as their visible state marker. They do not repeat
`Pending` or `Completed` in a trailing text column. The same status remains in
the accessible name for assistive technology. Goal titles use balanced
wrapping, task titles use pretty wrapping, and progress metadata occupies its
own quieter line so that status does not collide with prose.

These are view rules over the existing snapshot. They do not add a derived
status to SQLite or require a snapshot version beyond the current
`WorkbenchSnapshot` schema version 2.

### Reviewing and approving completion

Completing a task is different from the other planning actions. The person
first supplies an outcome summary and asks Exo to review it. Review performs
the same readiness evaluation used by task completion, but it cannot update the
task, record approval evidence, or run post-write persistence.

When the outcome is ready for a decision, the browser presents the task,
readiness rationale, and exact proposed outcome in a decision card. The person
may approve it, revise the text and request a new review, or keep working.

Approval is a new deliberate request. The browser sends the opaque review
identity returned by Exo and echoes the exact reviewed task and outcome. Those
echoed values are replay material, not approval authority: Exo uses them to
reconstruct the canonical request identity for a terminal ledger lookup. A new
execution still requires the server-held review to match every echoed value.
The browser does not construct a workflow confirmation. Exo creates the
canonical task completion confirmation internally and commits approval
evidence and task completion through its normal atomic path.

Revising the outcome creates a new review request and a new review identity.
Keeping the task open dismisses the card locally and sends no command. Review
records are session-local. They do not survive a daemon replacement or browser
session expiry, and they do not form a durable queue of things that supposedly
need the person's attention. Each session retains at most 32 transient reviews.
When that bound is reached, Exo evicts the oldest consumed review first and
then the oldest remaining review, together with its request-replay record.
The host also admits at most 32 concurrent completion-review evaluations.
Additional reviews receive a retryable busy result before entering the blocking
readiness path.

## Reference-Level Design

### Authority boundary

The workbench continues to run inside the project daemon defined by RFC 10203.
The launch ticket selects a validated workspace and grants named
capabilities. The browser session retains that workspace identity; callers
cannot supply a path, project ID, phase owner, or command address.

Planning requests enter the same daemon dispatcher as CLI, MCP, and editor
requests. The adapter maps each browser operation to a fixed Exo operation and
constructs the `RequestEnvelope` with the session's canonical workspace root.
The browser never opens SQLite, writes Markdown or SQL projections, or
persists an optimistic plan.

Every planning operation is independently capability-gated:

```text
workbench.task.add
workbench.task.update
workbench.task.reorder
workbench.task.start
workbench.task.log
workbench.task.complete.review
workbench.task.complete.approve
```

A session with snapshot or lane-focus authority does not implicitly receive
planning authority. There is no wildcard task capability and no fallback to
command text.

### Version-two command request

Snapshot and lane focus remain available through the version-one request
defined by RFC 10203. Planning uses a version-two request:

```typescript
interface WorkbenchPlanningRequestV2 {
  protocol_version: 2;
  id: string;
  session_key: string;
  expected_daemon_instance_id: string;
  expected_revision: number;
  expected_phase_id: string;
  operation: WorkbenchPlanningOperation;
}

type WorkbenchPlanningOperation =
  | {
      kind: "task_add";
      goal_id: string;
      title: string;
    }
  | {
      kind: "task_update";
      task_id: string;
      title: string;
    }
  | {
      kind: "task_reorder";
      task_id: string;
      position: number;
    }
  | {
      kind: "task_start";
      task_id: string;
    }
  | {
      kind: "task_log";
      task_id: string;
      message: string;
    }
  | {
      kind: "task_complete_review";
      task_id: string;
      outcome: string;
    }
  | {
      kind: "task_complete_approve";
      review_id: string;
      task_id: string;
      outcome: string;
    };
```

`expected_daemon_instance_id` and `expected_revision` are copied together from
the snapshot that rendered the control. `expected_phase_id` is that snapshot's
focused phase. All identifiers are opaque values copied from the snapshot; the
browser does not parse goal-qualified task IDs.

This version number belongs to the browser command protocol. The adapter still
constructs an internal `RequestEnvelope` at Exo's current machine-protocol
version; browser clients never select or override that internal version.

Titles are trimmed, must contain at least one non-whitespace character, may not
contain a line break, and may contain at most 512 UTF-8 bytes. Progress messages
and outcomes must contain at least one non-whitespace character and may contain
at most 16 KiB of UTF-8. Their internal whitespace and line endings are part of
the canonical value shown for review and stored by Exo. The existing 64 KiB
HTTP request-body limit remains the outer transport bound.

`position` is a zero-based index within the task's current goal after removing
that task from the ordered list. The server rejects an index beyond the final
valid insertion point. The operation cannot move a task between goals.

### Mapping to Exo operations

The adapter maps the closed browser operations to existing task behavior:

| Browser operation | Canonical Exo behavior |
| --- | --- |
| `task_add` | `task add <title> --goal <goal_id>` with no explicit task ID |
| `task_update` | `task update <task_id> --title <title>` |
| `task_reorder` | `task reorder <task_id> <position>` |
| `task_start` | `task start <task_id>` |
| `task_log` | `task log <task_id> --message <message>` |
| `task_complete_review` | non-mutating task completion readiness evaluation |
| `task_complete_approve` | `task complete` with a server-constructed canonical workflow confirmation |

Task addition lets Exo derive the task ID from the title and returns the
canonical ID. Editing a title never renames that ID. Completed, abandoned,
skipped, and other non-actionable tasks are read-only through this protocol.
Starting is valid only for a pending task, progress logging and completion
review are valid only for an in-progress task, and reorder or title editing is
valid for a pending or in-progress task. The server validates these
transitions; hiding a control in the frontend is not an authorization check.
The focused phase must also be unowned or owned by the current workspace's
phase owner. A phase owned elsewhere remains visible as shared project context,
but its planning controls are read-only in this workspace.

`task_start` changes authoritative Exo planning state. It does not dispatch,
notify, or resume an agent host. The cockpit labels this action as marking the
task active in Exo and reports that agent handoff remains separate. A future
agent-launch contract may connect that state transition to Codex or another
agent host, but this protocol does not imply that execution began merely
because a task became active.

The goal must belong to the expected phase and permit new pending work. Every
task operation must resolve one unambiguous task in that same phase. Cross-goal
move, task removal, task-ID rename, and mutation of completed tasks are
excluded.

### Atomic stale-view guard

The workbench revision is a daemon-generation counter. A compatible daemon
replacement may restore a browser session, but it does not carry the prior
generation's revision forward. Every snapshot therefore carries the current
daemon instance ID, and every planning request binds its revision to that
instance. A terminal same-request-ID outcome is replayed before precondition
validation; otherwise a request from a prior instance is stale even when the
replacement's counter has reached the same numeric revision. During
reconnection the browser also disables new mutations, renews the durable
grant, and loads a fresh authoritative snapshot.

Within one daemon generation, every `AtomicProjectState` write enters a
revision gate. The gate serializes the following sequence:

1. compare `expected_daemon_instance_id` and `expected_revision` with the
   current workbench instance and revision when the request carries a
   workbench precondition;
2. verify that this workspace still focuses `expected_phase_id`;
3. resolve the goal or task and verify that it belongs to that phase;
4. execute or roll back the existing `AtomicProjectState` transaction;
5. publish the next revision for any committed `Effect::Write` response,
   including a committed write whose post-write finalization reports an error.

All goal, task, lane, and phase-focus writers used by this planning protocol
are `AtomicProjectState` operations and pass through the gate whether they
originate in the browser, CLI, MCP, or an editor adapter. Commands that own
filesystem, Git, process, or other external effects are not exposed by this
protocol; they retain their existing recovery boundary and still publish a
revision after a committed write. The direct-write ownership fence prevents an
independent process from silently changing canonical state outside daemon
authority.

The revision comparison and focus/entity checks happen before mutation while
the gate is held. The SQLite membership and lifecycle checks are repeated
inside the canonical transaction. The gate remains held through commit or
rollback and revision publication. This prevents another client from
committing between a successful precondition check and the browser write.
Snapshot reads use the same gate only while capturing the revision and one
transactionally consistent SQLite view. Workspace Git metadata is sampled
before entering the gate, so branch and dirty-state inspection cannot delay
unrelated canonical writes.

A completion review first enters a bounded host admission lane, then acquires
the same revision gate while it compares revision, validates focus, and reads
readiness evidence. It releases the gate without incrementing the revision
because review cannot mutate state. This gives the returned review one coherent
project-state boundary without treating it as a write or allowing review work
to grow without bound.

A revision mismatch, focus mismatch, entity outside the phase, invalid task
transition, or invalid destination commits nothing. A stale response tells the
client to refresh and does not claim that retrying the unchanged gesture is
safe.

### Request identity and recovery

Each deliberate user action receives one request ID. The client keeps the
request ID and byte-equivalent payload until that intention reaches a terminal
result.

If delivery is ambiguous or Exo returns an explicit retryable busy result, the
client retries with the same request ID and payload. Exo's request ledger
remains responsible for replay, conflict detection, and recovery. The browser
does not invent a replacement request ID merely because the response was lost.

A stale-view result is terminal for that request. After refreshing, applying
the gesture again is a new intention with a new revision and request ID.
Changing an input also creates a new request ID.

Completion review and completion approval are separate intentions. They always
have different request IDs. Retrying either one preserves its own ID.
For approval, Exo checks both the runtime request ledger and canonical atomic
outcome before consulting the transient review cache. A runtime terminal
response replays directly. A canonical outcome whose runtime response was lost
re-enters the dispatcher only to replay the committed core and finish
post-commit persistence. This lets a committed approval recover after daemon
replacement or review eviction without allowing an evicted review to authorize
a new completion.

### Non-mutating completion review

The completion implementation exposes its readiness evaluator without calling
the mutating completion path. Review resolves the canonical task, verifies the
expected phase and task state, reads completion claims and outcome evidence,
and constructs the canonical workflow-completion review. It cannot update the
task or add approval evidence even when earlier evidence would allow the normal
completion command to proceed.

The browser adapter projects only this safe result:

```typescript
interface WorkbenchTaskCompletionReview {
  kind: "workbench.task_completion_review";
  ok: true;
  schema_version: 1;
  review_id: string;
  task_id: string;
  readiness_rationale: string;
  proposed_outcome: string;
  approval_evidence_present: boolean;
}
```

It does not expose raw confirmation inputs, branch instructions, steering,
commands, agent identifiers, local paths, internal evidence IDs, traces, or
arbitrary nested failure details.

`review_id` is a random, opaque, session-bound identity. The host stores the
canonical task ID, exact outcome, expected phase, reviewed revision, and
canonical review result under that identity. Replaying the same review request
ID in the same session returns the same review identity and content.

An approval operation that has no terminal ledger outcome succeeds only when
the review belongs to that session, the echoed task and outcome exactly match
the server-held review, the expected phase and revision still match the
reviewed values, and the task is still eligible for completion. The server
constructs the internal value:

```rust
WorkflowConfirmationInput {
    kind: "workflow_completion_confirmation",
    entity_type: "task",
    entity_id: reviewed.task_id,
    decision: YesComplete,
    outcome: reviewed.proposed_outcome,
}
```

The browser cannot alter those fields for a new execution: any mismatch with
the server-held review is rejected. Approval evidence and task completion
commit together through the existing atomic request boundary. The review
record is consumed after a terminal successful approval; an ambiguous
transport outcome is retried with the same approval request ID and exact echoed
task and outcome so the outcome ledger can replay the commit before transient
review lookup.

### Browser-safe results and failures

Successful task mutations return a small acknowledgement containing the result
kind, operation, and canonical task ID. Completion approval returns the
canonical task ID. The browser does not patch its snapshot from these
acknowledgements; it refreshes after the write notification or successful
response.

Known planning failures are projected into stable, path-free browser kinds:

| Kind | Meaning | Client behavior |
| --- | --- | --- |
| `workbench.stale_snapshot` | Daemon generation or revision no longer matches | Refresh and ask for a new intention |
| `workbench.phase_mismatch` | Workspace no longer focuses the expected phase | Refresh; do not retry unchanged |
| `workbench.entity_outside_phase` | Goal or task is not in the expected phase | Refresh and discard the stale target |
| `workbench.invalid_transition` | Task state does not allow the operation | Refresh and explain the current state |
| `workbench.invalid_input` | Text or position is invalid | Keep editable input and show the validation |
| `workbench.review_invalid` | Review is absent, expired, consumed, or does not match | Request a new completion review |
| `workbench.busy` | Bounded daemon admission rejected the request | Retry the same request ID and payload |

Session, workspace, capability, and transport failures keep the contract from
RFC 10203. Unknown internal failures remain generic. No browser response
contains a workspace root, SQLite path, raw command, ticket, cookie, sidecar
location, or arbitrary Exo error details.

### Client state machine

The browser keeps at most one in-flight mutation per task and disables controls
that would produce a conflicting intention. An add-task request belongs to its
goal; reorder controls for that goal wait until it resolves. Unrelated goals
may remain interactive, although Exo may conservatively reject their request
after another write advances the revision.

The client keeps an input draft separate from the snapshot and captures the
daemon instance, revision, and focused phase when the editor opens. A later
refresh may update the visible plan, but submitting that draft retains its
opening binding so Exo can reject it as stale instead of applying old text over
a collaborator's change. A successful write clears its submitted draft after
the authoritative refresh. A stale response preserves the text and explicitly
rebinds that editor only after the refreshed plan arrives, allowing the person
to apply it again as a new intention. If refresh itself enters session
recovery, rebinding remains pending until recovery applies an authoritative
snapshot. When that snapshot shows that the task can no longer accept the
action, the workbench preserves the draft for inspection but makes the editor
read-only instead of offering a request that Exo must reject. Other invalid
responses preserve useful text. A transport failure keeps both the payload and
request ID for retry.
An approval retry remains attached to its completion review when the approved
write's own invalidation refreshes the snapshot before the original response is
read.

The completion card stores only the browser-safe review result. `Approve`
dispatches its review identity with the exact task and outcome Exo returned.
`Revise` restores the exact proposed outcome to the editor and invalidates the
visible card before issuing a new review. `Keep working` dismisses the card
without a request.

Planning controls are enabled only when the snapshot has one focused lane, that
lane belongs to the displayed phase, both lane phase and phase are in progress,
the phase is unowned or owned here, and no focus-mismatch diagnostic is present.
The snapshot exposes only the resulting `planning_available` boolean, not phase
owner identity or a workspace path. A missing, incoherent, or foreign-owned
focus keeps the plan readable but non-mutating, and the daemon repeats the
ownership check before admitting review or mutation.

An event-stream failure closes that stream and retries it with bounded
exponential backoff while ordinary snapshot polling remains active. Reaching
the stream's `ready` event resets that backoff. A transport or
unreadable-response failure from snapshot polling enters session recovery. The
last authoritative snapshot stays visible, but all mutations pause while the
client renews the session and obtains a fresh snapshot. Recovery invalidates
snapshot requests that began under the prior connection so a delayed response
cannot replace the recovered view. Replacing the browser session clears notices
and pending transient planning state from the prior session. A session that
cannot be restored asks for a current launch instead of retrying expired
credentials.

### Linked worktrees

The browser session remains bound to one validated workspace root. Lane and
phase focus are workspace-local; task and goal state are shared project state.

A planning mutation from one linked worktree therefore changes the canonical
plan and invalidates snapshots in compatible sessions, but it cannot change the
sibling worktree's focused lane or active phase. The sibling refreshes its
shared task data within its own focused context. If its context no longer
contains the changed entity, it does not acquire the issuing workspace's focus.

### Compatibility and rollout

`WorkbenchSnapshot` uses schema version 2 after RFC 10203's between-phase
trajectory addition. This RFC does not introduce another snapshot version.
Goal-progress presentation is derived in the client from existing task
statuses. Each task may also carry an additive `progress` collection containing
the message and timestamp of canonical progress logs plus an additive
`progress_truncated` marker when the bounded recent window omits canonical
history. The focused phase also carries the additive `planning_available`
boolean derived from phase ownership; clients that do not understand it remain
orientation-only rather than assuming write authority.

The host accepts version-one snapshot and lane-focus requests while adding
version-two planning requests. Older embedded clients continue to orient and
focus lanes. New clients enable a planning control only when the session grants
its exact capability. There is no database migration and no compatibility
fallback to a generic command endpoint.

## Security and Privacy

The planning surface has more authority than the focus-only workbench, but it
does not widen the transport boundary. It remains loopback-only, protected by
the host and origin checks, independent session selector and cookie, validated
workspace binding, fixed capabilities, request-body limit, and path-free
responses from RFC 10203.

Browser input is data, never command text. IDs are resolved against the
expected phase rather than interpolated into a shell command. The server
constructs every Exo address and every workflow confirmation. Review identities
are random, session-local, and useless without the independent authenticated
session. The task and outcome echoed for approval can identify only an already
committed terminal ledger entry; they cannot bypass the matching live review
required for a new execution.

## Scope

This RFC covers task planning inside an already focused lane whose phase is
in progress. It covers adding tasks under existing goals, editing titles,
reordering within one goal, starting tasks, recording progress, non-mutating
completion review, exact-outcome approval, stale-view rejection, request
recovery, and the progress presentation needed to make partial goals legible.

It does not add goal creation or reorganization, cross-goal task moves, task
removal or ID rename, lane or phase lifecycle controls, RFC management,
pull-request actions, validation history, attachments, remote access,
multi-user identity, or cloud hosting.

Completed-phase lanes remain visible but cannot be focused. Read-only historic
lane inspection is a later interaction. Pull-request identity as the primary
workspace label requires new snapshot evidence and is also deferred. Launch
ticket lifetime and relaunch behavior remain governed by RFC 10203 and will be
reconciled separately.

This RFC does not introduce a human-attention queue. Agent guidance remains
secondary context, and a completion decision exists in the browser only as the
direct result of the person's own action.

## Drawbacks

The closed API duplicates a small amount of task vocabulary at the browser
boundary. That duplication is intentional: it makes browser authority legible
and reviewable, but each new operation requires coordinated Rust, TypeScript,
capability, and test changes.

The daemon revision gate adds ordering around project-state writes that SQLite
already serializes at a lower level. That explicit ordering is the cost of
making the state the person saw part of the write contract instead of a
best-effort frontend check.

Conservative revision checks may interrupt an edit after an unrelated write.
The workbench can preserve draft text and make recovery gentle, but it should
not weaken the stale-state guard to feel more optimistic.

The completion decision card is useful without being durable. A person who
reloads must request the review again. Persisting pending decisions would
produce a smoother experience, but doing so before Exo has a real
human-attention model would turn transient UI state into accidental workflow
policy.

## Alternatives

### Keep planning in the agent or CLI

This preserves the smallest browser surface, but it keeps the human and agent
from shaping the plan where they can already see it together. The focus-only
workbench would remain an orientation endpoint rather than becoming a shared
working surface.

### Expose a general `exo-run` endpoint

A generic endpoint would make every present and future command available
without browser-specific code. It would also erase the capability boundary,
force the frontend to understand heterogeneous effects and confirmations, and
make local command syntax part of the browser protocol. This RFC rejects that
tradeoff.

### Check revision only in the browser host

A host-local preflight appears simpler, but another client could commit after
the check and before the task transaction. The person would then mutate a plan
other than the one they saw. The daemon revision gate exists to close that
window.

### Let completion review invoke the mutating command

The current completion command often returns a structured review before
mutation. Existing approval evidence can also make that command eligible to
complete immediately. A review endpoint must never rely on the absence of such
evidence for its non-mutating guarantee, so this RFC extracts readiness
evaluation instead.

### Build a browser-owned plan and synchronize later

Optimistic local planning could feel immediate and support offline editing. It
would create a second authority, complicate linked-worktree behavior, and
weaken Exo's atomic recovery contract. The workbench should instead make
authoritative writes clear and responsive.

### Add a durable attention queue first

A durable queue could unify task approvals, diagnostics, review comments, and
agent questions. The project does not yet have an evidence-backed model for
those different obligations. Completion review is narrow enough to expose now
because Exo already defines its transition and confirmation contract.

## Validation

The implementation is ready for candidate reconciliation only when the
following evidence is green:

- shared Rust and TypeScript fixtures accept every version-two operation and
  reject unknown operations, excess fields, invalid bounds, and missing
  capabilities;
- command tests prove each browser operation maps to the intended Exo behavior
  without accepting command text or caller-supplied paths;
- concurrent daemon tests prove the generation/revision/focus/entity guard and
  atomic mutation are one ordered boundary for browser, CLI, MCP, and
  linked-worktree writers;
- request-ledger tests prove ambiguous retries preserve request ID, payload,
  completion review identity, and exactly-once approval;
- completion tests prove review is non-mutating and bounded, approval is bound
  to the exact reviewed task and outcome, terminal approval replays before
  transient review lookup, revised outcomes require a new review, and stale
  reviews cannot complete a task;
- browser tests cover task editing, add, reorder, start, progress, review,
  approval, stale refresh, opening-snapshot draft binding, post-rejection
  rebinding, superseded-refresh rejection, immediate transport recovery,
  session reset, retry, keyboard operation, and accessible status names;
- view tests distinguish untouched goals from partial progress, remove
  redundant visible task-status labels, and verify balanced wrapping at narrow
  and wide layouts;
- disposable linked-worktree acceptance proves shared task updates, isolated
  workspace focus, SSE invalidation, polling freshness, and unchanged sibling
  focus;
- normal Exo persistence, outcome-ledger, daemon, cockpit check, build, and
  static validation suites remain green.

Browser inspection covers desktop and mobile widths without launching native
macOS VS Code. The locald development proxy remains machine-local and outside
the repository.

## Unresolved Questions

No unresolved question changes the first implementation boundary. Dogfooding
may change the placement of compact task actions or the wording of progress
metadata, but it may not widen the operation set, weaken the revision guard,
make completion review mutating, or turn transient decisions into a durable
attention queue without another design review.
