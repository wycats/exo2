<!-- exo:10208 ulid:01m0grrn352apyvvew9n83ms3r -->

# RFC 10208: Storage Writer Compatibility Fence

**Status**: Stage 2 (Draft)
**Feature**: storage

## Summary

Exo should migrate older state forward without asking the user to manage schema
versions, and it should refuse to open state that requires a newer writer before
the current process can mutate it.

This RFC proposes one semantic `minimum_writer_generation` shared by the
canonical SQLite database and its portable sorted-SQL projection. The generation
is a compatibility fence, not a migration number. SQLite carries it in
`PRAGMA user_version`; the portable projection carries it as a strict first
header in the required `epochs.sql` file. A missing or zero generation means
legacy state and follows the existing invisible migration path.

The fence is deliberately prospective. A fence-aware release must ship before
a later release writes state requiring the next generation. Exo cannot make an
already-released importer recognize metadata it was never written to inspect,
especially when reconstructing a database from projection files alone.

## Motivation

Exo has two representations of project state with different jobs. SQLite is the
canonical runtime store. The deterministic SQL projection from RFC 10178 makes
portable project state reviewable, mergeable, and reconstructible. A compatible
binary must understand both before it can safely act as a writer.

Today, a newer binary can open an older database and apply every migration it
knows. That is the user experience Exo should preserve: forward migration is an
implementation detail. The reverse direction is less disciplined. The
migration runner can observe history entries it does not know and continue, and
an older projection importer knows only its compiled table order. It can ignore
new files, construct a partial database, and later write an older projection
over the same project.

Daemon executable identity reduces the frequency of that mixed-version path,
but it is not storage authority. A direct CLI invocation can open storage
without going through the shared daemon. An already-running daemon may outlive
an installation. A fresh clone can contain only portable projections and no
runtime identity at all. The state itself therefore needs to say which writers
can interpret it.

The compatibility contract should be simple from the user's perspective:

> A current Exo opens older state and upgrades it invisibly. An older Exo that
> cannot understand the state stops before changing it and tells the user to
> use a compatible version.

## Compatibility Authority

`minimum_writer_generation` is the minimum storage contract a binary must
support before it may semantically open a state surface. A binary has one
compiled `supported_writer_generation`. It may proceed only when:

```text
required_generation <= supported_writer_generation
```

The same generation namespace applies to SQLite and portable projections. A
successfully completed full export publishes the database generation for that
snapshot. During crash recovery or downgrade publication, the projection may
temporarily advertise a higher conservative generation, but never a lower one
than any visible file requires. The generation describes the oldest
fence-aware writer that can preserve the state's meaning; it does not describe
which migrations happened to run.

Generation zero is reserved for legacy state. A missing projection header and
`PRAGMA user_version = 0` both mean that no compatibility floor was published.
A fence-aware binary treats that state as legacy, applies its known migrations,
and publishes the generation required by the resulting state.

The shared numeric domain is the non-negative signed-32-bit range
`0..=2_147_483_647`. This is the range that can be represented consistently by
SQLite `user_version` and the portable header. Negative values, overflow, a
leading sign, and non-canonical decimal encodings are malformed compatibility
metadata. Zero is the only canonical encoding with a leading zero.

The writer generation is intentionally separate from `__schema_history`.
Schema history records which known migrations have run. The writer generation
says whether this binary may interpret and write the state at all. Migration
identifiers may be sparse or applied out of numeric order, and a generation
change may cover a semantic storage contract spanning several migrations or no
SQL migration at all.

The generation must never be inferred from the largest migration number, and a
writer must never require contiguous migration history.

The fence-aware baseline supports and writes generation zero. The first future
storage change that is incompatible with that baseline uses generation one.
Generation numbers advance only when the retained writer contract actually
changes; they are not release counters.

## SQLite Database Contract

SQLite carries the generation in `PRAGMA user_version`. Exo reserves that
pragma for the storage compatibility fence. Migration history continues to
live in `__schema_history` and is not reconstructed from the pragma.

### Semantic open

Every semantic database open, including a nominally read-only Exo operation,
passes through the fence. This is necessary because opening the current storage
abstraction can perform writable pragmas, migration work, virtual-table
construction, and revision backfill before the caller executes its apparent
read.

