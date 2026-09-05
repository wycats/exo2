<!-- exo:10207 ulid:01m0e1az3h7tz2mckprpqd6vym -->

# RFC 10207: Cockpit Project Flow: Lanes, Campaigns, RFCs, Pull Requests, and Coordination

**Status**: Stage 2 (Draft)
**Feature**: cockpit-project-flow
**Related**: RFC 00230, RFC 00238, RFC 0108, RFC 10176, RFC 10178, RFC 10181, RFC 10191, RFC 10202, RFC 10204, RFC 10205, RFC 10208

## Summary

The Exo cockpit should present one coherent account of how a project is moving.

The project is the durable object being changed. RFCs, pull requests,
validation, documentation, release, dogfooding, and coordination are not
separate kinds of progress. They are complementary ways that one intended
improvement becomes more clearly specified, more faithfully implemented, and
more confidently understood. Exo should model that movement as a sequence of
bounded **project deltas** that leave the project at incrementally better,
legible checkpoints.

A durable **lane** carries the continuity of an intended improvement across
time. Within that lane, sequential bounded **campaigns** coordinate the
commitments that can actually be planned, executed, reviewed, and closed. RFC
advancement makes the desired project more precise and authoritative.
Pull-request delivery realizes or validates that decision in the
implementation. Validation, documentation, release evidence, and dogfooding
test whether the specification and the experienced project agree.

The existing `phase` entity is the transitional storage, command, ownership,
and lifecycle implementation of a campaign. The first implementation slice in
this RFC does not rename phases or migrate lane storage. It makes the current
campaign's RFC objective and pull-request delivery legible as one **Project
Motion** projection in the active and inspected lane views.

This Stage 2 draft is deliberately implementation-ready. RFC 10208's separately
shipped storage-writer fence is the remaining prerequisite before
project-motion implementation may begin. No command, campaign completion,
pull-request merge, or provider observation performs an RFC stage transition.

## Motivation

The lane workbench has made current execution easier to see, but understanding
the project still requires reconstruction. A person sees the focused lane and
its campaign plan in the cockpit, reads an RFC elsewhere to understand the
decision being advanced, visits GitHub to learn whether the implementation is
under review, and relies on conversation to connect those facts. Each surface
is locally truthful. None presents the whole movement.

The agent performs the same reconstruction with more internal identifiers. It
maps goal RFC numbers to RFC records, worktrees to branches, branches to pull
requests, review findings back to tasks, and user steering back to the current
plan. When those relationships are implicit, even a routine question such as
"what is moving?" requires a fresh synthesis. When relationships are inferred
from titles, branch names, timestamps, or one active Git remote, that synthesis
can also be wrong.

This fragmentation obscures the meaning of completion. An RFC promotion can
look like success while its implementation remains partial. A merged pull
request can look finished while ordinary use reveals that the experience is
still wrong. A completed campaign can contain useful delivery evidence without
having advanced an RFC at all. The cockpit needs to preserve those distinctions
while showing that they belong to one project delta.

The product should remove the reconstruction burden without creating a second
lifecycle authority. From a lane, a person should be able to understand the
decision in motion, its possible RFC advancement, the delivery artifacts that
implement or validate it, the freshness of external observations, and the
current execution plan. RFC promotion, Exo goal and campaign completion, and
human approval remain explicit actions owned by their existing systems.

## Project Evolution as Convergence

Project flow is not a collection of artifact progress bars. It is the
project's movement from its current reality toward a more clearly specified and
incrementally better implemented reality.

That movement has four related forms:

| Motion | Project question | Typical evidence |
| --- | --- | --- |
| **Decision** | What should become true, and how firmly has the project decided it? | RFC stage advancement, accepted design, settled relationships |
| **Realization** | How much of that decision has become implemented and accepted? | Workspaces, commits, pull requests, reviews, checks, merge and release state |
| **Coherence** | Does the implemented and understood project agree with the accepted specification? | Tests, verification, manuals, finalized specs, release evidence, dogfooding |
| **Learning** | What did ordinary use reveal about the next improvement? | Progress evidence, coordination events, diagnostics, successor RFC objectives |

A campaign coordinates one bounded project delta across these forms of motion.
Not every campaign needs an RFC or pull request, and one campaign need not take
an RFC all the way to Stable. Each completed campaign should nevertheless leave
the project at a legible checkpoint from which the next campaign can begin.

The forms of motion will often be uneven. Specification can lead implementation
while the project works out how to realize an accepted direction.
Implementation can lead specification when a prototype reveals a capability or
constraint the current design did not name. Dogfooding can show that code and
specification agree with each other but not with the experience the project
intended to create.

A campaign does not need to erase every divergence before it closes. It must
make the remaining divergence explicit, preserve the evidence that was
established, and identify successor motion when more work is warranted. A
coherent checkpoint is therefore a truthful baseline, not a declaration of
perfection.

Within this broader model, the RFC pipeline remains the privileged and
authoritative account of decision motion and project canon. Delivery,
coherence, and learning can move independently, but they do not replace RFC
stage authority or turn RFCs into ordinary work attachments. Project evolution
is the cockpit's top-level story; RFC and pull-request views are projections
over that story.

## Lanes and Campaigns

A lane answers:

> What durable stream of intent am I following?

A campaign answers:

> What bounded project delta are we committing to now, and what would establish
> that it happened?

A lane can contain sequential campaigns. Prior campaigns remain inspectable as
history, one campaign may be active, and a future campaign may be prepared.
Campaign completion records what changed and what remains without declaring
that the lane's subject has no future.

