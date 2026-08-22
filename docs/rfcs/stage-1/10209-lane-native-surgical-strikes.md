<!-- exo:10209 ulid:01m0k862838wn0s389y6e1zqpt -->

# RFC 10209: Lane-native surgical strikes

**Feature**: lane-native-surgical-strikes

## Summary

Surgical strikes are how bounded fixes discovered through dogfooding can
interrupt forward work without causing the project to lose the larger
direction of its current work.

Exo already represents a surgical strike as a goal. This RFC preserves that
foundation and adds the continuity that becomes necessary in a lane-centered
workbench. A strike temporarily becomes the visible locus of execution while
retaining an exact account of what it interrupted and where work should return.
It has a concrete trigger, a small plan, a stopping condition, proof against
that condition, an exact resume context, and an explicit return to the prior
work.

The central distinction is:

- a **lane** is durable continuity for a stream of work;
- a **campaign** is a bounded planned delta within that stream; and
- a **surgical strike** is a temporary execution overlay on the current lane
  and campaign.

A strike is therefore neither a new lane nor a new campaign. It should be
visible enough that the user understands why the plan changed, bounded enough
that it cannot quietly become a second roadmap, and continuous enough that the
workbench can return to the prior task without reconstructing intent from chat
history.

## Motivation

### Dogfooding should be allowed to change the route

The workbench is meant to become a shared perception surface for the user and
the agent. That promise becomes weaker if problems found through real use are
always deferred behind the feature plan that exposed them. A noisy project
surface, misleading status, broken loading state, or unsafe interaction may be
small compared with the campaign in progress, but it can make continued
dogfooding materially worse. Treating every such finding as unrelated cleanup
teaches the project to ignore its own product evidence.

At the same time, reacting immediately can damage continuity. Adding incidental
tasks to the current campaign obscures what the campaign was intended to prove.
Starting another campaign gives a bounded correction more weight than it
deserves. Replacing the current lane loses the durable identity of the work.
Handling the issue only in conversation leaves future agents unable to tell why
the plan changed or where they should return.

Surgical strikes exist for this middle case. They let the project say:

> This finding matters enough to interrupt what we are doing, but it does not
> replace what we are doing.

The strike makes that judgment durable. Its trigger explains why the
interruption is warranted. Its mini-plan and stopping condition keep the work
bounded. Its proof explains why the correction is sufficient. Its resume point
protects the continuity of the interrupted campaign.

### The existing strike model does not yet preserve continuity

RFC 10175 established the important implementation insight that a strike is a
goal. That gives strikes normal tasks, progress, completion evidence, and
history. It also avoids introducing a parallel work-item hierarchy.

That model does not currently retain the interruption around the goal. Exo can
know that a strike is active without knowing which lane, campaign, goal, and
task it interrupted; what observation caused the interruption; what condition
ends it; or what should become current afterward.

This gap is visible in current dogfooding. After a strike was started, `status`
and `task list` continued to show the interrupted storage task as working now.
The current implementation also omits goal kind from the workbench projection,
flattens steering into generic phase context, and permits ambiguous routing
when more than one goal is active. These are observed contributing gaps, not a
proven single explanation for the projection result. Together they show that
urgency alone is insufficient: a strike needs explicit execution context that
the workbench and command surface can project consistently.

## Experienced behavior

When a strike begins, the user should see a deliberate interruption rather than
an unexplained reordering of the plan.

In a workspace focused on the attached lane, the workbench presents the strike
as **working now**. It names the observation that triggered it, the bounded
outcome being pursued, and the condition that will permit return. The
interrupted lane and campaign remain visible underneath as paused continuity.
When a task was active, that exact task remains identifiable, but it is no
longer presented as the action currently underway.