Every fence-aware semantic opener participates in a real cross-process storage
compatibility lock rooted in the resolved state root. Ordinary compatible opens
hold a shared lease for the full lifetime in which their semantic connection or
request-scoped storage object can issue SQL. The constructed `Database`, or the
request-scoped object that owns its connection, also owns that lease and cannot
outlive or detach from it. Migration takes the exclusive lease before changing
generation or schema and waits for every older semantic user to release its
shared lease. This operating-system lock is the serialization authority across
daemons, direct CLI processes, and one-shot tools; a connection-local mutex or
SQLite `BEGIN IMMEDIATE` alone does not satisfy the contract.

The open sequence is:

1. Open the SQLite file through a narrow low-level compatibility probe that
   does not run writable pragmas, migrations, virtual-table setup, hydration,
   or request transactions.
2. Read `PRAGMA user_version` and compare it with the binary's supported
   generation.
3. If the required generation is newer, close the probe and return
   `storage.writer_incompatible` before semantic construction.
4. Acquire the exclusive cross-process storage compatibility lock when known
   migrations remain; otherwise acquire its shared semantic-open lease.
5. Re-read `PRAGMA user_version` while serialized. A different or now
   unsupported value aborts before migration.
6. Compute the generation required by the complete set of missing known
   migrations, preserving sparse and out-of-order migration semantics.
7. If any pending migration can write state incompatible with the current
   generation, conservatively raise `PRAGMA user_version` to the required
   generation and durably commit that raise before executing the first such
   migration statement or pragma.
8. Apply every missing known migration while retaining the exclusive
   cross-process lock. Individual migrations may use their existing transaction
   and pragma behavior; `BEGIN IMMEDIATE` is an implementation tool, not the
   publication boundary.
9. On success, verify that `user_version` is at least the generation required by
   the resulting state. Release the exclusive lock only after migration history
   and generation publication are durable.
10. Establish a shared semantic lease for the resulting generation, rechecking
    `user_version` if the exclusive lease had to be released before the shared
    lease was acquired. Transfer ownership of the shared lease into the normal
    `Database` or request-scoped storage object before constructing virtual
    tables, reactive revision state, request transaction machinery, and the
    higher-level Exo context.

The conservative raise makes the transition crash-safe even when an existing
migration cannot be wrapped atomically because it changes connection pragmas
such as `foreign_keys`. A crash after the raise but before migration completion
leaves the database over-fenced: an older writer rejects it, while a compatible
writer can resume the missing migration under the same exclusive lock. A crash
must never leave incompatible schema or rows beneath the old generation.

The second check closes the gap between the initial probe and serialization.
The lifetime lease prevents a generation-advancing migration from beginning
while an older semantic object can still issue SQL. Dropping the lease ends that
object's authority to use its connection. The lock does not replace normal
SQLite locking, daemon identity, or writer ownership; those mechanisms continue
to control ordinary concurrent access within one compatible generation.

For repo and sidecar policy, the available canonical projection participates in
the same semantic-open decision even when a local SQLite cache already exists.
Exo first confirms that Git exposes a settled projection, then preflights the
projection's compatibility metadata before opening SQLite semantically. The
effective compatibility requirement is the higher observed requirement exposed
by the database or projection. A compatible cached database does not authorize
an older writer to ignore a newer portable representation, and checking the
projection does not imply replacing an existing cache from that projection.
This cached-open preflight intentionally does not require the complete
projection to form a coherent import snapshot: a compatible writer must still
be able to repair safely over-fenced output left by an interrupted publication
from canonical SQLite. Fresh-clone import separately retains and validates the
complete projection before target creation. Shadow policy has no portable
projection and therefore uses only the database fence.

Only the compatibility probe may inspect a database that requires a newer
writer. Exo must not expose a general "read anyway" mode because the normal
read stack is not observationally pure and because projecting partially
understood state as authoritative would be misleading.

The baseline lock is an `fs2` advisory shared/exclusive file lock at
`db_path.with_extension("writer-compat.lock")`. Acquisition is bounded to five
seconds. The lock file is persistent coordination state and is not normally
deleted. A timeout returns `storage.compatibility_busy`; it does not trigger
repair, daemon replacement, or lock-file removal.

