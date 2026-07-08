<!-- exo:10196 ulid:01ktyrawrak9hc8v4hjj3m162j -->

# RFC 10196: Worktree-Aware Sidecar State and Branch-Local Document Overlays

**Status:** Stage 1 Proposal
**Feature:** sidecar

## Summary

Exo projects share durable operational state across linked worktrees. RFC documents remain ordinary Git documents, so each worktree observes the RFC corpus at its own branch and commit.

This RFC introduces a worktree-aware document overlay for RFC metadata. Shared `rfcs` state represents the canonical RFC corpus from the locally known default-branch ref. Each workspace records a machine-local observation of the RFC documents visible in its checkout. RFC reads and managed mutations use the current workspace observation, while shared canonical metadata advances when the corresponding document change reaches the canonical Git ref.

This model lets agents revise RFCs on feature branches without publishing provisional lifecycle state to every linked worktree. It also lets a stale worktree read its own branch without restoring older metadata over newer project truth.

## Motivation

A sidecar-backed project may have several worktrees on different commits while all of them share one SQLite database and sidecar projection. RFC metadata currently combines two different kinds of fact:

- project-level RFC identity and accepted lifecycle state;
- branch-local observations of Markdown files under `docs/rfcs`.

Treating every workspace scan as global reconciliation lets whichever worktree runs Exo most recently rewrite shared metadata. A worktree on an older commit can restore a withdrawn RFC to its former stage. A valid RFC present on the canonical branch can disappear from SQLite and surface as repeated `metadata_relink` debt. A feature branch can also publish an unmerged lifecycle decision into the shared sidecar.

Exo needs both layers. The shared project needs durable, queryable RFC identity. The current workspace needs a faithful view of the documents it has checked out. Making the relationship explicit gives both layers stable semantics.

## Design Principles

1. **Shared metadata follows accepted Git history.** The canonical RFC view comes from the locally known default-branch ref.
2. **Workspace reads follow the current checkout.** A feature branch can read and validate its own RFC changes immediately.
3. **Merge is the publication boundary.** Feature-branch document changes remain workspace observations until the canonical ref contains them.
4. **Absence is scoped evidence.** A document missing from one worktree says nothing about whether the shared RFC identity remains valid.
5. **RFC documents carry portable lifecycle meaning.** A clone can reconstruct stage, status, reason, and relationships from Git plus the stable anchor.
6. **Exo performs no implicit network fetch.** Canonical reconciliation uses refs already present in the local Git common directory.

## State Model

| Layer | Ownership | Contents | Portability |
| --- | --- | --- | --- |
| Shared RFC metadata | Project | Stable anchor identity and the canonical RFC view | Included in sidecar SQL projection |
| Workspace RFC observations | Machine-local workspace | Documents visible at the workspace branch, commit, and working tree | Stored locally and excluded from sidecar projection |
| RFC Markdown | Git branch | Public design prose and portable lifecycle metadata | Transported by the project repository |

The shared `rfcs` table remains the project-level query and relationship surface. Workspace observations use the same stable anchor identity and record the observed RFC number, title, stage, status, feature, slug, path, relationships, lifecycle reason, branch, commit, and observation time.

Workspace observations are reactive SQLite state so trace validation sees RFC document changes. They are machine-local in the same sense as workspace focus and phase ownership: dump and sidecar projection code excludes them.

## Canonical Git Ref

Exo resolves the canonical RFC document tree from locally available Git refs in this order:

1. the primary remote's symbolic default-branch ref, normally `refs/remotes/origin/HEAD`;
2. another available remote symbolic `HEAD`;
3. local `refs/heads/main`;
4. local `refs/heads/master`;
5. the current `HEAD` when the repository has one worktree.

When no unambiguous canonical ref is available, Exo preserves existing shared metadata and operates from the workspace observation. Status explains that canonical publication is waiting for a default-branch ref.

