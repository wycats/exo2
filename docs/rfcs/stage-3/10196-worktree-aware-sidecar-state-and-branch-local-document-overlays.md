<!-- exo:10196 ulid:01ktyrawrak9hc8v4hjj3m162j -->

# RFC 10196: Worktree-Aware Sidecar State and Branch-Local Document Overlays

**Status**: Stage 3 Candidate
**Feature**: sidecar

## Summary

Exo projects share durable operational state across linked worktrees. RFC documents remain ordinary Git documents, so every worktree observes the RFC corpus at its own branch, commit, index, and working tree.

RFC metadata therefore has two coordinated views:

- shared `rfcs` state represents the canonical RFC corpus from the locally known default-branch ref;
- each workspace has a reactive, machine-local snapshot of the RFC documents visible in its checkout.

RFC reads reconcile accepted canonical documents and then compose the issuing workspace's view. Managed RFC commands edit Markdown in that workspace and refresh its observations. Shared canonical metadata advances when the corresponding document reaches the canonical Git ref.

Git history accepted by the locally known default branch is the publication boundary. Feature branches can create, edit, promote, withdraw, archive, supersede, rename, and repair RFCs while those changes remain local to their worktree. Older worktrees stay faithful to their own checkout, and shared metadata advances from canonical history.

## Motivation

A sidecar-backed project can have several worktrees on different commits while all of them share one SQLite database and sidecar projection. RFC metadata combines two kinds of fact:

- project-level RFC identity and accepted lifecycle state;
- workspace-local observations of Markdown under `docs/rfcs`.

When one workspace scan directly rewrites shared metadata, command order can become authority. A worktree on an older commit can restore an RFC to an earlier stage. A valid RFC present on the canonical branch can disappear from SQLite and surface as recurring relink debt. A feature branch can publish an unmerged lifecycle decision into the shared sidecar.

The two-view model gives each fact a stable owner. Shared state follows accepted Git history. Workspace state follows the issuing checkout. Exo can then preserve durable RFC identity, provide reactive queries, and show branch-local document changes without allowing one worktree to overwrite another worktree's view.

## Design Principles

1. **Shared metadata follows accepted Git history.** The canonical RFC view comes from the locally known default-branch ref.
2. **Workspace reads follow the current checkout.** A branch sees its committed, staged, unstaged, and newly created RFC documents immediately.
3. **Merge is the publication boundary.** Workspace document changes remain local observations until the canonical ref contains them.
4. **Absence is scoped evidence.** A missing workspace document changes that workspace's visibility. Canonical absence preserves an established shared row until an explicit lifecycle decision replaces it.
5. **RFC documents carry portable lifecycle meaning.** Git plus the stable anchor reconstructs stage, status, reasons, feature, consolidation metadata, and relationships.
6. **Stable anchors define identity.** File paths and RFC numbers can change through managed operations; the anchor ULID joins workspace and canonical records.
7. **Canonical reconciliation is local and deterministic.** It reads refs already present in the Git common directory and leaves network synchronization to the user.
8. **Reconciliation preserves the last committed view.** Git, parse, identity, and transaction failures keep previously committed state available and surface scoped evidence.

## User Experience

A feature worktree sees its RFC changes as soon as Exo observes the edited documents. Other linked worktrees continue to see the documents in their own checkout, with canonical lifecycle state available for comparison.

After the change merges and the locally known default-branch ref advances, the next public RFC read reconciles the merged document state into shared SQLite. Successful write-side finalization then regenerates the portable SQL projection and checkpoints sidecar state according to project policy.

When a checkout lacks an RFC that exists canonically, Exo still returns the shared RFC and marks its workspace presence as absent. A workspace-only RFC remains readable in that workspace and is marked unpublished. Human output calls out relevant overlay differences, while JSON carries additive provenance fields.

## State Model

| Layer | Ownership | Contents | Portability |
| --- | --- | --- | --- |
| Shared RFC metadata | Project | Stable anchor identity and canonical RFC state | Included in sidecar SQL projection |
| Workspace RFC snapshot | Machine-local workspace | Git identity and document-set fingerprint | Excluded from portable state |
| Workspace RFC observations | Machine-local workspace | Parsed RFC documents visible in the checkout | Excluded from portable state |
| Workspace RFC diagnostics | Machine-local workspace | Parse, identity, lifecycle, and path diagnostics | Excluded from portable state |
| Canonical baseline and quarantine | Machine-local project state | Migration evidence for rows without canonical Git authority | Excluded from portable state |
| RFC Markdown | Git branch | Public design prose and portable lifecycle metadata | Transported by the project repository |