The strike is also visible project-wide as an exceptional shared priority.
That visibility does not capture ambient execution in workspaces focused on
other lanes. Those workspaces continue to present their own lane as working
now and can inspect the strike. To participate, a workspace must explicitly
join or focus the attached lane and satisfy the attached campaign's ordinary
ownership rules. Focusing the lane does not itself grant that ownership.

The strike uses normal goals and tasks. Progress updates describe the
correction as it happens. Completion requires a reviewed outcome that evaluates
the declared stopping condition rather than a generic assertion that the strike
is done.

When the strike finishes, Exo records its outcome in history and restores the
prior execution context as working now in workspaces focused on the attached
lane. When a task was active, that context identifies the exact task. Between
tasks, it identifies the campaign and any current goal without nominating an
unrelated pending task. Lane focus does not move away and then move back; the
durable lane remained focused throughout, and sibling focus was never
rewritten. If the prior task, goal, or campaign is no longer a valid resume
target, Exo stops and asks for an explicit choice rather than guessing.

The result should feel like a focused interruption inside one coherent stream
of work, not like an invisible detour and not like a second project plan.

## Relationship to existing RFCs

### RFC 0048: Surgical Strike Workflow

RFC 0048 identified the continuity problem and described a push, execute, and
pop workflow. Its phase-stack design is obsolete: phases are not call frames,
and nested phases would introduce another execution hierarchy alongside lanes,
campaigns, goals, and tasks.

This RFC recovers the durable insight without reviving the mechanism. A strike
does need to remember what was interrupted and return to it. It does not need a
stack of phases. RFC 0048 remains superseded.

### RFC 10175: Surgical Strikes as Goals

RFC 10175 remains the implementation foundation. A strike is still a goal with
strike-specific activation, presentation, and steering behavior. The
project-wide singleton, bypass of ordinary scheduling, ordinary task model, and
durable strike history remain part of the design.

This RFC prospectively refines that model by separating the **strike goal**
from its **interruption context**. The goal describes the work. The interruption
context describes why it displaced the current work, what it displaced, what
will end the interruption, and where execution should return.

Two RFC 10175 behaviors are prospectively superseded. First, `strike finish`
must not immediately write a generic completion; completion proceeds through a
normal reviewed goal outcome evaluated against the strike's stopping
condition. Second, urgency may bypass ordinary scheduling, but it may not
bypass an upgrade, storage-compatibility, ownership, approval, or safety gate.

### RFC 10202: Lane-Centered Workbench Adoption

RFC 10202 makes lanes the durable identity for streams of work and makes focus
workspace-local. A strike attaches to the lane focused in the workspace where
the interruption is started, but it does not replace that focus. Other linked
worktrees continue to share the lane, campaign, and strike records while
retaining their own workspace focus and ambient execution context.

This matters for recovery: returning from a strike changes the working-now
projection within the interrupted context. It does not rewrite workspace lane
focus as a side effect.

## Relationship to project-flow work

The project-flow work currently under development distinguishes durable lanes
from bounded campaigns and treats the cockpit as a projection of the project's
decision and delivery motion. This proposal does not depend on that unpublished
RFC as current authority. It states the smaller distinction it needs here: the
campaign remains the planned delta, while the strike records why execution
temporarily departed from that plan and how it returned.

In the current compatibility model, a phase is the implemented campaign
boundary. This RFC uses campaign for the product concept while preserving the
phase association required by current Exo planning and ownership semantics.

## Detailed direction

### A goal plus interruption context

The strike goal continues to own the familiar work-item behavior:

- its title and intended outcome;
- its mini-plan of tasks;
- progress and verification evidence;
- its reviewed outcome; and
- its completed or aborted history.

The interruption context owns the continuity behavior:

- the concrete observation or event that triggered the strike;
- the interrupted lane and campaign;
- the interrupted goal and task when one is current;
- the stopping condition that bounds the interruption;
- the strike lifecycle state; and
- the return disposition, including any reason automatic return was unsafe.

