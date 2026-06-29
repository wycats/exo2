<!-- exo:10196 ulid:01ktyrawrak9hc8v4hjj3m162j -->

# RFC 10196: Worktree-Aware Sidecar State and Branch-Local Document Overlays

**Status:** Idea
**Feature:** sidecar

## Summary

RFC 10184 correctly defines a project as the shared Exo state boundary and a
workspace as the checked-out worktree issuing commands. RFC 10189 correctly
defines sidecar Git as a transport for Exo state, not the state model itself.

This RFC fills in the missing layer between those decisions: document-backed
state can be branch-local even when the Exo project database and sidecar state
are shared by all linked worktrees.

In particular, RFC markdown under `docs/rfcs` is a branch/worktree file view.
When a sidecar-backed project has multiple worktrees checked out at different
commits, Exo must not treat the current workspace's `docs/rfcs` tree as a
complete, destructive projection of global RFC metadata. It is an observation
from one branch-local document overlay.

## Problem

The current dogfood warning for RFC 10194 and RFC 10195 exposed the gap:

- the current worktree can see RFC markdown files that another checkout does
  not see yet,
- the shared sidecar database can still contain metadata rows for those RFCs,
- `rfc status` reports `metadata_relink` repair debt for the current worktree,
- and the same shared sidecar state can be reconciled by whichever worktree
  happens to run an Exo command.

That warning is not the evidence-file repair bug. The evidence-file bug was
about mistaking non-RFC markdown such as
`docs/rfcs/evidence/.../2026-...md` for RFC 2026. This problem is about real
RFC files whose file existence is branch-local while their metadata is stored
in shared project state.

Without an explicit overlay model, Exo can make unsafe inferences:

- a file missing from the current checkout can look like stale global metadata,
- a file present only on the current branch can look like universal project
  truth,
- repair reminders can imply that every worktree should mutate shared state,
- and reconciliation can delete or rewrite metadata based on one worktree's
  partial file view.

## Thesis

Shared sidecar state should remember durable project knowledge. A workspace
scan should contribute branch-local document observations. Reconciliation
between those layers must be non-destructive unless Exo has explicit evidence
that the project-level fact, not merely the current checkout's file view, is
obsolete.

## Proposed Model

Exo should distinguish three layers:

```text
Project state
  Shared phases, goals, tasks, RFC identities, inbox rows, durable events

Document observations
  Facts learned from scanning files in one workspace at one branch/commit

Workspace overlay
  The current checkout's branch-local file tree and repairable documents
```

The project database remains shared for linked worktrees. The sidecar remains
the portable representation of that shared project state. The new rule is that
document-backed metadata must carry enough provenance to avoid pretending that
one workspace overlay is the whole project.

For RFC metadata, Exo should record at least:

- RFC id and ULID anchor identity,
- title and expected canonical RFC path,
- a stable workspace/worktree observation key, derived from project identity
  and git state rather than an absolute local path,
- the git branch or detached commit that produced the observation when
  available,
- the observed document path,
- whether the current workspace can currently resolve the document,
- and the repair reasons that apply to that workspace's document view.

Machine-local checkout paths can still be useful display/debug details, but
they must be derived locally or stored in a local-only observation cache. They
must not become portable shared sidecar facts because those paths vary across
machines and may expose local filesystem details.

The exact schema is intentionally left to the implementation RFC or Stage 1
revision, but the contract should be stable: absence from one worktree is not
evidence that the shared RFC identity is obsolete.

## Reconciliation Rules

RFC discovery remains constrained to real RFC document locations:
`docs/rfcs/stage-0`, `docs/rfcs/stage-1`, `docs/rfcs/stage-2`,
`docs/rfcs/stage-3`, `docs/rfcs/stage-4`, `docs/rfcs/archive`,
`docs/rfcs/withdrawn`, and valid legacy flat RFC files.

When scanning a workspace:

- create or refresh document observations for RFC files visible in that
  workspace,