The existing `rfcs` virtual table remains the project query and relationship surface. Canonical reconciliation writes through it so row digests and rowset revisions remain current.

Migration V022 adds three reactive virtual-table families:

- `rfc_workspace_snapshots` records workspace root, branch, HEAD, document digest, canonical ref and OID, and observation time;
- `rfc_workspace_observations` records parsed RFC identity, lifecycle, feature, relationships, reasons, consolidation metadata, declaration flags, path, and Git provenance;
- `rfc_workspace_diagnostics` records scoped path, parse, lifecycle, and identity findings.

The migration also adds the machine-local `rfc_canonical_baseline` and `rfc_canonical_quarantine` tables. The workspace tables participate in `REACTIVE_TABLES` and have matching revision tables and rowset counters. All five machine-local surfaces stay outside portable dumps, imports, backups, and SQL projections.

The document digest is SHA-256 over sorted tuples of repository-relative path, collection kind, and SHA-256 of the raw document bytes. The candidate set includes managed Markdown directly under `stage-0/` through `stage-4/`, `withdrawn/`, and `archive/`, plus legacy numbered Markdown directly under `docs/rfcs/`. README files, templates, nested evidence, support files, and non-Markdown files remain outside the snapshot.

## Canonical Git Ref Resolution

Exo resolves one canonical RFC document tree from refs already present in the repository's Git common directory. The resolver uses this order:

1. `refs/remotes/origin/HEAD` when it resolves to a commit;
2. the sole resolvable `refs/remotes/<remote>/HEAD` when exactly one other remote advertises a default branch;
3. `refs/heads/main`;
4. `refs/heads/master`;
5. the current `HEAD` when Git reports one worktree.

The resolver returns the symbolic ref and peeled commit OID. Canonical reconciliation reads tree entries and blobs from that commit without checking it out.

Multiple eligible non-origin remote HEADs and detached multi-worktree repositories preserve shared state because they provide no unique publication ref. A workspace with no usable Git history uses workspace fallback reconciliation. Fallback observations use the workspace document digest as their reconcile version, so each new request sees filesystem document changes. When fallback reconciliation runs, it performs its own filesystem scan before workspace snapshot refresh; a later request observes any change that occurs between those scans.

Snapshots with no canonical source expose empty canonical ref and OID provenance. A dedicated human-facing publication-waiting diagnostic remains a future refinement.

## RFC Document Authority

Canonical blobs and workspace files use one parser with two inputs:

- repository-relative path determines the active stage or retired collection;
- document bytes determine anchor identity, number, title, feature, relationships, reasons, consolidation metadata, and explicit lifecycle markers.

The parser reads metadata from the preamble between the H1 and the first level-two heading and ignores fenced code examples. It supports compact metadata, list rows, metadata table rows, and established sentence-style relationship markers. Managed edits preserve a recognized source form and use a compact metadata block as the fallback.

Active documents derive stage from `docs/rfcs/stage-N/`. Withdrawn and archived documents carry their last active stage, explicit status, and reason in Markdown. A supersession relationship changes the effective read status to superseded while preserving the stored lifecycle status.

Each optional field records whether the workspace document declared it. A declared value, including an intentional empty value, supplies the workspace or canonical field. An undeclared compatibility field inherits established shared metadata. Managed lifecycle operations materialize the fields they own, steadily increasing document authority.

## Canonical Reconciliation

Canonical reconciliation performs one coherent pass under the cross-process RFC reconciliation lock:

1. resolve and peel the canonical ref;
2. enumerate managed RFC paths and read their blobs;
3. parse candidates in memory;
4. classify malformed paths, duplicate anchors, and lifecycle conflicts;
5. upsert each independent valid candidate through the reactive `rfcs` surface in one SQLite transaction;
6. establish or advance the canonical baseline.

A valid anchored canonical document creates or relinks its shared row automatically. Canonical title, stage, lifecycle, path, feature, declared reasons, consolidation metadata, and declared relationships replace older shared values. Canonical absence preserves an established row. Explicit withdrawal, archive, supersession, and consolidation metadata express retirement and relationships portably.