The physical model does not yet express that complete relationship. Today a
`workbench_lanes_data.execution_phase_id` points to one phase. For this RFC's
first implementation slice, that phase is the lane's current campaign and the
existing phase owns goals, tasks, ordering, execution ownership, completion
review, and outcome. The following remain unchanged:

- the phase state machine and ownership checks;
- workspace-local lane and phase focus;
- the `phase -> goal -> task` foreign-key hierarchy;
- historical lane inspection through the lane's associated phase;
- explicit phase and goal outcome approval.

The cockpit and every new public command in this slice use *campaign* vocabulary.
Their campaign selectors resolve to the transitional internal phase identity
described above. Existing `exo phase` commands and status surfaces retain their
compatibility vocabulary. A later migration can add durable
lane-to-campaign history and move the lane's current-phase pointer without
changing the project-motion relationships defined here.

## The First Project-Motion Slice

The first slice proves one narrow but complete claim:

> A person inspecting a lane can see which RFC decision the current campaign is
> trying to advance, which pull request implements or validates it, and what
> current provider evidence says about that delivery.

The slice has four boundaries:

1. campaign-to-RFC objectives and campaign-to-PR delivery relationships are
   explicit portable project state;
2. GitHub observations are machine-local evidence refreshed only by an explicit
   command;
3. workbench snapshot and inspection reads project the stored state without
   performing provider I/O;
4. the cockpit is read-only for these relationships.

Dedicated RFC and pull-request screens, background refresh, browser mutation,
coordination receipts, and automatic Exo or RFC lifecycle transitions are not
part of this proof.

## Canonical Campaign RFC Objectives

### Separate typed authority

New typed objectives live in a separate portable
`campaign_rfc_objectives_data` table. `phase_rfcs_data` remains unchanged as
legacy compatibility input.

This separation is an authority requirement rather than a naming preference.
The legacy `replace_phase_rfcs` implementation deletes every phase RFC row
before recreating a legacy `related` set. Storing typed objectives in that table
would let an ordinary legacy update erase canonical project motion even in a
current binary. The separate table gives typed relationships their own schema,
commands, revision identity, and projection file.

Cross-version protection comes from RFC 10208, not from relying on an older
binary to preserve a table it does not understand. The storage-writer fence
must ship as an accepted compatibility baseline before this RFC introduces a
new writer generation for project-motion state. A fence-aware older binary then
rejects the newer database or projection before semantic opening or mutation.

The portable objective row has this logical shape:

```text
text_id              stable Exo ULID for the objective
phase_id             -> phases_data.id
rfc_ulid             stable RFC ULID value; not a foreign key
rfc_number_snapshot  RFC number when the objective was established
rfc_title_snapshot   RFC title when the objective was established
observed_stage       stage when the objective was established
target_stage         optional target stage
relation             drives | implements | validates
created_at           RFC 3339 creation time
updated_at           RFC 3339 time of the latest explicit objective update
```

`(phase_id, rfc_ulid)` is unique. `phase_id` references `phases_data` with
`ON DELETE RESTRICT`, so a phase cannot be deleted until its typed objectives
are explicitly detached. The durable RFC ULID is stored as a value, not as a
foreign key to `rfcs_data`. That table is reconciled from the current workspace
corpus and an RFC may temporarily disappear from the effective view. Removing
that row must not cascade, invalidate, or make the campaign objective
unreadable.

New project-motion writes use exactly:

- `drives`: the campaign is intended to produce evidence for an RFC stage
  advancement;
- `implements`: the campaign realizes an already stated RFC decision;
- `validates`: the campaign establishes evidence that the implementation and
  decision agree.

A campaign and exact RFC ULID have at most one typed relation. `observed_stage`
and the display snapshots are immutable establishment evidence. Relation and
target may change only through another explicit attach using the same exact
identity.

When the effective RFC corpus contains `rfc_ulid`, reads use its current title,
number, stage, and lifecycle while retaining the establishment snapshot. When
the identity is absent, reads retain the objective, use the snapshot for
display, set current lifecycle fields to unavailable, and emit
`project_flow.rfc_identity_missing`. If the same ULID reappears after
reconciliation, the objective reconnects automatically. An RFC with the same
number but a different ULID never satisfies the relationship.

An optional `target_stage` must be in Stage 0 through Stage 4 and strictly
greater than the current stage when a new target is attached. The target
describes a potential advancement this campaign is trying to make possible.
Reaching it does not happen through this relationship; it happens only through
the RFC promotion workflow.

An objective without a target remains a useful implementation or validation
relationship. `drives` normally carries a target, but the storage contract does
not invent one when it is absent.

### RFC selector resolution

The public selector accepts either an exact RFC ULID or a decimal RFC number,
with or without leading zeroes. Resolution depends on the operation.

Attach requires a live canonical RFC row: the selected ULID must be present in
the effective `rfcs_data` view after reconciliation. A numeric attach resolves
only when exactly one canonical row has that number. This establishes the
durable ULID and display snapshot from current authority rather than from a
missing or historical objective.

Detach first resolves the specified phase. An exact ULID then resolves directly
against `campaign_rfc_objectives_data` for that phase, even when the effective
RFC corpus no longer contains the identity. This makes an identity-missing
objective removable without restoring its document. A numeric detach still
uses the canonical RFC view and succeeds only when exactly one current row has
that number; it then detaches that row's ULID from the phase. Numeric detach
never searches establishment snapshots or guesses among missing identities.

