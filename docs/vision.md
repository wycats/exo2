# Vision: Workspace-Centered Collaboration

Exo coordinates human and agent work around a durable project context. The
workspace is where work happens; project state preserves intent, progress, and
evidence across sessions, tools, worktrees, and machines.

The product promise is continuity. A collaborator should be able to open a
project, understand the work in progress, see what needs attention, and resume
from recorded state without reconstructing the project from chat history. The
same state should remain inspectable by the user, available to agents through
structured operations, and specific enough to support review.

## Collaboration Continuity

Software work spans conversations, editor sessions, worktrees, pull requests,
and machines. A useful collaboration system carries forward more than a task
list. It preserves:

- the intent that organized the work;
- the current focus and who may change it;
- observations and feedback that should affect the next action;
- validation evidence and review outcomes; and
- durable design decisions that future work can rely on.

Exo records these as project state and exposes them through a common operation
model. A new session can inspect that state, understand the current work, and
continue from evidence rather than memory.

## State Roles

The system separates several kinds of context because they have different
lifetimes and ownership rules.

| Role                    | Purpose                                                                                              | Current surface                                  |
| ----------------------- | ---------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| Canonical project state | Records epochs, phases, goals, tasks, RFC metadata, inbox items, focus, ownership, and review evidence | SQLite, read and mutated through `exo`           |
| Workspace view          | Identifies the work this worktree is looking at and the owner allowed to mutate phase-scoped state   | Worktree-aware focus and phase ownership         |
| Observations            | Carries user feedback, completion claims, concerns, and other signals into the next relevant action  | Exo inbox and steering surfaces                  |
| Durable documents       | Explains designs, specifications, research conclusions, and RFC decisions                            | `docs/design/`, `docs/specs/`, `docs/research/`, and `docs/rfcs/` |
| Portable projections    | Makes selected operational state portable under repo or sidecar policy                               | Generated SQL projections                        |

SQLite is the operational source of truth. Storage policy determines whether
the database and its generated projection belong to the repository, a sidecar,
or private machine-local state. Human-authored prose remains in the durable
document directories. The detailed ownership rules are defined in
[Agent Context Ownership](design/agent-context-ownership.md).

These roles let each surface answer a precise question. Project state says what
the project currently records. A workspace view says which part of that state
is in focus here. Observations say what collaborators should notice. Durable
documents explain why the system has its present shape.

## Bounded Work And Review

Exo organizes work into explicit scopes. Epochs and phases establish coherent
arcs of work; goals describe outcomes; tasks make the next executable steps
visible. Small urgent work uses the same model through strike goals, preserving
status and history while becoming the immediate focus. The amount of workflow
structure follows the size and urgency of the work.

Completion records an outcome, not merely the end of activity. When a task or
goal reaches completion review, Exo presents the proposed outcome for approval,
revision, continued work, or discussion. Approved outcomes become durable
evidence attached to the entity. The user can judge what was accomplished while
agents retain a structured account of the decision.

## Workspace Focus And Ownership

Focus and ownership are distinct. A worktree may focus a phase to inspect it,
while phase ownership determines whether that workspace or branch may mutate
the phase and its goals and tasks. This supports read-only inspection and
concurrent work without turning a shared active pointer into write authority.

Ownership is derived from the current Git context. Named branches can provide
portable branch ownership, while detached and agent-created worktrees use a
workspace identity. Status surfaces expose the derivation and identify local,
foreign, and stale ownership. Explicit release and takeover operations make
ownership changes visible.

The result is a workspace-specific view over shared project state:
collaborators can look at the same project while retaining clear mutation
boundaries.

## Shared Evidence

Human and agent collaboration improves when both sides can inspect the same
signals. Exo treats feedback and observations as scoped project data. Steering
summarizes relevant inbox items, and completion review brings evidence into the
decision point.

Validation follows the same principle. Exohook makes long-running checks
observable while they run and reports their outcomes. Reactive SQLite traces
connect observations of project state to later validation, so a consumer can
determine whether the state supporting an earlier result has changed.