Malformed or conflicting canonical candidates and duplicate-anchor groups are skipped while independent valid RFCs continue through the same pass. Existing rows whose anchored documents remain present are preserved. Canonical reconciliation does not persist a public diagnostic for skipped candidates; current-workspace parsing and identity conflicts appear in workspace diagnostics.

Distinct anchors that claim the same RFC number can both reconcile. Numeric lookup reports the resulting ambiguity rather than quarantining either record. Git object and transaction failures preserve the prior committed canonical view. A successful no-op leaves row digests and rowset counters unchanged.

## Workspace Snapshot Refresh

Workspace refresh reads managed RFC documents from the issuing checkout's filesystem, including tracked modifications, visible renames and deletions, and untracked candidates. It does not inspect index-only Git blobs; staged content is observed when the same content is present in the working tree. Exo builds observations and diagnostics before replacing the workspace snapshot in one transaction.

The replacement transaction:

1. upserts the workspace snapshot;
2. replaces that workspace's observation and diagnostic rows;
3. updates reactive row digests and rowset revisions;
4. commits the complete snapshot.

The observation key is `(workspace_root, text_id)`, and repository-relative path is unique within a snapshot. Branch name can be absent for detached HEAD. The resolved commit or `unborn` marker supplies `head_oid`.

Exo reuses a snapshot when branch, HEAD, document digest, canonical ref, and canonical OID all match. A branch or HEAD change with identical RFC bytes refreshes provenance. A changed workspace fallback request recomputes the filesystem document digest; fallback shared reconciliation and snapshot refresh can perform separate scans, and the next request converges on any mid-pass filesystem change.

A failed replacement retains the previous snapshot, observations, diagnostics, and reactive revision state.

## Effective RFC View

Every RFC surface receives a validated workspace root and composes records by stable anchor:

1. a current workspace observation supplies document-derived values;
2. the shared row supplies canonical comparison values and undeclared compatibility fields;
3. a shared row without a workspace observation remains visible with `workspace_presence = "absent"`;
4. a workspace observation without a shared row remains visible with `canonical_presence = "unpublished"`.

Composition itself is side-effect-free. Internal derived-state consumers can load a refreshed effective view without publishing canonical changes. Public `rfc list`, `rfc show`, and `rfc status` first perform one observe-and-reconcile pass, then compose their response. They are write-effect commands because reconciliation can update shared canonical state, workspace observations, and portable projection state.

The public read commands travel through the daemon writer lane and successful responses run post-write SQL projection and sidecar persistence. Their current daemon recovery class is external-at-most-once. Request-scoped observation reuse gives each command one canonical source and effective RFC bundle; the next command observes canonical-ref advancement afresh.

Numeric lookup succeeds when one effective anchor owns the requested number. Ambiguity names the matching paths and anchors. A missing or ambiguous public `rfc show` rolls back the request's reconciliation and workspace refresh, keeping canonical SQLite and portable projection aligned.

Human output identifies meaningful overlay differences. JSON entries from `rfc list` and `rfc status` include `document_source`, workspace and canonical presence, and `differs_from_canonical`. `rfc show` additionally includes workspace branch and commit provenance plus canonical ref and commit provenance. Status diagnostics retain their scoped workspace root.

## Managed RFC Mutations

Managed create and ID-addressed edit, promote, withdraw, archive, supersede, rename, and repair operations edit Markdown in the issuing workspace and refresh that workspace's observations before returning. Explicit `--path` edit and supersede operations target the supplied existing absolute or workspace-relative path; the current command contract treats an absolute path as trusted input and does not confine it to the issuing workspace.

Portable lifecycle operations materialize their meaning in Markdown:

- promote moves the file to `stage-N`;
- withdraw and archive preserve the last active stage, move the file to its retired collection, and write status, stage, and reason;
- supersede writes reciprocal relationship markers;
- feature edits write `Feature`;
- rename preserves stable identity while changing the path;
- repair updates the managed anchor, number, or path identity selected by the repair operation.

Consolidation metadata is part of the parser, effective view, shared row, migration, and portable projection. A dedicated managed `rfc consolidate` command remains future work; current consolidation decisions can be represented through managed RFC editing.

A mutation succeeds when its file operation and workspace refresh succeed. If refresh fails after the file edit, Exo reports the path and leaves the Git change available for retry while shared canonical metadata remains governed by the canonical ref.

## Request-Scoped Daemon Workspace

The request envelope carries an optional absolute workspace root. CLI, MCP, forwarded writer-lane, and VS Code clients stamp the canonical issuing worktree root.

