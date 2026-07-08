<!-- exo:10196 ulid:01ktyrawrak9hc8v4hjj3m162j -->

# RFC 10196: Worktree-Aware Sidecar State and Branch-Local Document Overlays

**Status**: Stage 2 Draft
**Feature**: sidecar

## Summary

Exo projects share durable operational state across linked worktrees. RFC documents remain ordinary Git documents, so each worktree observes the RFC corpus at its own branch, commit, index, and working tree.

This RFC gives RFC metadata two coordinated views. Shared `rfcs` state represents the canonical RFC corpus from the locally known default-branch ref. Each workspace records a reactive, machine-local snapshot of the RFC documents visible in its checkout.

RFC reads use the current workspace snapshot. Managed commands edit Markdown in that workspace and refresh the snapshot. Shared canonical metadata advances when the corresponding document change reaches the canonical Git ref.

Git history accepted by the locally known default branch forms the publication boundary. Feature branches can create, edit, promote, withdraw, archive, supersede, rename, and repair RFCs while those changes remain local to their worktree. Older worktrees stay faithful to their own checkout, and shared metadata advances only from canonical history.

## Motivation

A sidecar-backed project may have several worktrees on different commits while all of them share one SQLite database and sidecar projection. RFC metadata currently combines two different kinds of fact:

- project-level RFC identity and accepted lifecycle state;
- branch-local observations of Markdown files under `docs/rfcs`.

Treating every workspace scan as global reconciliation lets whichever worktree runs Exo most recently rewrite shared metadata. A worktree on an older commit can restore a withdrawn RFC to its former stage. A valid RFC present on the canonical branch can disappear from SQLite and surface as repeated `metadata_relink` debt. A feature branch can also publish an unmerged lifecycle decision into the shared sidecar.

Exo needs both layers. The shared project needs durable, queryable RFC identity. The current workspace needs a faithful view of the documents it has checked out. This RFC makes that relationship explicit and gives both layers stable semantics.

## Design Principles

1. **Shared metadata follows accepted Git history.** The canonical RFC view comes from the locally known default-branch ref.
2. **Workspace reads follow the current checkout.** A branch sees its committed, staged, unstaged, and newly created RFC documents immediately.
3. **Merge is the publication boundary.** Workspace document changes remain local observations until the canonical ref contains them.
4. **Absence is scoped evidence.** A missing document updates that workspace's visibility while shared identity remains available until canonical reconciliation observes an explicit managed document change.
5. **RFC documents carry portable lifecycle meaning.** A clone can reconstruct stage, status, reasons, feature, consolidation, and relationships from Git plus the stable anchor.
6. **Stable anchors define identity.** File paths and RFC numbers can change through managed operations; the anchor ULID joins workspace and canonical records.
7. **Canonical reconciliation is local and deterministic.** It reads refs already present in the local Git common directory and leaves network synchronization to the user.
8. **Reconciliation preserves a last-known-good view.** Git, parse, identity, or transaction failures keep previously committed state available and produce scoped diagnostics.

## User Experience

A feature worktree sees its RFC changes as soon as Exo writes or reads the edited documents. Other linked worktrees continue to see the documents in their own checkout, with canonical lifecycle state available for comparison.

After the change merges and the locally known default-branch ref advances, the next Exo load publishes the merged document state into shared SQLite and the sidecar projection. Every workspace can then compare its checkout with that accepted state.

When a checkout lacks an RFC that exists canonically, Exo still returns the shared RFC and identifies its workspace absence. Repair guidance stays attached to the workspace or canonical tree that produced the diagnostic.

## State Model

| Layer | Ownership | Contents | Portability |
| --- | --- | --- | --- |
| Shared RFC metadata | Project | Stable anchor identity and the canonical RFC view | Included in sidecar SQL projection |
| Workspace RFC snapshot | Machine-local workspace | Git identity and the document-set fingerprint for one checkout | Excluded from sidecar projection |
| Workspace RFC observations | Machine-local workspace | Parsed RFC documents visible in that checkout | Excluded from sidecar projection |
| RFC Markdown | Git branch | Public design prose and portable lifecycle metadata | Transported by the project repository |