These are two views of one strike, not two independently managed objects. A
strike cannot be active without its goal. New lane-native strikes cannot claim
to preserve continuity without valid interruption context.

The exact storage representation and migration shape remain part of the
detailed design. The context itself must be durable, authority-aware, and
readable by every adapter that can present working-now state.

### Scope and singleton behavior

The first lane-native model preserves the project-wide singleton from RFC
10175. Only one surgical strike may be active for the project at a time. That
strike is attached to one interrupted lane and campaign and is globally visible
as a project-level priority.

Project-wide priority does not mean project-wide ambient task routing. Ambient
execution and the **working now** projection remain lane- and workspace-scoped.
A workspace focused on the attached lane routes ambient work to the strike. A
workspace focused on another lane continues to route ambient work to that lane.
It may inspect the active strike, but it executes strike work only after an
explicit workspace transition to the attached lane under valid campaign
ownership. Lane focus alone does not transfer that ownership.

Project-wide singleton behavior is intentionally conservative. A strike means
that the project's normal execution priority has been interrupted. Allowing
one strike per lane would make several items simultaneously claim exceptional
priority and would complicate the shared workbench before ordinary strike
continuity has been proven.

This constraint may later be revisited for genuinely independent agents and
lanes. Such a change should be driven by observed concurrency needs rather than
included speculatively in the first contract.

While a strike is active, the attached campaign cannot complete or otherwise
leave its executable lifecycle through ordinary campaign operations. Such an
operation fails with a stable explanation that the strike must first finish or
abort. This preserves the campaign authority required to record the strike's
reviewed outcome and return disposition; it does not expand the strike's own
mutation authority.

### Choosing the resume point

Starting a strike records an exact resume context. Exo may infer the current
task only when there is exactly one valid task that the current workspace and
lane would otherwise present as working now. If the active context is
ambiguous, the caller must name the task explicitly. If no task is active, Exo
records a taskless resume disposition anchored to the exact lane and campaign,
and to the current goal when there is one. Return then restores that context and
re-evaluates working now; it does not select an unrelated pending task.

This rule keeps convenience from becoming guesswork. A branch name, recent
timestamp, array position, or conversational mention is not sufficient
authority to select the work that will resume.

The resume target is a promise to re-evaluate, not an instruction to force old
state back into place. At return time Exo verifies that the lane, campaign,
goal, and task still form a coherent execution path. If another workspace or an
approved mutation completed, removed, or superseded that path, the strike can
still finish, but automatic return becomes blocked pending an explicit user
choice.

At minimum, automatic return is blocked when the target task has completed,
been removed, or moved to another campaign; when its goal or campaign is no
longer executable; when the lane no longer contains that campaign; or when a
current ownership, compatibility, approval, or safety condition forbids the
return. Changes that leave the exact execution path intact, such as progress
recorded on the target or work performed in a sibling task, do not by
themselves make the resume point stale. Stage 2 must define the revision or
generation evidence used to make this check without a race.

### Lifecycle

#### Start

Starting a lane-native strike requires an active lane and campaign, a concrete
trigger, a proposed outcome, a stopping condition, and an unambiguous resume
context. The context may be taskless when execution is between tasks. Exo
creates the strike goal and its interruption context as one authoritative
operation. Lane focus remains unchanged.

Once started, the strike becomes the working-now locus for steering, task
routing, status, and the cockpit in workspaces focused on its attached lane.
The interrupted task remains in its existing lifecycle state; Exo does not mark
it incomplete, blocked, or abandoned merely because execution has paused.
Starting the strike does not rewrite sibling workspace focus or redirect the
ambient work of another focused lane.

#### Execute

The strike receives a deliberately small task plan. In a workspace focused on
the attached lane, commands that operate on the current work route to the
strike only when the strike has supplied a unique current task; otherwise they
require an explicit strike task selector. A workspace focused on another lane
must first join or focus the attached lane under valid campaign ownership. This
avoids the current ambiguity in which a strike goal and campaign goal may both
be active while unqualified task commands still resolve to the campaign.