These mechanisms give collaborators a shared account of what was observed,
what changed, and why an outcome is ready for review.

## Common Operations Across Surfaces

The `exo` command model is shared by the CLI and MCP transport. Editor and
workbench clients consume the same structured operations and read models rather
than defining separate project semantics.

Each surface can adapt presentation to its audience while preserving the same
entities, mutation rules, review boundaries, and structured results. Agents can
use MCP, developers can use the CLI, and visual clients can present project
state without creating parallel truths.

## Current Foundation

The current implementation provides:

- SQLite-backed canonical project state;
- shared project identity with workspace-specific worktree views;
- workspace-local phase focus and explicit phase ownership;
- scoped inbox signals and steering;
- task and goal outcome review;
- a common CLI and MCP command language;
- reactive state traces and validation; and
- policy-controlled repo, sidecar, and machine-local persistence.

These capabilities form the foundation for concurrent collaboration. The phase
is currently the primary ownership unit, and workspace context remains central
to execution.

## Lane-Centered Direction

The [lane-centered workbench design](design/lane-centered-workbench/README.md)
develops the next product model: a lane as a durable, observable execution
stream that connects intent, workspace activity, goals, observations,
validation, pull requests, and outcome review.

A lane has its own project-state identity. Branches, worktrees, pull requests,
RFCs, phases, and chat sessions remain linked artifacts with distinct roles.
The first proof is intentionally narrow: an agent can create, focus, and resume
a lane from canonical project state without relying on chat history.

This remains a design direction until the lane runtime model and user surfaces
land. The detailed lane semantics, product hierarchy, interaction design, and
implementation brief belong to the lane-centered workbench design package.

## Relationship To Design Records

This vision is the architectural overview. RFCs remain the scoped records for
specific contracts and implementations.

| Source | Role in this vision |
| ------ | ------------------- |
| [RFC 10176: Project State Model](rfcs/stage-3/10176-project-state-model.md) | Defines the canonical execution entities, relationships, and authority model. |
| [RFC 10184: Project / Workspace / Worktree](rfcs/stage-1/10184-project-workspace-worktree-unbundling-the-conflated-root.md) | Defines shared project identity, workspace views, state roots, and persistence policy. |
| [RFC 10181: Shared Perception](rfcs/stage-2/10181-shared-perception-inbox-as-a-steering-channel.md) | Defines scoped feedback and observations delivered through inbox and steering. |
| [RFC 10165: Reactive SQLite](rfcs/stage-3/10165-reactive-sqlite-virtual-table-integration-with-revision-algebra.md) | Defines how state observations and mutations support trace validation. |
| [RFC 10200: CLI-Shaped MCP Transport](rfcs/stage-1/10200-cli-shaped-exo-run-mcp-transport.md) | Defines the common Exo command language exposed to MCP-capable clients. |
| [RFC 10202: Lane-Centered Workbench Adoption](rfcs/stage-2/10202-lane-centered-workbench-adoption.md) | Specifies the lane-centered product direction and the first create, focus, and resume proof. |

Two existing RFCs need explicit alignment as the lane design advances:

- [RFC 10155: Modes of Collaboration](rfcs/stage-4/10155-modes-of-collaboration.md)
  describes role-oriented collaboration modes. Current Exo behavior is organized
  more directly around scoped work, steering, ownership, and outcome review.
- [RFC 10192: Epoch-Owned Sidecar Collaboration](rfcs/stage-0/10192-epoch-owned-sidecar-collaboration.md)
  proposes epoch ownership for future concurrency. Current implementation uses
  phase ownership, while the lane design proposes a durable execution-stream
  boundary. Future ownership work should resolve that relationship explicitly.

## Design Commitments

Exo's collaboration model carries these commitments forward:

1. Project state survives individual sessions and remains inspectable.
2. Work has explicit scope, focus, ownership, and reviewable outcomes.
3. Human and agent share observations and validation evidence.
4. Workflow structure matches the size and urgency of the work.
5. Every interface operates on the same project semantics.
6. Concurrent work develops toward durable execution streams with
   workspace-specific views.