For an explicit workspace, the daemon:

1. resolves files and nested directories to their canonical worktree root;
2. resolves that root through retained project policy;
3. verifies project identity and state root against the daemon's startup project;
4. constructs command context, reconciliation, diagnostics, and persistence from the validated workspace;
5. rejects foreign and reused paths before document access.

The validation supports linked worktrees and Git submodules. Legacy envelopes use the validated daemon startup workspace. Runtime outcome replay retains its authority when the issuing workspace disappears, while finalization uses a validated surviving project workspace.

Daemon lifecycle, instance recovery, health probing, and request-outcome authority belong to RFC 10195. RFC 10196 supplies request workspace identity and consumes the retained project without repeating project resolution inside RFC reads.

## Transaction and Persistence Semantics

Canonical reconciliation and workspace snapshot replacement each have an atomic SQLite boundary. Their ordinary sequence is canonical reconciliation followed by workspace refresh and response composition.

A canonical failure stops the observe pass before workspace replacement. A workspace replacement failure preserves its prior snapshot; an already committed canonical pass remains available for retry. Public lookup adds a request transaction around reconciliation, refresh, and selection so a missing or ambiguous RFC does not leave canonical state ahead of its portable projection.

Successful public RFC reads and managed mutations run post-write finalization after command execution. Finalization regenerates portable shared RFC SQL and checkpoints sidecar state according to project policy. Workspace observations, diagnostics, baseline records, and quarantine records remain machine-local.

## Migration and Backfill

V022 creates the workspace observation model, reactive revision coverage, canonical baseline, and quarantine storage. Existing shared rows remain readable until the first successful canonical baseline.

The baseline transaction parses the canonical ref, upserts valid canonical documents, quarantines pre-overlay rows without canonical evidence, removes branch-only publication leaks from shared state, and records the ref, commit, and completion time. If a quarantined anchor later appears canonically, reconciliation restores it from Markdown and clears its quarantine record.

The lifecycle metadata migration materializes portable status, last active stage, reason, feature, and relationship evidence for the existing retired corpus. It recovers compatible stage evidence from current documents, retained history, and canonical shared state, while preserving malformed or conflicting declarations as repair evidence. Repeated managed updates are idempotent.

Opening the shared database from another worktree refreshes that workspace's observations and leaves accepted canonical state governed by the locally known default ref.

## Repair and Failure Semantics

| Condition | Delivered result |
| --- | --- |
| Canonical ref ambiguous in a multi-worktree repository | Preserve shared rows and refresh the issuing workspace |
| Workspace has no usable Git history | Reconcile from the workspace document digest |
| Canonical Git object access fails | Preserve the prior canonical view and return the error |
| Canonical document is malformed, lifecycle-conflicting, or part of a duplicate-anchor group | Skip the affected candidate, preserve an existing anchored row, and continue independent valid candidates; no persisted canonical diagnostic is emitted |
| Distinct canonical anchors claim one RFC number | Reconcile both records and report ambiguity when that number is read |
| Valid canonical RFC is missing from SQLite | Insert or relink automatically |
| RFC is absent from the current workspace | Keep the shared row and report workspace absence |
| RFC is absent from the canonical tree | Keep the established shared row |
| Current-workspace parse, anchor, path, or lifecycle conflict | Record a workspace diagnostic and keep canonical shared state available |
| Workspace snapshot replacement fails | Roll back the replacement and retain prior observations and revisions |
| Public RFC lookup is absent or ambiguous | Roll back request reconciliation and return the lookup error |
| Sidecar finalization fails after a successful command | Keep committed SQLite state and report retryable persistence debt |
| Request workspace belongs to another project or state root | Reject before document access |

Workspace repair diagnostics retain their workspace provenance. Skipped canonical candidates do not currently create persisted repair reminders. One workspace never receives an instruction that would rewrite another workspace's branch.

## Security and Privacy

The daemon validates request workspaces against project identity and state root before accessing documents. Workspace scanning accepts managed RFC paths rooted in the validated workspace. Explicit absolute `--path` mutations are a trusted-input escape hatch and can address a document outside that root.

Machine-local snapshots stay in local SQLite because they contain absolute paths and local Git observations. `rfc show` JSON can expose request-scoped branch names, ref names, and commit OIDs. `rfc status` diagnostics serialize their machine-local `workspace_root`, so callers with access to that diagnostic surface can observe an absolute path. Portable projections, dumps, sidecar commits, and shared logs contain shared RFC state only.