Within an authorized attached-lane workspace, an explicit strike task selector
is the exact identity of a task belonging to the active strike goal. Naming the
task disambiguates routing; it never substitutes for lane, campaign, workspace,
or ownership authority.

The trigger and stopping condition remain visible throughout execution. New
findings may refine the strike plan, but broadening the work beyond the stopping
condition requires a deliberate change in scope rather than quiet task growth.

#### Review

A strike reaches review when its mini-plan is complete and evidence exists for
the stopping condition. Review asks whether the bounded correction achieved
what justified the interruption and whether returning to the prior work is now
reasonable.

Urgency does not reduce this evidence standard. A strike may be short, but its
outcome should be at least as truthful as an ordinary goal outcome because the
strike displaced approved work.

#### Finish and return

Finishing uses the normal reviewed goal-outcome path. A successful outcome
records the proof, closes the strike goal, and attempts to restore the exact
resume context. The strike moves into history, where it remains attached to the
lane and campaign whose execution it interrupted.

If the resume point is still valid, it becomes working now again in workspaces
focused on the attached lane. If it is stale, Exo records that return requires
attention and presents the relevant choices. It does not silently select the
next pending task. The attached workspace's lane focus never moved, so return
changes its working-now overlay rather than moving focus back. Finishing does
not rewrite sibling workspace focus.

Once the reviewed finish is recorded, the strike is no longer active and no
longer occupies the project-wide singleton. A blocked return remains a visible
attention item attached to the completed strike; it does not keep the project
inside an otherwise finished interruption.

The reviewed outcome, inactive strike lifecycle, singleton release, and durable
return disposition form one recoverable authority transition. An implementation
may commit them atomically or use a durable intermediate state whose retry
finishes the transition, but it must never expose a completed strike without a
recorded return or blocked-return disposition.

#### Abort

Aborting records why the strike stopped without satisfying its stopping
condition. Abort is not erasure: the trigger, attempted work, evidence, and
return disposition remain part of project history.

An abort may still return to the prior task when that target remains valid.
When returning would ignore an unresolved safety or ownership condition, Exo
holds the return for explicit resolution.

Recording the abort ends the active strike and releases the project-wide
singleton even when its return disposition still needs attention. Starting a
later strike must not erase or implicitly resolve that earlier disposition.
The abort state, singleton release, and return disposition follow the same
recoverable transition rule as successful finish.

### Cockpit semantics

The cockpit should make the interruption legible at a glance without implying
that every workspace has abandoned its own lane.

In a workspace focused on the attached lane, the principal execution surface
shows:

- that a surgical strike is working now;
- the trigger in the language of the observed product problem;
- the strike's intended outcome and stopping condition;
- its current task and bounded plan;
- the interrupted lane and campaign, plus the resume goal and task when they
  exist; and
- whether return is currently expected to be automatic or requires attention.

Project-level surfaces expose the active strike as a globally visible priority,
including its attached lane and current state. In a workspace focused on a
different lane, that project-level signal remains inspectable while the
principal execution surface continues to show the local lane as working now.
The cockpit offers an explicit route to join or focus the attached lane; it
does not silently redirect the workspace merely because a strike exists.

The interrupted plan remains visible as background continuity, but it should
not compete visually with the strike for current status. The user should be
able to understand both "what is the agent doing?" and "what were we doing
before this?" without opening a diagnostic inspector.

After completion, the strike becomes compact lane history. It explains a
meaningful deviation in the execution narrative without permanently crowding
the current plan. Historical presentation should retain the trigger, outcome,
proof, duration, and return destination.

This RFC defines the information hierarchy rather than a particular responsive
layout. Exact controls, dimensions, and collapse behavior belong in Stage 2 and
the cockpit implementation.

## Linked worktrees and authority