The existing `rfcs` virtual table remains the shared project query and relationship surface. Canonical reconciliation writes it through the reactive virtual table so row digests and rowset revisions remain current.

Workspace snapshots and observations use two new reactive virtual tables:

```sql
CREATE TABLE rfc_workspace_snapshots_data (
    id                  INTEGER PRIMARY KEY,
    workspace_root      TEXT NOT NULL UNIQUE,
    branch_name         TEXT,
    head_oid            TEXT NOT NULL,
    document_digest     BLOB NOT NULL CHECK(length(document_digest) = 32),
    canonical_ref       TEXT,
    canonical_oid       TEXT,
    observed_at         TEXT NOT NULL
);

CREATE TABLE rfc_workspace_observations_data (
    id                  INTEGER PRIMARY KEY,
    workspace_root      TEXT NOT NULL,
    text_id             TEXT NOT NULL,
    rfc_number          INTEGER NOT NULL,
    title               TEXT NOT NULL,
    stage               INTEGER NOT NULL CHECK(stage BETWEEN 0 AND 4),
    status              TEXT NOT NULL
                            CHECK(status IN ('active', 'archived', 'withdrawn')),
    feature             TEXT,
    slug                TEXT NOT NULL,
    file_path           TEXT NOT NULL,
    superseded_by       TEXT,
    supersedes          TEXT,
    withdrawal_reason   TEXT,
    archived_reason     TEXT,
    consolidated_into   TEXT,
    branch_name         TEXT,
    head_oid            TEXT NOT NULL,
    observed_at         TEXT NOT NULL,
    UNIQUE(workspace_root, text_id),
    UNIQUE(workspace_root, file_path),
    FOREIGN KEY(workspace_root)
        REFERENCES rfc_workspace_snapshots_data(workspace_root)
        ON DELETE CASCADE
);
```

The overlay storage migration creates matching `*_rev` tables and `rowset_revisions` seed rows. The virtual table names are `rfc_workspace_snapshots` and `rfc_workspace_observations`. They participate in `REACTIVE_TABLES` and the reactive shadow-name boundary from RFC 10165.

Portable dumps and SQL projections serialize shared `rfcs` state. The workspace tables stay outside `dump::TABLE_ORDER`, imports, and portable backups because their absolute paths and Git observations belong to one machine.

The document digest is a SHA-256 digest over the sorted tuples of repository-relative RFC path, file kind, and content digest for every managed RFC document visible in the checkout. It changes for committed, staged, unstaged, renamed, deleted, and untracked RFC document changes without depending on unrelated working-tree files.

## Canonical Git Ref Resolution

Exo resolves one canonical RFC document tree from refs already present in the repository's Git common directory.

The resolver uses this order:

1. `refs/remotes/origin/HEAD` when it resolves to a commit;
2. the sole resolvable `refs/remotes/<remote>/HEAD` when exactly one other remote advertises a symbolic default branch;
3. `refs/heads/main`;
4. `refs/heads/master`;
5. the current `HEAD` when `git worktree list` reports one worktree.

Multiple eligible non-origin remote HEADs are ambiguous. A detached `HEAD` in a multi-worktree repository identifies a workspace rather than the canonical tree. When the resolver finds no unambiguous canonical ref, Exo keeps shared metadata, refreshes the workspace snapshot, and reports that canonical publication is waiting for a default-branch ref.

The resolver returns the symbolic ref name and peeled commit OID. Exo records both in the workspace snapshot used for the reconciliation pass. Canonical resolution is a read-only operation over existing refs and Git objects; the working tree, index, and refs remain unchanged.

## RFC Document Parser

Canonical blobs and workspace files use one parser with two inputs:

- repository-relative path, which determines stage and lifecycle location;
- document bytes, which determine anchor identity, number, title, feature, relationships, reasons, and consolidation.

The parser recognizes these portable forms near the top of the document:

```markdown
**Status**: Withdrawn
**Feature**: sidecar
**Reason**: The implemented design moved to RFC <replacement-id>.
- **Superseded by**: RFC <replacement-id>
- **Supersedes**: RFC <older-id>, RFC <another-older-id>
**Consolidated into**: RFC <replacement-id>
```