- attach repair debt to the workspace/document observation that produced it,
- update shared RFC identity only when the anchor identity is valid and the
  update is compatible with existing project state,
- do not delete shared RFC metadata merely because the file is absent from the
  current checkout,
- do not rewrite another branch's path as a side effect of repairing the
  current branch's document,
- and never broaden RFC scanning to arbitrary markdown under `docs/rfcs/**`.

If Exo believes a shared RFC row is truly stale, it should require stronger
evidence than "not present in this worktree". Examples might include an
explicit archive/withdraw/supersede operation, a semantic sidecar sync
conflict, or a user-approved cleanup command.

## User-Facing Behavior

Status and reminder output should name the layer:

- "This workspace has RFC document repair debt" when the file exists in the
  current checkout and can be repaired locally.
- "Shared sidecar metadata references an RFC document not present in this
  workspace" when the shared row exists but the current branch lacks the file.
- "Another workspace observed this RFC document" when provenance is known.

Repair hints should remain actionable, but scoped. `rfc repair <id>` should
only repair a document visible in the current workspace. If the RFC is known
only through shared metadata and the current checkout lacks the file, Exo
should not suggest a mutating repair in that workspace.

JSON details should expose the current path, expected path, reasons, title,
workspace root, and document-observation provenance when available.

## Relationship To Existing RFCs

RFC 10184 defines the project/workspace/worktree split. This RFC preserves its
core decision that linked worktrees share a project id, state root, database,
runtime directory, socket, and daemon. It adds that document-backed metadata
needs a workspace overlay layer before it updates shared project facts.

RFC 10189 defines sidecar Git as transport. This RFC applies that principle to
repo documents: the sidecar database is not allowed to treat one checkout's
files as the complete portable state model.

RFC 10191 defines sidecar write ownership and stale-writer fencing. This RFC is
orthogonal but complementary: writer fencing decides who may checkpoint shared
state; document overlays decide what evidence a writer is allowed to treat as
global truth.

## Implementation Direction

The first implementation should be defensive and narrow:

- stop destructive RFC metadata deletion during workspace reconciliation for
  sidecar-backed and worktree-shared projects,
- represent missing-current-workspace documents as workspace visibility debt,
  not global staleness,
- keep `rfc repair <id>` scoped to documents visible in the current workspace,
- and update status/reminder text to explain branch-local document repair debt.

A later implementation can generalize the provenance model to other
document-backed tables if phases, specs, inbox attachments, or generated
context files need the same overlay treatment.

## Test Scenarios

- Two linked worktrees share one sidecar database. Worktree A contains RFC
  10194 and worktree B is checked out before that RFC exists. Running
  `rfc status` in B must not delete or rewrite RFC 10194 metadata.
- A real RFC file exists only in the current worktree and has metadata relink
  debt. `rfc status` should suggest `rfc repair <id>` for that workspace.
- Shared metadata references an RFC absent from the current worktree. `rfc
  status` should explain workspace visibility, not suggest a mutating repair.
- Evidence/support markdown under `docs/rfcs/evidence/**` remains ignored as
  an RFC source and never produces `rfc repair 2026`.
- `sidecar repo sync` and dogfood verification can report document-overlay debt
  without treating it as raw Git or SQLite corruption.

## Open Questions

- Should document observations be persisted as a new table, embedded in RFC
  metadata, or derived from a workspace scan cache?
- How much git provenance is stable enough to store: branch name, commit SHA,
  worktree path, or all three?
- Should repo-policy projects use the same overlay model, or is the first
  implementation limited to sidecar/shadow policies and linked worktrees?
- What command should intentionally retire shared metadata for a document that
  no active branch can still observe?

## Non-Goals

- This RFC does not change RFC anchor identity format.
- This RFC does not promote, archive, withdraw, or rewrite any existing RFC.
- This RFC does not broaden RFC scanning to arbitrary markdown.
- This RFC does not replace sidecar write ownership or semantic sidecar sync.