Zero matches return not found. More than one numeric match returns the stable
`project_flow.rfc_ambiguous` precondition failure with the candidate ULIDs,
titles, stages, statuses, and paths needed to choose an exact identity. Exo
never chooses the active, newest, highest-stage, or nearest workspace document
on the caller's behalf.

### Legacy compatibility and migration

Migration creates the new table and does not copy or rewrite
`phase_rfcs_data`. A number-only legacy row does not contain enough evidence to
mint a durable RFC ULID, even when the current corpus happens to contain one
matching number. Creating canonical identity from that coincidence would turn
observation into authority.

New reads may resolve legacy phase rows against the current effective corpus
for compatibility presentation. They use the same exact-one-match rule as a
numeric selector and mark the result `legacy_phase`; zero or several matches
produce diagnostics. This resolution is ephemeral and never writes a typed
objective. `goals_data.rfc` receives the same treatment as `legacy_goal`.

Portable dumps preserve `phase_rfcs_data` exactly as before and serialize
`campaign_rfc_objectives_data` independently, including its RFC ULID and
establishment snapshots. Import never consults the receiving workspace's RFC
corpus to reconstruct identity.

### Goal compatibility

`goals_data.rfc` and `goals_data.target_stage` remain supported compatibility
fields. This slice does not delete, rewrite, or backfill them, and attaching a
campaign objective does not mutate a goal.

Project-flow reads prefer a new typed campaign objective. If none exists, they
may project legacy phase relationships and uniquely resolved goal RFC metadata
as compatibility candidates. Goal metadata with a target appears as legacy
`driving`; metadata without a target appears as legacy `related`. Conflicting
goal targets or ambiguous RFC numbers produce diagnostics instead of a guessed
canonical objective. Once a typed objective exists for an exact RFC identity,
its decision semantics win and duplicate legacy candidates are suppressed from
the primary Project Motion band.

`rfc pipeline` and steering use the same resolver. They prefer typed objectives,
retain legacy relationships as compatibility evidence, and surface unresolved
or conflicting legacy input. This prevents the cockpit, pipeline, and steering
from telling different stories about the same campaign.

`phase update --rfcs` keeps its existing legacy behavior inside
`phase_rfcs_data`. It does not read, update, replace, or delete
`campaign_rfc_objectives_data`. Typed objectives are created and removed only
through `project-flow rfc attach` and `project-flow rfc detach`.

## Pull-Request Delivery Records

### Portable identity and relationship

A pull request is an explicit delivery artifact, never an inference from the
current branch, worktree, commit message, remote, or open GitHub page.

Portable `project_flow_pull_requests_data` rows have this logical shape:

```text
text_id       stable Exo ULID
provider      provider identity; initially "github"
repository    provider-qualified repository identity
number        provider pull-request number
url           canonical public artifact URL
created_at    RFC 3339 creation time
```

`(provider, repository, number)` is unique. Repository identity is normalized
according to the provider adapter; the GitHub adapter uses lowercase
`owner/name`. The URL is data for presentation and navigation, not an authority
or lookup key.

Portable `phase_pull_request_relations_data` connects a phase and artifact with
one role:

- `implements`: the artifact delivers implementation of the campaign delta;
- `validates`: the artifact primarily establishes validation or coherence
  evidence.

One pull request may serve several campaigns through separate explicit
relationships. One campaign may have zero, one, or several delivery artifacts.
No relationship is inferred from Git history.

Its `phase_id` references `phases_data` with `ON DELETE RESTRICT`. Typed PR
motion must therefore be explicitly detached before phase deletion, and an old
binary deleting a phase cannot cascade relationships it does not understand.
Its `artifact_id` references `project_flow_pull_requests_data` with
`ON DELETE RESTRICT`. Artifact cleanup is separate: after removing a
phase-to-PR relation, the same transaction may delete the artifact only when no
relation still references it. The artifact's machine-local observation uses
`ON DELETE CASCADE` from artifact deletion. An artifact shared by another phase
and its observation remain.

Both tables participate in deterministic repository and sidecar SQL dumps.
Artifacts are serialized before their campaign relationships, and foreign keys
use stable text IDs rather than database row IDs.

### Machine-local observations

Provider facts are evidence observed by one machine, not portable project
policy. `project_flow_pull_request_observations_data` is therefore reactive
machine-local state and is excluded from repository and sidecar dumps.

One observation row per artifact records:

```text
artifact_id           -> project_flow_pull_requests_data.id
title                 title from the last successful observation
lifecycle             open | closed | merged
head_oid              provider head revision, when available
review_state          none | pending | approved | changes_requested | unknown
checks_state          none | pending | passing | failing | unknown
last_success_at       RFC 3339 time of the last successful observation
last_attempt_at       RFC 3339 time of the latest attempt
last_error            sanitized latest failure, or null after success
```

The successful fields and `last_success_at` change only after a complete,
valid provider response. Every attempt updates `last_attempt_at`. A failed
attempt sets `last_error` while preserving the last successful title,
lifecycle, head, review, checks, and time. A later success replaces the
successful fields and clears `last_error`.

Provider errors are bounded and sanitized before storage. They may identify the
provider operation and stable error class, but they do not include access
tokens, command environments, private filesystem paths, or unbounded response
bodies.

### Reactive storage contract

All four project-motion entities participate in the existing reactive SQLite
contract. The migration creates these virtual-table, shadow-table, and row
revision-table triples:

```text
campaign_rfc_objectives              campaign_rfc_objectives_data              campaign_rfc_objectives_rev
project_flow_pull_requests           project_flow_pull_requests_data           project_flow_pull_requests_rev
phase_pull_request_relations         phase_pull_request_relations_data         phase_pull_request_relations_rev
project_flow_pull_request_observations project_flow_pull_request_observations_data project_flow_pull_request_observations_rev
```

Each `*_data` table uses an integer primary key suitable for stable reactive
row identity. Its matching `*_rev` table stores the BLAKE3 content digest by
rowid. The migration also inserts a zero seed into `rowset_revisions` for each
of the four `*_data` names, using `ON CONFLICT DO NOTHING` so an existing
counter is never reset. All four virtual/shadow pairs are registered in
`REACTIVE_TABLES`; ordinary reads and writes use the virtual names, while only
trusted migration, projection, and recovery code may address the shadow tables
with defensive mode managed explicitly.

An insert or delete changes membership and atomically advances the persistent
rowset counter. An insert or update writes the row digest, and a delete removes
it. Readers that observed membership through `xFilter` are invalidated by the
rowset counter; readers that observed row content through `xColumn` are
invalidated by its digest. The final project-flow transaction updates domain
rows and these revision records together, then publishes one workbench revision
after commit. A rollback publishes no invalidation. Observation-only refreshes
therefore invalidate delivery evidence without pretending that a portable
relationship changed, while attachment changes invalidate both the affected
row content and relationship membership.

The portable projection includes
`campaign_rfc_objectives_data`, `project_flow_pull_requests_data`, and
`phase_pull_request_relations_data`. Only
`project_flow_pull_request_observations_data` is machine-local and excluded
from repository and sidecar dumps; being non-portable does not make it
non-reactive.

That statement covers the four project-motion entity authorities. Prepared
request recovery records belong to V021 execution infrastructure rather than
project motion and are not projection candidates.

## Public Command Contract

The public namespace is `exo project-flow`. The first slice exposes exactly:

```text
exo project-flow rfc attach <rfc> --campaign <id> --relation <drives|implements|validates> [--target-stage <0-4>]
exo project-flow rfc detach <rfc> --campaign <id>
exo project-flow pr attach <owner/repo#number|url> --campaign <id> --role <implements|validates>
exo project-flow pr detach <owner/repo#number|url> --campaign <id>
exo project-flow refresh [--campaign <id>]
exo project-flow status [--campaign <id>]
```

Campaign is the public concept for new project-flow surfaces. In this
transitional slice, campaign selectors resolve internally to existing phase
identities through the exact phase and alias resolver. Existing `exo phase`
commands remain compatibility surfaces, but new `project-flow` commands do not
introduce a second phase-named flag spelling.

When optional `--campaign` is absent from `refresh` or `status`, Exo uses the
current workspace's active phase as its current campaign; no active campaign is
a precondition failure. Attach and detach require `--campaign` so a
relationship never follows ambient focus by accident.

RFC selectors obey the operation-specific identity rules above. The
GitHub-first PR selector
accepts either `owner/repo#number` or
`https://github.com/owner/repo/pull/number`. Other hosts, malformed paths, zero
numbers, fragments, query strings, and implicit current-repository shorthand
are rejected in this slice.

`rfc attach` upserts the one typed relation for the exact phase and live RFC.
Repeating the same relationship is a successful no-op; changing relation or
target is an explicit update. `rfc detach` removes only the typed relation
selected by the detach rules and leaves legacy rows untouched.

`pr attach` and `refresh` use a new recovery class,
`RecoveryClass::PreparedExternalRead`, serialized as
`prepared_external_read`. It is distinct from `atomic_project_state`: its
external reads are safe to repeat, but they cannot be recovered from the
absence of a V021 outcome unless the exact prepared input also survived. It is
also distinct from `external_at_most_once`, because repeating the bounded,
read-only provider fetch after a crash does not repeat a mutation.

The class has a durable, non-portable prepared-request record keyed by request
ID. The reservation transaction stores:

- the normalized request hash and complete normalized payload;
- the exact phase text ID;
- the ordered provider and artifact identities to fetch, including the intended
  role for an attachment;
- the issuing daemon's instance ID, PID, and process-start identity; and
- the recovery class and preparation time.

For `refresh`, the ordered identities are the attachment membership observed at
reservation time. They are not recomputed after takeover. For `pr attach`, the
prepared set contains the one normalized provider artifact and proposed
relationship.

Execution proceeds in three stages:

1. Reserve or recover the request ID. A committed V021 outcome replays
   immediately without provider I/O. A different normalized payload is a
   request conflict. A new request resolves and validates its phase and
   attachment membership, then commits the complete prepared record before any
   provider process starts.
2. Run each bounded, read-only provider fetch from that persisted prepared
   input with no SQLite write transaction open.
3. Open one `BEGIN IMMEDIATE` transaction, revalidate the prepared phase and
   relationships, and atomically commit the portable relationship changes,
   successful or failed observation attempts, reactive and workbench revisions,
   the terminal V021 outcome, and closure of the prepared request.

The portable relationship succeeds even when GitHub is unavailable; the final
transaction records the failed attempt and returns it truthfully. No provider
subprocess runs while `BEGIN IMMEDIATE` is open.