Fence-aware writable connections also request SQLite's persistent-WAL behavior.
The final connection therefore leaves the WAL and WAL-index files available for
later read-only compatibility probes instead of repeatedly deleting and
reconstructing the shared WAL index. A missing WAL index remains a recovery
case: the probe reconstructs it only in an isolated copy and does not create or
modify canonical SQLite sidecar files.

During a rolling upgrade, a compatible older writer may still remove those
sidecars when its final connection closes. A probe that encounters SQLite or
sidecar I/O failure retries a bounded number of times and resamples the
canonical sidecar state before each attempt. Invalid or incompatible writer
metadata remains an immediate failure.

## Portable Projection Contract

RFC 10178's required `epochs.sql` file carries the projection generation. Its
first line is exactly:

```sql
-- exo:minimum-writer-generation=<generation>
```

The header is part of the projection format and precedes every SQL statement.
The generation is a canonical ASCII decimal in `0..=2_147_483_647`: no sign,
no surrounding whitespace, no trailing fields, and no leading zero except the
value zero itself. A missing header means legacy generation zero. A line
beginning with the Exo writer-generation prefix but not matching the grammar is
malformed metadata, not legacy state.

The existence of any canonical projection table file makes the projection
available for compatibility purposes. If `epochs.sql` itself is absent, the
projection generation is legacy generation zero. Fresh-clone import still
reads and validates every available table file; the missing generation carrier
does not turn an existing projection into an empty project.

### Import preflight

A fence-aware importer reads and validates the `epochs.sql` header before it
creates a state directory, creates a SQLite file, starts hydration, or parses
another table's SQL body. An unsupported generation returns
`storage.writer_incompatible` with surface `projection`. Malformed metadata
returns the distinct stable projection-metadata failure.

After preflight succeeds, import uses RFC 10178's ordinary dependency-ordered
table list and sorted SQL format. The generation header does not add a manifest,
change table identity, serialize `__schema_history`, or replace deterministic
round-trip validation.

The importer reads every projection file and validates the complete import in
an isolated in-memory database before it creates the target parent or SQLite
file. Clearing and populating an existing target then occurs in one SQLite
savepoint. A parse, constraint, or foreign-key failure therefore cannot expose
a partially cleared canonical database.

### Export

The projection publication invariant is **never under-fenced**. Every SQL body
is derived from one canonical database snapshot together with that snapshot's
required generation. Publication may conservatively expose a generation newer
than the currently visible SQL bodies, but it must never expose a file that
requires generation G while `epochs.sql` advertises a value lower than G.

When publication raises the projection generation, Exo first atomically
replaces `epochs.sql` with the strict raised header and the snapshot's epochs
body. Only after that replacement is durable may it replace another table file
whose content requires the higher generation. It then replaces the remaining
files from the same snapshot using the ordinary per-file atomic-write path.

This order does not claim directory-wide atomicity. A crash can leave a
conservatively over-fenced, stale, or partially advanced projection, and the
export remains incomplete until a compatible writer regenerates every file.
The safety claim is narrower: no successfully published file is under-fenced.
A future directory commit protocol may strengthen snapshot atomicity without
changing the generation rule.

A compatible repair export checks the available projection and its generation
before opening canonical SQLite, but it does not require the interrupted files
to pass complete relational import validation. The canonical database remains
the repair source. This permits regeneration when a crash published a parent
table and dependent table from different snapshots under a safely
conservative header.

An exporter must not lower the generation merely because a newer binary did not
touch a feature that required it. Lowering the compatibility floor requires a
proven downgrade that rewrites or validates every retained logical value under
the older writer contract. During such a downgrade, Exo publishes all
downgraded table bodies while retaining the higher header, then lowers the
strict `epochs.sql` header last. A failure is therefore over-fenced, never
under-fenced.

### External projection mutation boundary

The settled-Git check detects repository operations and unresolved index
conflicts that Git exposes before Exo samples projection compatibility. Exo's
fresh-clone importer reads the table files into one retained in-memory candidate
and validates that candidate before creating or mutating SQLite. Cached
semantic open and repair export sample projection availability and the
`epochs.sql` generation before opening SQLite, and Exo's own publisher uses the
ordered atomic replacements above.