Canonical reconciliation reads RFC paths and blobs from Git objects. It does not check out files, alter the index, fetch a remote, or treat the current working tree as canonical merely because it issued the command.

## Reconciliation

Reconciliation has two independent passes.

### Canonical pass

Exo scans valid RFC document locations in the canonical Git tree and parses their anchored metadata.

- A valid anchored document creates or updates its shared RFC row.
- A canonical document whose shared row is missing is relinked automatically.
- Canonical stage, lifecycle, path, title, and declared relationships replace older shared values.
- Existing database-only compatibility fields are preserved when a legacy document has not materialized them yet.
- A shared row absent from the canonical tree remains available. Intentional RFC retirement uses a managed lifecycle operation rather than inference from absence.
- Malformed anchors, duplicate identities, ambiguous numbers, and invalid RFC paths remain explicit repair debt.

### Workspace pass

Exo scans the current checkout and atomically refreshes that workspace's observation set.

- Valid documents become workspace observations even when they do not yet exist in shared metadata.
- Missing documents are absent only from that workspace snapshot.
- Working-tree edits participate in the workspace view before commit.
- Switching branches or commits replaces the workspace snapshot rather than mutating another workspace's observations.

The effective RFC view overlays current workspace observations on shared metadata by stable anchor identity. Workspace-only RFCs appear in the current workspace. Shared RFCs absent from the current checkout remain visible with workspace-visibility context.

## Managed RFC Mutations

Managed RFC commands continue to edit Markdown in the current workspace.

On a feature branch, create, edit, promote, withdraw, archive, supersede, rename, and repair refresh the workspace observation. Shared canonical metadata remains at the accepted default-branch state.

When the canonical ref advances to include those document changes, the canonical pass updates shared metadata and the sidecar projection.

A working tree on the default branch follows the same rule: uncommitted or unpushed changes remain workspace observations until the locally known canonical ref contains them.

## Portable Lifecycle Metadata

Managed lifecycle commands materialize the metadata needed to reconstruct RFC state from Git:

- status;
- withdrawal or archive reason;
- superseded-by and supersedes relationships;
- feature when supplied;
- consolidation target when used.

Exo updates an existing metadata form in place when possible and otherwise writes a compact metadata block after the anchor and title. Table rows remain table rows, and status or reason prose retains its non-relationship context.

Legacy documents may rely on database-only values during migration. The canonical reconciler preserves those values until a managed edit materializes them.

## Read Surfaces

`rfc show`, `rfc list`, and `rfc status` use the effective workspace view.

Human output names a workspace overlay when its lifecycle or path differs from canonical metadata. JSON output includes the document source, observed branch and commit when available, and whether the current record differs from the canonical view. Absolute workspace paths remain local diagnostics and are not written to portable sidecar state.

Repair reminders identify their scope:

- malformed documents visible in the current workspace produce workspace repair guidance;
- a shared RFC absent from the current checkout produces workspace visibility information;
- a valid canonical RFC missing from SQLite is relinked automatically;
- one workspace never receives a repair suggestion that would rewrite another branch's document.

## Daemon Dispatch

The project daemon remains shared, while each request carries its issuing workspace.

The request envelope accepts an optional workspace root. CLI and MCP callers supply it. The daemon canonicalizes the requested path and verifies that it resolves to the same project ID and state root before dispatch. A request for another project is rejected. Legacy callers that omit the field use the daemon startup workspace.

Handlers, RFC reconciliation, repair detection, reminders, and post-write persistence use the validated request workspace. Daemon boot identity, connection recovery, and project-resolution caching remain the responsibility of the runtime lifecycle contract.

## Migration

The storage migration adds machine-local workspace observation state and excludes it from portable dumps.

On first open after migration:

1. Exo preserves existing shared RFC rows.
2. The canonical pass repairs shared metadata from the available canonical Git tree.
3. The workspace pass records the current checkout.
4. The next normal sidecar checkpoint regenerates canonical `rfcs.sql` without workspace observations.