Existing list rows, metadata table rows, and sentence-style status, reason, or note markers remain supported. Managed edits preserve the source form when it can be updated without ambiguity. A compact metadata block immediately after the H1 is the canonical fallback.

Stage comes from `docs/rfcs/stage-N/`. `withdrawn/` and `archive/` determine lifecycle status; their explicit status and reason markers make that lifecycle portable outside the original SQLite database. A superseded relationship derives the read status `superseded` while the stored lifecycle status remains `active`, `withdrawn`, or `archived`.

The parser reports whether each optional field was declared. Canonical reconciliation follows this merge rule:

- a declared field, including a declared empty value, replaces the shared value;
- an undeclared compatibility field preserves the existing shared value;
- every managed lifecycle mutation materializes the field it changes, moving the corpus toward complete document authority.

## Canonical Reconciliation

Canonical reconciliation reads RFC paths and blobs directly from the resolved Git tree.

The pass proceeds as follows:

1. Resolve and peel the canonical ref.
2. Enumerate managed paths under `docs/rfcs` from the commit tree.
3. Read blob bytes and parse all candidates in memory.
4. Partition malformed paths, duplicate anchors, ambiguous numeric identities, and invalid lifecycle locations into repair diagnostics.
5. In one SQLite transaction, upsert every non-conflicting canonical candidate through `rfcs`.
6. Commit the ref name and OID to the workspace snapshot together with the workspace pass.

A valid anchored canonical document creates or relinks its shared row automatically. Canonical stage, lifecycle, path, title, feature, declared reasons, consolidation, and declared relationships replace older shared values. An absent canonical document leaves its existing shared row intact. Managed withdrawal, archive, supersession, and consolidation express retirement explicitly.

Each conflicting identity group retains its previous shared rows while Exo reports the paths and anchors needed for managed repair. Independent valid RFCs continue through the same reconciliation pass.

Any Git object or SQLite transaction failure preserves the complete prior shared view. Successful no-op reconciliation leaves row digests and rowset counters unchanged.

## Workspace Snapshot Refresh

Workspace refresh reads the issuing checkout, including working-tree files. It builds the complete candidate set and diagnostics in memory before opening the write transaction.

In one transaction Exo:

1. upserts the workspace snapshot;
2. deletes the prior observation rows for that workspace;
3. inserts every non-conflicting valid observation through the reactive virtual table;
4. commits the snapshot and observations together.

The observation key is `(workspace_root, text_id)`. Repository-relative path is unique within the workspace snapshot. Branch name may be absent for detached HEAD; `head_oid` always records the resolved commit.

A branch switch, commit, reset, rename, or working-tree edit changes the snapshot fingerprint and replaces only that workspace's observations. When the fingerprint and canonical OID match the stored snapshot, Exo reuses the current observation set.

A workspace scan failure retains the previous snapshot and returns a scoped error. Malformed and conflicting documents remain visible as workspace repair diagnostics while independent valid observations are committed.

## Effective RFC View

Every RFC read receives a validated workspace root and constructs the effective view by stable anchor identity.

For each anchor:

1. a current workspace observation supplies document-derived values;
2. the shared row supplies canonical comparison values and compatibility fields the document has not declared;
3. a shared row with no workspace observation remains in the result as `workspace_presence = "absent"`;
4. a workspace observation with no shared row remains in the result as `canonical_presence = "unpublished"`.

Read composition is side-effect-free. Canonical reconciliation is the operation that advances shared rows.

Numeric lookup succeeds when one effective anchor owns the requested RFC number. Multiple effective anchors with the same number return an ambiguity error naming their repository-relative paths and anchor IDs. Title grouping and relationship rendering operate on the effective set.

Human output identifies an overlay when workspace path, stage, lifecycle, title, or relationships differ from canonical metadata. JSON adds optional provenance fields:

- `document_source`: `workspace` or `canonical`;
- `workspace_presence`: `present` or `absent`;
- `canonical_presence`: `present` or `unpublished`;
- `workspace_branch` and `workspace_head`;
- `canonical_ref` and `canonical_head`;
- `differs_from_canonical`.