This does not serialize an unrelated Git process or filesystem writer with the
subsequent semantic open or export. An external process can change projection
files after the settled check or compatibility sample, and a Git operation that
does not yet expose a repository marker or conflict may be indistinguishable
from ordinary file replacement. The generation-zero implementation therefore
claims fail-closed behavior for the projection metadata it observes and
never-under-fenced ordering for Exo's own publication; it does not claim that
the projection sample remains unchanged until the SQLite operation completes.

Stage 2 must decide whether this boundary needs stronger coordination, such as
retaining one verified projection snapshot for the whole operation or using an
explicit cross-process projection/Git authority. That decision must account for
Git tooling outside Exo and may not be inferred from the SQLite compatibility
lock, which does not cover external projection writers.

### Semantic merge

The writer generation is typed projection metadata in both repo-policy and
sidecar-policy merge paths, but their orchestration guarantees differ.

The sidecar merge is Exo-orchestrated. It preflights the base and every input
projection before mutating the sidecar checkout, SQLite cache, or output
projection. Unsupported or malformed generation metadata stops that merge
before mutation.

Repo-policy projection files currently participate in Git's per-file merge
machinery. A per-file driver cannot preflight the whole projection before Git
starts changing the worktree. The strong pre-mutation guarantee would require a
future Exo-owned repository merge coordinator. Until that exists, Exo refuses
every semantic projection read, hydration, or export while Git reports an
unsettled merge operation or unresolved index conflicts. The `epochs.sql` merge
driver treats the header as typed metadata and renders at least the maximum
generation from its base, ours, and theirs. After Git settles, Exo preflights
the completed projection before any semantic use. Git's transient unsettled
worktree is quarantined input, not a published semantic projection;
never-under-fenced applies to the completed projection Exo is willing to read.

For either policy, a completed compatible merge advertises at least the maximum
generation from the base and all inputs. The semantic row merge proceeds under
RFC 10178's stable logical identities, and the renderer always emits the strict
`epochs.sql` header even when every input was legacy. Conflict resolution may
raise the result generation when the resolved rows require it; it may not lower
the conservative maximum. A lower generation requires the explicit proven
downgrade path above, not an ordinary merge or manual choice of one header.

## Failure Contract

An unsupported database or projection returns the stable kind:

```text
storage.writer_incompatible
```

The structured details contain:

```text
required_generation
supported_generation
state_surface          database | projection
upgrade_action
```

`upgrade_action` is a product instruction to run a compatible Exo, not a raw
SQLite repair recipe. The result is a precondition failure: no request outcome
is ambiguous because no semantic operation began.

Malformed projection metadata is distinct from incompatibility. It means Exo
cannot establish which writer contract governs the files. The failure identifies
`epochs.sql` and the header problem, and it also occurs before target database
creation. Database or projection values outside the shared non-negative
signed-32-bit range, or outside the projection's canonical decimal encoding,
are treated as malformed compatibility metadata rather than silently mapped to
legacy.

The stable malformed-metadata kind is
`storage.writer_metadata_invalid`. Compatibility-lock timeout uses
`storage.compatibility_busy`. All three compatibility failures map to Exo's
existing `precondition_failed` daemon and MCP surface with
`request_outcome_checked=false` and `retry_with_same_request_id=true`; only the
bounded busy result is retryable.

## Daemons and Direct CLI Access

Daemon executable identity remains valuable. It normally replaces or avoids a
daemon whose executable no longer matches the installed caller. That behavior
reduces surprising mixed-version use and should remain the ordinary path.

The storage fence is nevertheless authoritative. Daemon startup, restored
daemon state, direct CLI commands, one-shot processes, tests, and any internal
storage consumer all enter through the same compatibility preflight. A caller
cannot bypass the fence by avoiding `daemon ensure`, and an existing daemon
cannot continue merely because its process identity remains live.

When the daemon encounters incompatible state, it returns the stable storage
failure. It does not restart repeatedly, delete caches, rebuild from a
projection it cannot understand, or signal another process as a recovery
strategy.

The lock order is domain and ownership authority first, compatibility lease
second, then SQLite connection and transaction locks. No compatibility path may
acquire project ownership or another domain lock while holding the storage
lease.

## Release Baseline

The fence becomes enforceable through deliberate release sequencing.

Release N introduces the generation carriers, preflight checks, stable errors,
and validation while continuing to write generation zero or N-compatible
state. That release is installed and accepted as the compatibility baseline.
Only a later release N+1 may introduce storage whose minimum writer generation
is greater than the baseline.