Same-instance callers using the same request ID are waiters, not executors.
They wait on that request's completion notification, re-read V021 when woken,
and never perform provider I/O. A live but different owner remains authoritative
and other instances report or wait on the in-flight request according to the
daemon's bounded admission contract. A replacement daemon may claim the
prepared request only after exact process-identity validation proves that the
recorded owner is no longer current. Takeover atomically replaces the owner
identity while preserving the request hash, normalized payload, exact phase,
and ordered target set byte-for-byte.

A caller disconnect is not abandonment; the bounded executor continues toward
one terminal outcome. A crash after a provider fetch but before the final
commit leaves no partial relationship, observation, revision, or outcome. The
replacement may repeat the safe fetch, but only for the persisted targets. A
crash after the final commit replays V021 and performs no provider I/O.
Graceful shutdown leaves an unfinished prepared request eligible for the same
identity-checked takeover; it does not delete the recovery input.

Preparation-time validation failure commits one terminal V021 precondition
outcome without starting a provider process or creating an open prepared
request. After preparation, abandonment is itself a transaction: it records an
`abandoned` terminal state and canonical V021 error, closes the prepared
request, and wakes waiters. An unresolved prepared record is never deleted to
manufacture permission to execute again.

If final revalidation finds that a prepared phase or relationship no longer
matches, the transaction commits one terminal
`project_flow.prepared_input_changed` precondition outcome, closes the prepared
request, discards the fetched evidence, and changes no project-flow rows or
revisions. An executor that cannot complete its bounded provider stage commits
the relationship and failed observation where that is valid; provider failure
by itself is not abandonment. Every terminal commit or abandonment wakes all
local waiters, which obtain that same canonical outcome. An uncommitted crash
wakes no waiter falsely; ownership validation and takeover provide the next
progress event.

`pr detach` removes the selected campaign relationship and removes an
unreferenced artifact and its local observation. It never removes an artifact
still used by another campaign.

`refresh` uses that same prepared executor. A failure for one artifact does not
prevent the remaining prepared reads. Its one final transaction either records
all successful and failed attempts with one revision and terminal outcome, or
commits the terminal revalidation failure without applying any of them.
`status` reads only stored project and observation state and never contacts a
provider.

All writes use Exo's normal request envelope, V021 outcome ledger,
transactional SQLite writer, revision publication, and post-write persistence.
Natural-key upserts make attach and refresh idempotent; the
`prepared_external_read` executor makes their external reads durably
recoverable. A committed portable relationship is never reported as absent
merely because its observation attempt failed.

## GitHub Provider Boundary

GitHub is the first provider, not the data model. The adapter uses the existing
guarded `gh` process boundary and a fakeable process trait. It invokes `gh`
without a shell, inherited token arguments, or command-string interpolation.
The initial implementation may use `gh pr view --repo owner/repo --json ...`
to obtain the canonical URL, title, state, head OID, review decision, and check
rollup in one bounded response.

The adapter maps provider values into the neutral lifecycle, review, and check
states above. Unknown or newly introduced provider values map to `unknown`; they
do not make the response unreadable. Missing `gh`, unavailable authentication,
not found, permission failure, timeout, malformed JSON, and unsupported values
are distinct stable diagnostics.

Provider I/O occurs only during `pr attach` and `project-flow refresh`. It never
occurs during:

- `project-flow status`;
- `workbench snapshot`;
- `workbench inspect`;
- cockpit HTTP snapshot or inspection requests;
- steering or RFC-pipeline reads.

This boundary is necessary for daemon responsiveness and truthful snapshots.
The workbench projects the latest stored evidence, not a network operation
hidden inside a read transaction.

Freshness is expressed by time, not a guessed policy threshold. The projection
always exposes `last_success_at`, `last_attempt_at`, and `last_error`. The
cockpit presents the age of the last successful observation and any later
failed attempt. It does not classify an observation as fresh or stale from a
hard-coded wall-clock duration.

## Workbench Projection

The ordinary `WorkbenchSnapshot` advances from schema version 4 to version 5.
`WorkbenchLaneInspection` advances from schema version 2 to version 3. Both add
the same non-null `project_motion` object for the phase they project:

```typescript
interface WorkbenchProjectMotion {
  campaign_id: string;
  rfc_objectives: WorkbenchRfcObjective[];
  deliveries: WorkbenchDelivery[];
  diagnostics: WorkbenchProjectMotionDiagnostic[];
}

interface WorkbenchRfcObjective {
  rfc_id: string; // durable RFC ULID stored by the objective
  number: number;
  title: string;
  availability: "available" | "identity_missing";
  status: "active" | "archived" | "withdrawn" | null;
  relation:
    | "drives"
    | "implements"
    | "validates"
    | "driving"
    | "related"
    | "blocked";
  source: "typed" | "legacy_phase" | "legacy_goal";
  observed_stage: number | null;
  current_stage: number | null;
  target_stage: number | null;
  motion:
    | "advancing"
    | "target_reached"
    | "associated"
    | "terminal"
    | "identity_missing";
}

interface WorkbenchDelivery {
  artifact_id: string;
  kind: "pull_request";
  provider: string;
  repository: string;
  number: number;
  url: string;
  role: "implements" | "validates";
  observation: WorkbenchPullRequestObservation;
}

interface WorkbenchPullRequestObservation {
  state: "never_observed" | "observed" | "refresh_failed";
  title: string | null;
  lifecycle: "open" | "closed" | "merged" | null;
  head_oid: string | null;
  review_state:
    | "none"
    | "pending"
    | "approved"
    | "changes_requested"
    | "unknown"
    | null;
  checks_state:
    | "none"
    | "pending"
    | "passing"
    | "failing"
    | "unknown"
    | null;
  last_success_at: string | null;
  last_attempt_at: string | null;
  last_error: string | null;
}
```