Canonical reconciliation reads Git tree entries and blobs. It executes no repository code and accepts managed repository-relative RFC paths.

## Compatibility

The request-envelope workspace field and RFC provenance fields are additive. Existing clients can continue reading the established response shape, and legacy daemon requests use the validated startup workspace.

A repository with one worktree and no remote default ref uses local `main`, `master`, or current HEAD as the canonical tree. An unborn or non-Git workspace uses workspace fallback reconciliation.

Repo, sidecar, and shadow policies share the same document and overlay semantics. Policy selects the shared state root and portable projection location. Existing RFC anchors and numbers keep their identity meaning.

## Delivered Evidence

The implementation is exercised across storage, reconciliation, migration, dispatch, and real linked-worktree boundaries:

- V022 migration and reactive storage tests cover table creation, revision updates, machine-local dump exclusion, and failed snapshot replacement rollback;
- canonical reconciliation tests cover ref precedence, Git-blob parsing, relink, quarantine, canonical absence, identity conflicts, stale-worktree resistance, and no-op revision stability;
- overlay tests cover branch-local stage and lifecycle changes, workspace-only RFCs, absent canonical records, declaration inheritance, provenance, canonical advancement, and portable projection privacy;
- lifecycle migration tests cover portable metadata recovery, retained-history evidence, reason precedence, malformed declarations, rename chains, dirty-workspace isolation, and idempotence;
- daemon and MCP tests cover request stamping, linked worktrees, nested paths, submodules, foreign and reused path rejection, direct/daemon view parity, replay, and persistence;
- public read regressions cover writer-lane effects, post-write projection, fallback freshness, one document collection per fallback observation, missing-lookup rollback, and request-scoped observation reuse.

## Remaining Refinements

Two refinements remain compatible with the delivered model:

1. expose an explicit human-facing publication-waiting diagnostic when no unique canonical ref is available;
2. add a dedicated managed consolidation command around the existing portable consolidation metadata.

Both refinements extend the operator surface. The implemented ownership, reconciliation, overlay, persistence, and portability contracts stand independently.

## Drawbacks

The model adds reactive workspace tables plus local baseline and quarantine storage. RFC reads also carry provenance and can perform reconciliation writes before returning their view.

A locally stale remote-default ref delays shared publication until local Git refs advance. This keeps accepted Git history authoritative and makes publication timing visible in provenance.

Preserving established shared rows after canonical absence makes true deletion an explicit lifecycle operation. RFC archive, withdrawal, supersession, and consolidation metadata provide that vocabulary.

Changed workspace snapshots currently replace the complete observation set. The RFC corpus is small enough for this boundary, and the reactive contract permits later row-level diffing.

## Alternatives

### Let every workspace reconcile shared metadata

One metadata layer makes shared project truth depend on command order across worktrees.

### Restrict reconciliation to one canonical checkout

A designated checkout introduces machine-local authority. A canonical Git ref represents accepted history without selecting one filesystem path.

### Publish workspace metadata with an explicit command

An explicit publish command adds a second state transition after every Git merge. Canonical-ref reconciliation uses the existing acceptance boundary.

### Store RFC metadata only in Markdown

Direct parsing gives up reactive traces, shared query surfaces, and sidecar-backed project state. The overlay preserves those capabilities while respecting Git locality.

## Relationship to Other RFCs

RFC 10184 defines project, workspace, worktree, state-root, and sidecar identity. RFC 10196 applies that model to branch-local RFC documents.

RFC 10189 defines sidecar Git as transport for project state. Workspace observations remain outside that portable transport.

RFC 10191 defines sidecar write ownership and stale-writer fencing. Writer ownership controls who checkpoints shared state; RFC 10196 controls which document evidence becomes shared state.

RFC 10195 defines daemon lifecycle, request outcome authority, and shared perception. RFC 10196 supplies request-scoped workspace context to that runtime.

RFC 10165 defines reactive SQLite observation and write mediation. Workspace RFC snapshots, observations, and diagnostics participate in that storage contract.

## Stage 3 Evidence

RFC 10196 is a Stage 3 candidate because the delivered storage, canonical-ref, overlay, lifecycle, dispatch, transaction, migration, privacy, and failure contracts match this document and the focused evidence above passes. The publication-waiting diagnostic and dedicated consolidation command remain additive refinements.

Stage 4 follows shipped use and corpus-level operating experience.