Lane focus and ambient execution remain workspace-local, while the active
strike and its goal are shared project state. The first model attaches the
project-wide strike to the lane and campaign from the workspace that starts it.
Every worktree can observe the strike as a project-level priority. Workspaces
focused on that lane route ambient work to the strike. Workspaces focused on
another lane continue their own ambient work and may execute the strike only
after explicitly joining or focusing the attached lane under valid campaign
ownership.

Observation is presentation, not routing: seeing the strike in a project-level
surface never changes a workspace's current lane, task resolution, or mutation
authority.

Starting and finishing the strike never rewrite sibling focus. A sibling that
has explicitly joined the attached lane participates under ordinary lane and
task authority; mere project-wide visibility does not grant execution
authority.

Starting, changing, reviewing, finishing, or aborting a strike must obey the
same project, campaign, and write-ownership rules as the corresponding ordinary
work. A strike's urgency changes prioritization only. It never bypasses:

- storage writer compatibility or migration fences;
- project, phase, sidecar, or workspace ownership;
- request replay and exactly-once protections;
- approval gates;
- validation requirements;
- runtime or process-recovery authority; or
- any safety boundary that would apply outside a strike.

This prospectively supersedes RFC 10175's older statement that strikes bypass
an upgrade gate. A lane-native strike may bypass ordinary scheduling, but it
does not gain broader mutation authority. Any implementation behavior that
treats urgency as permission to cross a compatibility or safety gate must be
reconciled before this RFC can reach implemented status.

Linked worktrees also make stale resume points possible. The return check must
use authoritative current state rather than assuming that the interrupted
context remained untouched while the strike ran.

## Compatibility

Existing strike goals remain valid goals. Migration must not invent triggers,
stopping conditions, interrupted lanes, or resume tasks that were never
recorded.

Completed legacy strikes remain historical strike goals with an explicit
legacy presentation. Active legacy strikes remain finishable or abortable. They
may acquire interruption context only through an explicit user choice grounded
in current state; Exo should not imply that their prior continuity can be
reconstructed exactly.

The project-wide singleton remains compatible with RFC 10175. Existing goal
history, task records, and completion logs remain useful. This RFC adds the
context required for future strikes to participate truthfully in lane and
workbench projections.

Compatibility for old clients should fail honestly at mutation boundaries that
cannot preserve the new context. The exact schema-generation and mixed-binary
policy belongs to the storage compatibility contract and the Stage 2 design;
this RFC does not create an urgency exception to it.

## First organic proof

The first organic proof should be the dogfood finding that prompted this RFC.

In the locald project, Git reports 57 registered worktrees. Read-only
reconciliation found only two existing paths, while `git worktree prune
--dry-run --verbose` identified the other 55 registrations as prunable stale
metadata. The workbench currently projects those registrations as ordinary
workspaces, creating a noisy project surface that makes active work harder to
see.

The strike is not permission to prune locald. Its bounded outcome is to make
Exo truthfully distinguish or collapse prunable registrations so that the
workbench presents the two existing worktrees as the current project structure.
The first implementation must not run `git worktree prune` or otherwise mutate
locald metadata. Any later reconciliation command remains a separate,
explicitly authorized operation.

The proof succeeds when ordinary dogfooding shows all of the following:

1. The measured worktree discrepancy appears as the strike trigger.
2. The current project-motion task remains visible as the exact resume point.
3. The strike becomes working now in the attached-lane workspace without
   changing that workspace's lane focus or any sibling focus.
4. The cockpit shows the strike's bounded plan and stopping condition.
5. Exo truthfully distinguishes or collapses the 55 prunable registrations
   while preserving the two existing worktrees and performing no silent prune.
6. The reviewed outcome records evidence from the real locald project.
7. Finishing the strike restores the exact project-motion task as working now.

This proof exercises both halves of the proposal. It improves the product in
response to real use, and it demonstrates that Exo can make such an
interruption without losing the work it was already advancing.