This ordering is part of the feature, not rollout paperwork. Shipping the fence
and the first incompatible state in one release would leave the immediately
previous binary unaware of the fence. In particular, no new marker can make a
pre-fence projection importer reject files it was never implemented to inspect.

The enforceable guarantee therefore begins with fence-aware release N:

- N+1 may invisibly migrate and write N state.
- N rejects N+1 database and projection state before mutation.
- Pre-fence binaries are outside the enforceable projection-only guarantee.

Exo documentation and rollout evidence must say this directly. The RFC does
not claim retroactive protection for already-released binaries.

Every fence-aware artifact exposes its compiled generation through the
read-only `exo storage compatibility` command. Release verification records
that value alongside the artifact identity. The retained N artifact is the
cross-version fixture used when N+1 first writes generation one; publishing or
installing that fixture remains a release operation rather than part of normal
database migration.

## Compatibility Guarantees and Limits

For a closed SQLite database, rejection by a fence-aware older binary leaves the
database bytes, migration history, and logical rows unchanged. For a live WAL
database, byte-for-byte equality of all files is not a stable assertion; the
guarantee is that Exo began no write transaction and canonical logical state did
not change.

For a projection requiring a newer writer, rejection occurs before target
directory or database creation. The projection files remain byte-identical.

The fence does not solve concurrent writer ownership, sidecar Git ownership,
projection merge conflicts, semantic downgrade transforms, or executable
replacement. It establishes only whether this binary is permitted to become a
semantic reader or writer of the observed state.

## Relationship to Existing RFCs

RFC 10178 owns the sorted SQL projection. This RFC adds the projection's writer
generation header and preflight while preserving deterministic per-table SQL,
dependency ordering, and the exclusion of `__schema_history`.

RFC 10191 owns sidecar write ownership and stale-writer fencing. Ownership asks
which compatible runtime may checkpoint shared state. This RFC asks whether a
binary understands the state well enough to participate at all. Compatibility
is checked before ownership can authorize a write.

RFC 10207 depends on this fence before adding project-motion storage. Its typed
RFC objectives and pull-request relationships use ordinary migrations and RFC
10178 projections after compatibility succeeds; they do not need a private
manifest or a second hydration protocol.

Repo and sidecar policies carry the strict projection header because both
publish portable SQL. Shadow policy carries the same SQLite fence but has no
portable projection carrier by design; it therefore performs database
preflight only.

## Alternatives

### Treat migration history as the compatibility floor

The highest migration identifier is not a semantic writer contract. Exo already
supports missing known migrations and out-of-order application, and some
compatibility changes span non-SQL behavior. Conflating the concepts would make
legitimate history look corrupt and would still miss projection-format changes.

### Rely on daemon executable identity

That is an important ergonomic defense, but it does not cover direct opens,
existing daemons, fresh clones, or non-daemon storage consumers. It cannot be
the authority carried by the state.

### Add a standalone manifest

An older exporter can preserve an unknown file while rewriting the projection
files it does know, leaving the manifest stale. More importantly, a pre-fence
importer will not consult a newly invented standalone file. A header in the
already-required first projection table is the smallest format extension that
every fence-aware importer can preflight.

### Attempt full downgrade compatibility

Teaching every older binary to preserve every future semantic extension is not
possible. Explicit downgrade migrations may be designed for particular
versions, but they are separate operations. The default contract is invisible
forward migration and fail-closed older writers.

### Permit read-only use of newer state

Exo's semantic read path may mutate storage during setup, and an older model can
misrepresent unknown authority even if SQLite itself is opened read-only. A
narrow generation probe is honest; a general compatibility bypass is not.

## Implementation Direction

The first implementation should centralize the low-level contract rather than
scatter version checks through commands. A storage compatibility module should
own generation parsing, SQLite probing, structured errors, and the compiled
supported generation. Database opening and migration should call it before any
current initialization side effect. Projection import should call it before
filesystem or database creation, while projection export should emit the
canonical header.

The cross-process compatibility lock must contain the second generation check,
any conservative generation raise, known migration application, and final
generation verification. Higher-level Exo context, daemon, API, and CLI layers
should map the same typed failure rather than reconstructing compatibility
judgments independently.