The fields are additive. Existing consumers can continue reading the current RFC shape.

## Managed RFC Mutations

Managed RFC commands edit Markdown in the issuing workspace, then refresh that workspace snapshot before returning.

This applies to create, edit, promote, withdraw, archive, supersede, rename, and repair. Their ordinary path refreshes the workspace observation; canonical reconciliation later publishes accepted document metadata into shared `rfcs`.

A mutation succeeds when the file operation and workspace refresh both succeed. If observation refresh fails after the file operation, Exo reports the document path, leaves the Git change available for retry, and preserves canonical shared metadata.

A working tree on the default branch follows the same rule as every other workspace. Uncommitted changes and local commits remain overlay state until the locally known canonical ref contains them. When fetch, pull, push, or merge activity advances that ref, the next canonical pass publishes the document state into shared SQLite and the next sidecar checkpoint updates `rfcs.sql`.

Repair remains path- and anchor-aware. A valid canonical document missing from SQLite relinks automatically. Managed repair remains available for malformed anchors, duplicate identities, ambiguous numbers, and invalid paths.

## Portable Lifecycle Mutations

Lifecycle commands update path and Markdown together:

- promote moves the file to `stage-N`;
- withdraw moves the file to `withdrawn/`, writes `**Status**: Withdrawn`, and writes `**Reason**`;
- archive moves the file to `archive/`, writes `**Status**: Archived`, and writes `**Reason**`;
- supersede writes reciprocal `Superseded by` and `Supersedes` markers;
- consolidation writes `**Consolidated into**`;
- feature changes write `**Feature**`.

A command updates an existing metadata source in place when its structure is recognized. Table rows remain table rows. Status, reason, and note prose retains text unrelated to the changed field. Linked supersession markers are replaced as a complete link so label, target, and title stay consistent.

The file path and explicit lifecycle markers must agree before the workspace observation commits. A mismatch becomes scoped repair debt, and managed lifecycle operations establish the transition.

## Request-Scoped Daemon Workspace

The shared daemon processes each request in the issuing workspace context.

The request envelope adds one backward-compatible field:

```json
{
  "workspace_root": "/absolute/canonical/workspace/path"
}
```

CLI and MCP clients send their canonicalized workspace root. The daemon:

1. resolves the requested path to an existing canonical workspace root;
2. resolves its project using the daemon's retained project-resolution service;
3. verifies the project ID and state root equal the daemon's startup project;
4. constructs command context, RFC reconciliation, repair diagnostics, and post-write persistence from the validated workspace;
5. rejects a different project before reading or mutating RFC documents.

Legacy requests without `workspace_root` use the daemon startup workspace. Normal daemon dispatch and `--direct` therefore share one workspace-resolution contract.

Daemon boot identity, connection recovery, bounded health probing, and project-resolution caching remain part of RFC 10195's runtime lifecycle work. This RFC supplies the request workspace and consumes the validated project without resolving it repeatedly inside RFC reads.

## Reactive and Transaction Semantics

Both workspace tables are reactive virtual tables. Snapshot replacement changes their row digests and rowset revisions through ordinary `xUpdate` mediation. Traces over RFC membership invalidate when a workspace document appears or disappears. Traces over RFC content invalidate when parsed metadata changes.

Canonical writes use the existing `rfcs` virtual table and commit in the same SQLite transaction as snapshot bookkeeping. Sidecar checkpointing runs after the transaction and serializes shared `rfcs` only.

Observation refresh is conservative: replacing a changed snapshot may update every observation row for that workspace. Unchanged snapshots perform no writes. A later optimization may diff rows while preserving the same revision contract.

## Migration and Backfill

The overlay storage migration creates the machine-local tables, revision tables, indexes, and rowset seeds. It preserves every existing `rfcs` row.

The first context load after migration runs:

1. canonical ref resolution;
2. canonical reconciliation into shared `rfcs`;
3. current workspace snapshot refresh;
4. ordinary sidecar checkpointing when shared canonical rows changed.

This backfill automatically restores a valid canonical RFC missing from SQLite and restores canonical lifecycle state previously overwritten by an older worktree. Workspace observations are rebuilt locally and never imported from SQL projection.