This migration repairs valid canonical rows such as an RFC that repeatedly disappeared from SQLite and restores lifecycle state that an older worktree overwrote.

## Compatibility

The request-envelope workspace field is optional, preserving existing JSON and MCP clients.

Repositories without multiple worktrees continue to observe the same effective RFC state. Repo-policy, sidecar-policy, and shadow-policy projects use the same document model; only the location and portability of shared project state differ.

Existing RFC anchors and numeric IDs keep their meaning. This RFC changes provenance and reconciliation, not RFC identity.

## Security and Privacy

The daemon validates request workspaces against project identity before reading or mutating files.

Machine-local observations may contain absolute paths and therefore remain outside generated SQL projections and portable sidecar commits. Portable metadata contains repository-relative RFC paths and Git identities only.

Canonical reconciliation executes no repository code and reads no untrusted path outside the managed RFC collections.

## Drawbacks

The model introduces a second RFC metadata layer and requires read surfaces to explain which layer supplied a value.

A locally stale remote-default ref delays shared publication until Git refs are refreshed. This is preferable to allowing a stale worktree to publish its own document tree as project truth.

Preserving canonical rows whose documents disappear requires explicit lifecycle cleanup for true deletion. RFCs already use managed archive, withdrawal, and supersession operations, so explicit retirement matches the corpus model.

## Alternatives

### Let every workspace reconcile shared metadata

This preserves the current implementation shape and makes shared state depend on command order across worktrees.

### Restrict reconciliation to one canonical checkout

A designated checkout prevents stale writers but introduces machine-local authority that is difficult to preserve across moves and machines. A canonical Git ref represents accepted history without selecting one filesystem path.

### Publish workspace metadata with an explicit command

An explicit publish command gives precise control but makes every ordinary merge require a second state-management step. Canonical-ref reconciliation makes the Git merge the publication boundary.

### Store RFC metadata only in Markdown

Direct file parsing avoids duplicated metadata but gives up reactive traces, shared query surfaces, and sidecar-backed project state. The overlay model preserves those capabilities while respecting Git locality.

## Test Scenarios

- Two linked worktrees share one sidecar database. A feature branch withdraws an RFC and sees the withdrawal; the default branch continues to see canonical active state.
- A stale worktree runs `status` after the withdrawal merges and cannot restore the old shared row.
- Advancing the canonical ref publishes the merged lifecycle change to shared metadata.
- A feature branch creates an RFC that is immediately readable there and does not produce global relink debt.
- A valid canonical RFC missing from SQLite is relinked without a manual repair command.
- A shared RFC absent from the current checkout remains queryable with workspace visibility context.
- Withdrawal and archive reasons survive restart, another worktree, merge, and canonical reconciliation.
- Normal daemon dispatch and `--direct` produce the same effective view for the issuing workspace.
- The daemon rejects a request-scoped workspace belonging to another project.
- Sidecar SQL projection contains canonical RFC rows and excludes workspace observations.

## Relationship to Other RFCs

RFC 10184 defines project, workspace, worktree, state-root, and sidecar identity. This RFC applies that model to branch-local RFC documents.

RFC 10189 defines sidecar Git as transport for project state. Workspace observations remain outside that portable transport.

RFC 10191 defines sidecar write ownership and stale-writer fencing. Writer ownership controls who checkpoints shared state; this RFC controls which document evidence can become shared state.

RFC 10195 defines daemon lifecycle authority and shared perception surfaces. This RFC adds request-scoped workspace context to that shared runtime.

RFC 10165 defines reactive SQLite observation and write mediation. Workspace RFC observations participate in that reactive storage contract.

## Stage 1 Acceptance

The proposal is ready for Stage 2 when the storage shape, canonical-ref resolver, effective-read precedence, lifecycle metadata format, daemon workspace validation, migration behavior, and multi-worktree test matrix are implementation-ready.