Implementation is expected to touch the storage open, migration, dump/import,
and Exo context boundaries. The exact module split remains an implementation
decision as long as every semantic open reaches one authoritative check.

## Validation

The implementation must prove:

- legacy databases and projections with generation zero or no header migrate
  invisibly;
- a current binary applies every missing known migration, including known
  out-of-order migrations, after compatibility succeeds;
- two independent processes serialize generation-advancing migration through
  the operating-system compatibility lock;
- a semantic `Database` or request-scoped storage object retains its shared
  lease through its final SQL access, and a generation-advancing migration
  cannot acquire the exclusive lease until every older semantic user closes;
- a crash immediately before the conservative raise publishes no incompatible
  state, while crashes after the durable raise and throughout migrations remain
  safely over-fenced and resumable;
- migrations that change `foreign_keys` or otherwise cannot share one SQLite
  transaction still publish the higher generation before incompatible writes;
- a compiled fence-aware N fixture rejects an N+1 closed database without
  changing its bytes, history, or logical rows;
- the same fixture rejects an N+1 projection before creating a directory or
  database and leaves every projection file unchanged;
- a live-WAL rejection begins no write transaction and leaves canonical logical
  state unchanged;
- the current N+1 binary opens and migrates N database and projection state
  without a user-managed step;
- malformed and overflowed database or projection metadata return their stable
  metadata failure rather than incompatibility or legacy behavior;
- direct CLI, daemon, restored-daemon, request, and projection-only entry paths
  all use the preflight;
- executable-aware daemon replacement remains ergonomic but cannot bypass the
  state-carried authority;
- sidecar semantic merge preflights every input before mutation, chooses at
  least the maximum generation, and always renders the strict header;
- repo-policy merge quarantines unsettled Git state from semantic reads, its
  `epochs.sql` driver retains the maximum input generation, and Exo preflights
  the settled projection before semantic use;
- raised-generation projection publication replaces the strict header before
  higher-generation bodies, and proven downgrade publication lowers it last;
  and
- failures during projection publication may leave stale or conservatively
  over-fenced output but never expose a newer logical file under an older
  generation.

The generation-zero implementation keeps two evidence layers distinct. A
source-level synthetic generation-one migration proves rejection, invisible
forward migration, and resumption after an interruption immediately following
the conservative raise. A subprocess test proves that an independently running
generation-advancing writer cannot acquire exclusive authority while another
process retains a shared semantic lease. The retained compiled N binary and the
full N/N+1 artifact matrix remain release evidence rather than being inferred
from those source-level tests.

The organic proof is the cross-version experience: release N fails closed on
N+1 state with an actionable upgrade instruction and no mutation, while N+1
opens N state and continues without asking the user to understand migrations.

## Stage 1 Resolved Design Choices

Stage 1 fixes the implementation contract for the generation-zero baseline:

- `PRAGMA user_version` is read by a read-only, no-create compatibility probe
  before writable pragmas, migrations, virtual-table setup, or parent creation.
- `fs2` shared/exclusive locking at the persistent
  `writer-compat.lock` sibling serializes semantic lifetimes and migration, with
  a five-second acquisition bound and a mandatory serialized re-probe.
- Every known migration declares `required_writer_generation`; the migration
  runner takes the maximum across all missing known migrations without assuming
  contiguous or numerically ordered history.
- Database, repo projection, and sidecar projection entry paths share the typed
  compatibility failures and `precondition_failed` protocol mapping.
- Repo and sidecar semantic opens preflight an available canonical projection
  before opening an existing SQLite cache; the cache and projection jointly
  define the open compatibility floor without forcing cache replacement.
- `exo storage compatibility` is the artifact verification hook, while retained
  binary publication and the N/N+1 execution matrix remain rollout evidence.
- Repo and sidecar policies carry the strict `epochs.sql` header, while shadow
  policy intentionally carries only the SQLite fence.
- Project-motion storage from RFC 10207 may not land until this generation-zero
  fence is validated and retained as the older-writer fixture.

Stage 2 review should test whether these choices are implementation-ready and
whether the cross-version proof closes every canonical open, projection, and
merge path. It should not reopen the user-facing goal of invisible forward
migration and fail-closed older writers without new evidence.