The migration is idempotent. Opening the database from another worktree refreshes that workspace only. Opening without a canonical ref preserves shared state and still establishes the workspace overlay.

## Repair and Failure Semantics

| Condition | Result |
| --- | --- |
| Canonical ref unavailable | Preserve shared rows, refresh workspace, report publication waiting |
| Canonical Git object unavailable | Preserve shared rows and prior canonical snapshot, return Git diagnostic |
| Valid canonical RFC missing from SQLite | Insert or relink automatically |
| RFC absent from current workspace | Keep shared row, report workspace absence |
| RFC absent from canonical tree | Keep shared row until explicit lifecycle action |
| Malformed anchor or invalid path | Preserve affected prior row, report scoped repair debt |
| Duplicate anchor or numeric identity | Exclude conflicting group, return ambiguity with paths |
| Workspace scan fails | Preserve prior workspace snapshot and observations |
| SQLite transaction fails | Roll back the complete pass |
| Sidecar checkpoint fails after commit | Keep committed SQLite state and report retryable persistence debt |
| Request workspace belongs to another project | Reject before document access |

Repair reminders name whether evidence came from the canonical tree or current workspace. One workspace never receives an instruction that would rewrite another workspace's branch.

## Security and Privacy

The daemon validates request workspaces against project identity and state root before accessing documents.

Machine-local snapshots stay within the local database because they may contain absolute paths, branch names, and commit identities. Local human diagnostics may display that context; portable projections, dumps, sidecar commits, shared logs, and machine responses omit it.

Canonical reconciliation reads Git tree entries and blobs only. It executes no repository code, follows no worktree symlink outside the project, and accepts only managed repository-relative RFC paths.

Workspace scanning rejects managed RFC paths that resolve outside the workspace root.

## Compatibility

The request-envelope field and JSON provenance fields are optional additions. Existing JSON, MCP, and CLI clients continue to work.

Repositories with one worktree and no remote default-branch ref use current HEAD as canonical and observe the same effective RFC state after migration.

Repo, sidecar, and shadow policies use the same document model and overlay semantics. Policy selects where shared state and its SQL projection live.

Existing RFC anchors and numeric IDs keep their meaning. This RFC changes provenance and reconciliation, not RFC identity.

## Drawbacks

The model introduces two machine-local tables and requires read surfaces to explain provenance.

A locally stale remote-default ref delays shared publication until local Git refs advance. This property makes accepted Git history the boundary and prevents a stale checkout from becoming project authority.

Preserving shared rows whose documents disappear requires explicit lifecycle cleanup for true deletion. RFCs already use managed archive, withdrawal, supersession, and consolidation operations, so explicit retirement matches the corpus model.

Workspace refresh may rewrite a complete observation set after relevant document changes. The corpus is small enough for this first implementation, and the reactive contract leaves room for row-level diffing.

## Alternatives

### Let every workspace reconcile shared metadata

This keeps one metadata layer and makes shared project truth depend on command order across worktrees.

### Restrict reconciliation to one canonical checkout

A designated checkout prevents stale writers but introduces machine-local authority that is difficult to preserve across moves and machines. A canonical Git ref represents accepted history without selecting one filesystem path.

### Publish workspace metadata with an explicit command

An explicit publish command gives precise control but makes every ordinary merge require a second state-management step. Canonical-ref reconciliation makes Git acceptance the publication boundary.

### Store RFC metadata only in Markdown

Direct parsing avoids duplicated metadata but gives up reactive traces, shared query surfaces, and sidecar-backed project state. The overlay preserves those capabilities while respecting Git locality.

## Implementation Sequence

1. Add the overlay storage migration, reactive workspace tables, revision coverage, and dump-exclusion tests.
2. Extract one path-and-bytes RFC parser for filesystem documents and Git blobs.
3. Add canonical-ref resolution and canonical Git-tree reconciliation.
4. Add atomic workspace snapshot refresh and effective-view composition.
5. Route RFC reads, repair diagnostics, and managed mutations through the effective view.
6. Materialize portable lifecycle metadata for every lifecycle command.
7. Add request-scoped workspace identity after the daemon reliability branch establishes retained project dispatch.
8. Migrate existing databases, repair canonical RFC state, and regenerate sidecar projection.
9. Verify linked-worktree isolation and reconcile this RFC with delivered behavior.