`campaign_id` is the phase text ID. Snapshot v5 projects the focused lane's
phase when one exists. Inspection v3 projects the inspected lane's phase. When
there are no relationships, the arrays are empty; absence is not a schema
version fallback.

`motion` is derived without provider input. An active RFC below its target is
`advancing`; one at or above its target is `target_reached`; an active objective
without a target is `associated`; an archived or withdrawn RFC is `terminal`;
and an objective whose durable ULID is absent from the effective corpus is
`identity_missing`. This derived field never promotes, reopens, withdraws, or
completes anything.

The snapshot builder loads canonical relationships, RFC records, delivery
artifacts, and machine-local observations inside the same SQLite read
transaction as the phase plan. It samples no GitHub state and opens no new
workspace database. Snapshot and inspection apply identical relationship,
compatibility, ordering, and diagnostic rules.

Objectives sort by relation priority (`drives`, `implements`, `validates`, then
legacy relations), RFC number, and stable RFC text ID. Deliveries sort by role,
provider, repository, number, and artifact text ID. Every comparison is
deterministic ASCII or numeric ordering rather than locale-aware collation.

## Cockpit Experience

The focused and historically inspected lane views render a compact **Project
Motion** band between the lane identity and the execution plan. It has two
parts:

- **Decision** shows the RFC title and number, typed relationship, and stage
  movement such as `Stage 2 -> Stage 3`.
- **Delivery** shows the pull-request number and title, its role, lifecycle,
  review and check summaries, and the age of the last successful observation.

The band is omitted when the campaign has neither RFC objectives nor delivery
records. Legacy relationships may appear with a compatibility treatment, but
diagnostics do not masquerade as typed decision motion.

A never-observed delivery says that provider status has not been refreshed. A
failed first attempt says that provider evidence is unavailable. A failed
attempt after a successful observation keeps the last known title and status,
labels the refresh failure, and continues to show the age of the preserved
observation. It never presents preserved data as current merely because a PR is
still attached.

The band contains navigation links and read-only disclosures only. It does not
attach or detach artifacts, refresh GitHub, promote an RFC, complete Exo work,
merge a pull request, or imply that any of those actions happened. Dedicated
project-home, RFC, and pull-request views remain future projections over the
same typed graph.

## Migration and Compatibility

Project-motion storage may not land until RFC 10208 has shipped and been
accepted as a separate fence-aware compatibility baseline. That sequencing is
a prerequisite of this RFC's storage generation, not an implementation detail
inside the project-flow feature.

After the baseline exists, the migration is additive around current project
authority:

1. leave `phase_rfcs_data` and its legacy replacement behavior unchanged;
2. add the portable `campaign_rfc_objectives_data` table without a foreign key
   to reconciled RFC rows;
3. add portable pull-request artifact and campaign-relation tables;
4. add the machine-local observation table and revision tracking;
5. add the durable non-portable prepared-request record and register
   `RecoveryClass::PreparedExternalRead`;
6. add the portable tables to ordinary deterministic `TABLE_ORDER` dump and
   import handling and explicitly exclude observations;
7. advance the checked Rust and TypeScript workbench schemas together; and
8. publish the project-motion writer generation through the RFC 10208 database
   and projection carriers.

Old databases and projections with no project-motion rows migrate forward
invisibly under the fence-aware current binary. New dumps retain legacy rows,
canonical RFC ULIDs and establishment snapshots, and portable PR relationships.
Import never derives a canonical RFC identity from a number or from the
receiving workspace corpus.

A fence-aware baseline binary encountering the newer project-motion writer
generation rejects the database before writable pragmas, migration,
`Database::new`, or request processing. It rejects the projection by
preflighting the `epochs.sql` generation header before creating a directory or
database. This is the rollback boundary: the older writer does not partially
hydrate or rewrite state it cannot preserve.

Once compatibility preflight succeeds, project-motion data uses RFC 10178's
normal per-table projection model. The portable tables are exported in
dependency order:

```text
campaign_rfc_objectives.sql      -> campaign_rfc_objectives_data
project_flow_pull_requests.sql   -> project_flow_pull_requests_data
phase_pull_request_relations.sql -> phase_pull_request_relations_data
```

They require no project-motion manifest, hydration markers, or feature-specific
new-to-old-to-new reconciliation. Their deterministic SQL bodies, stable text-ID
foreign-key resolution, empty-table representation, and round-trip behavior
follow the same `TABLE_ORDER` contract as other portable authority. Only the
machine-local provider observations and prepared request records remain outside
portable dumps.

Typed RFC objectives and PR relations use `ON DELETE RESTRICT` against their
campaign's phase row. A current binary therefore requires explicit detach
before phase removal. Cross-version safety does not depend on that constraint:
an older fence-aware writer is stopped by RFC 10208 before it can reach the
legacy phase deletion path.

Linked worktrees share one project state root, portable relationships, and
machine-local provider observations. Each workspace keeps its own lane and
phase focus. Attaching, detaching, or refreshing project motion in one worktree
must not focus a lane, transfer phase ownership, change a sibling workspace's
active phase, or write the sibling worktree.
## Relationship to Existing RFCs

RFC 0108 continues to own RFC stages, readiness, and explicit human promotion.
An objective names a possible advancement and its evidence; it is never stage
authority.