## Drawbacks

The proposal adds durable machinery around work that is intentionally small.
For a very short correction, recording a trigger, stopping condition, and
resume point may feel heavier than simply adding a task. That cost is real. A
strike should be used when the interruption itself needs explanation and
continuity, not as ceremony around every incidental fix.

Project-wide singleton behavior may be too restrictive as Exo supports more
independent lanes and agents. It is nevertheless a clearer first contract than
multiple simultaneous exceptional priorities.

The working-now overlay also makes status projection more sophisticated. Every
surface must distinguish durable lane focus, campaign planning, and temporary
execution priority consistently. Partial adoption would be worse than the
current model because different clients could disagree about what is current.

Finally, preserving an exact resume point exposes changes made by other
worktrees while the strike runs. Returning can therefore require user attention
instead of being automatic. That friction is preferable to silently resuming
the wrong work.

## Alternatives

### Add the correction to the current campaign

This is appropriate when the finding is part of the campaign's intended proof.
It is a poor fit when the correction exists to restore the quality of the
dogfooding surface rather than advance the planned delta. Folding every product
finding into the campaign makes the plan cease to explain its own scope.

### Create a new campaign

A new campaign gives the correction durable planning structure, but it also
turns a bounded interruption into a peer of the campaign it briefly
interrupted. Campaign sequencing does not by itself retain the interrupted task
or express an automatic return.

### Create a dedicated strike lane

A separate lane would give the strike a visible identity, but it would misuse
lanes as short-lived task containers and require focus to move away from the
durable stream that encountered the problem. The strike belongs in that lane's
execution history.

### Restore RFC 0048's phase stack

The stack expresses return clearly, but phases are planned campaign boundaries,
not execution frames. Reintroducing a phase stack would duplicate the
lane/campaign/goal hierarchy and make linked-worktree focus harder to reason
about.

### Keep strikes conversational

An agent can announce that it is pausing one task to fix another and later say
that it has returned. That works only while the same conversation remains the
authority. It does not give the cockpit, another agent, or a future session a
truthful shared account.

## Open design questions

The detailed design still needs to resolve:

- Which additional state changes should block automatic return, and which can
  be reconciled while preserving the exact execution path?
- How should an authorized user deliberately broaden or narrow a stopping
  condition after the strike begins?
- What retention and summarization policy keeps completed strikes useful as
  lane history without overwhelming the current plan?
- What evidence would justify moving from a project-wide singleton to scoped
  concurrent strikes?

These questions affect implementation precision. They do not change the
proposal's central direction: a strike remains a goal, overlays one interrupted
lane and campaign, and returns through an exact validated resume context.

## Proposal boundary

This RFC asks the project to adopt the following public contract:

- surgical strikes are bounded interruptions that preserve the larger direction
  of the active work;
- RFC 10175's strike-as-goal model remains the foundation;
- interruption context durably records trigger, stopping condition, interrupted
  execution context, and return disposition;
- the first model preserves one project-wide active strike, globally visible as
  a project priority and attached to one lane and campaign;
- ambient execution and working-now state remain lane- and workspace-scoped;
- attached-lane workspaces route ambient work to the strike, while other
  workspaces continue their lane and require an explicit transition to the
  attached lane under valid campaign ownership to participate;
- resume context always identifies the exact lane and campaign, includes the
  goal and task when they exist, and never invents pending work when execution
  is between tasks;
- resume-task inference is allowed only when the answer is unique;
- starting and finishing leave workspace lane focus, including sibling focus,
  unchanged;
- successful completion uses a normal reviewed goal outcome against the
  stopping condition;
- legacy strikes remain usable without invented continuity; and
- urgency never expands ownership, compatibility, approval, or safety
  authority.

The proposal does not yet choose a particular table layout, migration script,
command spelling, or cockpit geometry. Those details belong to the subsequent
detailed design, informed by the first organic proof against current code.