## Test Matrix

### Storage

- Migration creates both data tables, revision tables, virtual tables, indexes, and rowset seeds.
- Workspace tables are absent from `dump::TABLE_ORDER`, sidecar projection, imports, and portable dumps.
- Snapshot insert, replacement, and delete maintain row digests and rowset counters.
- Observation content and membership traces invalidate after RFC document changes.
- Failed replacement transactions retain the previous complete snapshot.

### Canonical Git Reconciliation

- `origin/HEAD` wins over current branch and local `main`.
- The documented fallback order is deterministic and ambiguity preserves shared state.
- Canonical blobs parse without checking out or executing repository content.
- A missing shared row relinks from a valid canonical document.
- A canonical path or lifecycle change updates the matching anchor.
- Canonical absence preserves the shared row.
- Duplicate anchors and numbers preserve affected prior rows and produce scoped diagnostics.
- A stale worktree cannot restore older canonical metadata.

### Workspace Overlays

- Two linked worktrees share one sidecar database.
- A feature branch withdraws an RFC and sees the withdrawal locally.
- The default branch continues to see canonical active metadata.
- A feature branch creates an RFC that is immediately readable there and creates no global repair debt.
- A shared RFC absent from one checkout remains queryable with workspace absence provenance.
- Switching branch or commit replaces only that workspace's snapshot.
- Working-tree edits invalidate content traces before commit.
- After the canonical ref advances, fresh workspaces and shared metadata see the merged lifecycle state.

### Lifecycle Portability

- Promote, withdraw, archive, supersede, consolidate, feature edit, rename, and repair update Markdown and workspace observations together.
- Withdrawal and archive reasons survive restart, another worktree, merge, canonical reconciliation, and sidecar regeneration.
- Status/reason prose and metadata tables retain unrelated content.
- Linked relationship markers keep labels, URLs, and titles consistent.
- File-path and status-marker disagreements remain scoped repair debt.

### Dispatch and Compatibility

- Normal daemon dispatch and `--direct` return equivalent effective views for the issuing workspace.
- Two CLI clients using one daemon receive their own workspace overlays.
- MCP supplies the same workspace identity as CLI.
- A daemon rejects a workspace from another project or state root.
- Legacy envelopes use the daemon startup workspace.
- Existing JSON consumers accept additive provenance fields.

### Regression

- Canonical RFC 10200 automatically relinks when absent from SQLite.
- Canonical RFC 0129 remains withdrawn after a stale linked worktree runs `status`.
- Shared `rfcs.sql` contains corrected canonical state and no absolute workspace paths.
- RFC list, show, status, pipeline, repair, create, edit, promote, withdraw, archive, supersede, and rename remain covered.

## Relationship to Other RFCs

RFC 10184 defines project, workspace, worktree, state-root, and sidecar identity. This RFC applies that model to branch-local RFC documents.

RFC 10189 defines sidecar Git as transport for project state. Workspace observations remain outside that portable transport.

RFC 10191 defines sidecar write ownership and stale-writer fencing. Writer ownership controls who checkpoints shared state; this RFC controls which document evidence can become shared state.

RFC 10195 defines daemon lifecycle authority and shared perception surfaces. This RFC adds request-scoped workspace context to that shared runtime.

RFC 10165 defines reactive SQLite observation and write mediation. Workspace RFC snapshots and observations participate in that reactive storage contract.

## Stage 2 Acceptance

The draft is ready for Stage 2 when reviewers confirm the schema, ref-resolution order, parser authority, effective-view precedence, transaction boundaries, lifecycle marker format, daemon workspace validation, migration behavior, failure semantics, and test matrix are implementation-ready.

After Stage 2 promotion, implementation proceeds in the sequence above. Stage 3 requires the complete storage, reconciliation, overlay, lifecycle, daemon-context, migration, and multi-worktree test contract to pass against the delivered behavior.