RFC 00238 remains authoritative that RFC advancement is meaningful project
motion and that the pipeline must be perceivable. This RFC refines its central
organizing claim: project evolution is the cockpit's top-level model, while the
RFC pipeline remains privileged and authoritative for decision motion and
canon. Chores and implementation work can participate in project flow without
pretending to be RFC lifecycle events.

RFC 00230 remains useful precedent for goals as PER-sized units and pull
requests as review artifacts. This RFC supersedes only the claims that goal
completion equals PR merge and that a phase is not finished until its PR
merges. Merge is authoritative provider evidence for a delivery artifact.
`exo goal complete` and `exo phase finish` remain explicit Exo outcome
decisions and may require additional validation or dogfooding evidence.

RFC 10176 remains the implemented authority for the phase, goal, and task
hierarchy in this slice. The new relations are campaign-level metadata around
that hierarchy. The longer-term conceptual hierarchy becomes
`lane -> campaign -> goal -> task`; epochs remain project planning history.

RFC 10181 continues to own shared inbox and steering lifecycle. Consumer
receipts, event acknowledgment, and typed approval are deferred to the later
coordination slice. Provider observations in this RFC are evidence records, not
inbox events or receipts.

RFC 10208 owns the storage-writer compatibility fence. Its separately shipped
baseline must be installed and accepted before this RFC assigns a newer writer
generation to project-motion tables. This RFC relies on that general database
and projection preflight rather than defining feature-specific rollback files
or hydration recovery.

RFC 10202 continues to own durable lane identity, workspace-local focus, and
the currently implemented lane-to-one-phase relation. This RFC interprets that
phase as the lane's current transitional campaign and establishes the eventual
model of sequential campaigns without changing lane storage in the first
slice.

RFC 10204 continues to own browser-safe planning and explicit outcome approval.
The Project Motion band adds no browser mutation capability.

RFC 10205 continues to own project workspaces, historical lane inspection,
selection, and focus separation. This RFC advances its snapshot and inspection
schemas and adds identical project motion to active and historical lane reads.

## Failure and Recovery Contract

Relationship validation failures commit nothing. Ambiguous RFC numbers,
malformed PR selectors, invalid target stages, and missing phases are terminal
precondition failures. Retrying them unchanged is not useful.

Provider unavailability is different. A PR relationship can exist without a
successful observation. Attachment records one failed attempt and returns the
committed relationship with an explicit observation diagnostic. `refresh`
continues across artifacts and can be repeated with a new request ID; an
ambiguous transport retry uses the same request ID and payload.

Snapshot and inspection remain available when `gh` is missing, GitHub is down,
authentication expires, or stored observation JSON originated from an older
provider mapping. They show the portable relationship and the strongest stored
observation the decoder can validate. A malformed machine-local observation is
omitted with a diagnostic rather than making the whole workbench response
unreadable.

## Validation Contract

Implementation must cover:

- preservation of every legacy `phase_rfcs_data` relation without inventing a
  canonical identity, plus proof that current `phase update --rfcs` changes
  only legacy relationships and cannot erase or rewrite canonical objectives;
- deterministic dump/import round trips for canonical RFC ULIDs,
  establishment snapshots, unresolved legacy input, and portable PR
  relationships;
- acceptance of a separately released RFC 10208 baseline before the
  project-motion writer generation lands;
- the compiled fence-aware baseline rejecting a project-motion database before
  writable pragmas, migration, `Database::new`, request handling, or legacy
  phase mutation;
- the same baseline rejecting a project-motion projection from the
  `epochs.sql` preflight before target directory or database creation;
- the current project-motion binary invisibly migrating baseline databases and
  projections, including legacy phase and goal RFC metadata;
- ordinary `TABLE_ORDER` round trips for non-empty and valid empty
  project-motion tables, with the three portable authorities included and only
  machine-local observations excluded;
- disappearance of a canonical RFC ULID from the effective corpus retaining
  the objective, snapshot display, and identity-missing diagnostic; restoration
  of the same ULID reconnecting it; and a same-number different ULID remaining
  disconnected;
- attach requiring a live canonical RFC row; exact-ULID detach succeeding for an
  identity-missing objective in the specified phase; and numeric detach
  retaining unique, missing, and ambiguous canonical-resolution behavior;
- typed objectives surviving `phase update --rfcs`;
- phase deletion being restricted while either typed RFC objectives or PR
  relations remain, with explicit detach required before removal;
- last-relation detach deleting an unreferenced PR artifact and cascading its
  machine-local observation, while a shared artifact and observation remain;
- goal metadata fallback, duplicate suppression, and conflict diagnostics;
- attach, update, detach, natural-key idempotency, same-request replay, and
  request-ID conflict;
- zero, one, and several PR artifacts per campaign, including one artifact
  shared by several campaigns;
- fake GitHub observations for open, closed, merged, pending review, approval,
  requested changes, pending checks, passing checks, failing checks, missing
  authentication, not found, timeout, malformed output, and recovery after a
  failed refresh;
- preservation of last-success data across a later failed attempt;
- durable prepared input containing the exact phase, ordered targets, normalized
  payload and hash, and owner identity before the first provider call;
- same-instance same-ID waiters producing one provider execution, waking on the
  canonical outcome, and never becoming provider executors;
- a current owner preventing takeover and an identity-proven dead owner allowing
  takeover with byte-identical prepared input rather than recomputed membership;
- a crash after provider fetch but before commit leaving no partial attachment,
  observation, revision, or outcome and allowing the bounded read to repeat;
- a crash after the atomic commit returning the canonical same-request outcome
  without another provider call, for both attach and refresh;
- final revalidation failure discarding fetched evidence, committing one stable
  terminal outcome, waking waiters, and changing no project-flow rows or
  revisions;
- a provider call counter proving `status`, snapshot, inspection, steering, and
  pipeline reads perform no provider I/O;
- all four virtual/shadow/revision triples registered in `REACTIVE_TABLES`, with
  persistent rowset seeds, row digest maintenance, transactional invalidation,
  and only the observation authority excluded from portable dumps;
- matching `project_motion` semantics in snapshot v5 and inspection v3;
- Rust and TypeScript decoder fixtures that fail closed on schema mismatch;
- cockpit rendering for no motion, targeted decision motion, never-observed
  delivery, observed delivery, and refresh failure with and without preserved
  success;
- linked-worktree sharing of relationships and observations without focus,
  ownership, or sibling-worktree mutation.

The implementation is ready for review only after the relevant storage, Exo
library, daemon integration, linked-worktree, cockpit component, decoder,
formatting, build, and diff checks pass.

## Drawbacks

The transitional model exposes campaign language while internal storage still
uses phase identities. New project-flow commands consistently say campaign and
resolve that selector internally; existing `exo phase` surfaces remain. A
later lane-to-campaign storage migration must preserve the identities
introduced here.

Explicit relationships add ceremony. The alternative is repeated inference,
which is cheaper at write time and more expensive every time a human or agent
must reconstruct project motion.

Stored provider observations can become old. Showing observation age and later
failure makes that uncertainty visible, but it asks the user to understand the
difference between an attached artifact and recently observed provider state.

The first band improves lane comprehension without yet delivering the larger
project trajectory, RFC, and PR views. It is intentionally a proof of the
shared graph rather than the final information architecture.

## Alternatives

### Infer pull requests from Git

Exo could inspect the current branch and GitHub remotes. This fails for
detached worktrees, stacked pull requests, several campaigns on one branch, one
PR serving several campaigns, and historical inspection. It also turns a
heuristic into hidden authority.

### Query GitHub during every snapshot

This would make the band look current without a refresh command, but network
latency and authentication would enter the daemon's bounded read path. A local
provider incident could then make the cockpit unavailable. Explicit refresh
keeps evidence acquisition separate from perception.

### Put provider state in portable dumps

Review and check state changes independently of project policy and differs by
machine access. Committing it would create noisy, misleading project history.
Portable identity and relationships are durable; observations are local.

### Keep goal RFC fields as the only relationship

Goal fields cannot represent campaign intent independently of one execution
unit, cannot distinguish implementation from validation, and bind ambiguous
RFC numbers. They remain compatibility input while the campaign objective
becomes canonical.

### Build dedicated RFC and PR views first

Separate screens could improve inventory navigation while leaving their mutual
meaning implicit. The lane-level band is the smaller proof that one typed graph
can tell a coherent project story before more projections depend on it.

## Deferred Work

This RFC's first implementation does not include:

- durable lane-to-sequential-campaign storage and first-class campaign
  lifecycle commands beyond the transitional `project-flow --campaign`
  resolver;
- dedicated project-home, RFC, or pull-request views;
- background, periodic, or provider-pushed refresh;
- non-GitHub provider adapters;
- browser attach, detach, refresh, merge, promotion, or completion actions;
- commits, worktrees, releases, or deployments as first-class delivery
  artifacts;
- consumer-specific coordination receipts, approval cards, or inbox history;
- automatic RFC promotion, Exo completion, merge, release, or lane lifecycle;
- a wall-clock freshness threshold.

These are subsequent projections or lifecycle slices over the authority
boundary established here. They must not be smuggled into the first proof as
derived behavior.

## Stage 3 and Organic Proof

Stage 3 requires reconciliation with implemented reality, not merely passing
tests. RFC 10207 is already a Stage 2 draft, while project-motion implementation
remains held until RFC 10208's separately released compatibility baseline is
installed and accepted. After that prerequisite, the implementation campaign
supplies the evidence for Candidate readiness and a later, separately approved
Stage 2 to Stage 3 promotion.

Before promotion to Candidate:

1. RFC 10208's fence-aware release is installed and accepted as a separate
   compatibility baseline before project-motion storage advances the writer
   generation;
2. the canonical relationship, provider observation, command, workbench, and
   cockpit contracts in this RFC are implemented and merged;
3. old data and dumps migrate without semantic loss;
4. the installed shared binary serves snapshot v5 and inspection v3 through the
   durable workbench origin;
5. RFC 10207 is explicitly attached to the real implementation campaign as
   `drives`, with `observed_stage=2` and `target_stage=3`;
6. the implementation pull request is explicitly attached with its delivery
   role and an observed GitHub result;
7. ordinary cockpit use lets the user identify the decision being advanced,
   the current and targeted RFC stages, the PR delivering it, and the current
   review or check blocker without reconstructing those facts from chat or
   GitHub;
8. implementation divergences are incorporated into this RFC before any Stage
   3 promotion request.

That ordinary-use observation is the Organic proof for the first slice. A green
test suite and a ready pull request establish implementation confidence; they
do not establish that the cockpit has become a shared perception surface.

Attaching RFC 10207 and its implementation PR to the real campaign is a
separate effect boundary after disposable validation. The completed Stage 2
promotion authorizes this draft, not implementation effects. Stage 3 promotion,
Exo task and goal completion, campaign completion, merge, install, and rollout
each retain their existing explicit approval gates.
