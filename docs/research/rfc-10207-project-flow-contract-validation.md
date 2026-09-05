# RFC 10207 Project-Flow Contract Validation

This document validates the Stage 0 conceptual model in
[RFC 10207](../rfcs/stage-2/10207-cockpit-project-flow-lanes-campaigns-rfcs-pull-requests-and-coordination.md)
against the eight representative project histories named by that RFC. It is a
design-contract test, not an implementation specification.

## Verdict

The model passes the representative-history test.

One normalized relationship model can explain all eight histories without
giving RFCs, pull requests, coordination events, or cockpit views independent
completion authority. The same model preserves RFC stage authority, explicit
Exo outcome review, durable terminal dispositions, and consumer-specific
receipt state.

The validation establishes enough conceptual coherence to consider RFC 10207
for Stage 1. It does not establish implementation readiness or Organic proof.
Storage, commands, provider APIs, migration mechanics, and ordinary cockpit use
remain later evidence.

## Model Under Test

The histories use the following conceptual records and relationships:

| Record | Meaning | Canonical authority |
| --- | --- | --- |
| Lane | Durable stream of project intent | Exo project state |
| Campaign | Ordered, bounded project delta within a lane | Exo project state; implemented by today's phase record during transition |
| Goal and task | Planned execution within one campaign | Exo project state |
| RFC objective | Typed decision relationship from a campaign to an RFC, including observed and optionally targeted stage | Exo project state plus the RFC corpus for RFC identity and stage |
| Delivery record | Campaign-scoped relationship to a workspace, commit, pull request, release, or other delivery artifact | Exo project state for the relationship; the named provider for fresh external observations |
| Coherence evidence | Validation, documentation, release, and dogfooding evidence about a campaign outcome | Exo project state and retained evidence provenance |
| Coordination event | Shared steering or judgment scoped to the project or a project entity | Exo inbox/project state |
| Consumer receipt | One consumer's incorporation of one event revision | Consumer-specific Exo coordination state |
| Typed decision | Human judgment such as approval associated with an event or lifecycle transition | Explicit Exo decision boundary |

The essential relationships are:

```text
project
  -> lane (durable stream)
    -> campaign (ordered bounded delta)
      -> goal -> task
      -> RFC objective -> RFC
      -> delivery record -> delivery artifact
      -> coherence evidence
      -> coordination event

coordination event revision
  -> consumer receipt
```

Dedicated project-home, RFC, pull-request, lane, and coordination views are
projections over this graph. They do not own parallel copies of its
relationships.

## Authority Invariants

Every history must preserve these invariants:

1. RFC stage changes occur only through the RFC lifecycle's explicit human
   approval boundary.
2. Goal and campaign completion occur only through explicit Exo outcome review.
3. A merge, check, review, or release observation is evidence. It never silently
   performs either lifecycle transition.
4. A campaign owns the canonical RFC objective. Goal-level RFC references derive
   from or specialize that objective rather than forming a second attachment
   system.
5. Delivery relationships are explicit. Titles, branch names, worktree paths,
   and temporal proximity are not relationship authority.
6. Shared event lifecycle, typed human decisions, and consumer receipts remain
   distinct.
7. Terminal RFC and campaign outcomes preserve provenance without remaining
   active attention.
8. A lane can continue after any one campaign or RFC objective reaches a
   terminal outcome.

## Current Implementation Boundary

The current system does not yet implement the normalized graph:

- `phases`, `goals`, and `tasks` are the canonical execution hierarchy;
- `goals.rfc` and `goals.target_stage` coexist with the separate `phase_rfcs`
  relationship table;
- each workbench lane references exactly one execution phase;
- no canonical pull-request relationship or provider observation model exists;
- inbox status is global and includes `acknowledged`; and
- no consumer receipt store exists.

These observations define migration inputs. They do not invalidate the proposed
contract, provided migration is explicit and does not infer missing semantics.

## Evidence Anchors

This validation was checked against the repository and canonical Exo state on
2026-08-19. The examples intentionally combine histories that the current model
records across several surfaces; the proposed graph explains those histories but
is not claimed to exist yet.

- [RFC 10203](../rfcs/stage-3/10203-local-lane-workbench-host-and-agent-launch.md)
  records the implemented local workbench host and launch foundation.
- [RFC 10206](../rfcs/stage-2/10206-durable-workbench-entry-and-browser-pairing.md)
  records the durable pairing, locald publication, restoration, and replay
  contract used by the multi-campaign and delivery examples.
- [RFC 0108](../rfcs/stage-4/0108-refined-staged-rfc-process.md) is
  canonically Stage 4 and supersedes RFC 0106.
- [RFC 0106](../rfcs/stage-4/0106-staged-rfc-process.md) is canonically
  superseded by RFC 0108.
- [RFC 0114](../rfcs/withdrawn/0114-advanced-phase-transition.md) is
  canonically withdrawn because the file-backed phase context and archived
  phase-snapshot model it depended on were retired.
- Canonical Exo outcomes for the current cockpit campaign record the separate
  design, implementation, rollout, and ordinary-use acceptance evidence that
  the proposed campaign model would relate directly.

## Scenario Matrix

| # | Representative history | Decisive invariant | Result | Stage 2 pressure |
| ---: | --- | --- | --- | --- |
| 1 | One lane, several sequential campaigns | Lane continuity is independent from campaign closure | Pass | Campaign ordering and lane lifecycle storage |
| 2 | One campaign targets RFC advancement | Objective readiness and RFC promotion remain separate | Pass | Objective schema and goal specialization |
| 3 | RFC reaches Stage 4 canon | Terminal decision leaves active motion but retains provenance | Pass | Canon projection and provenance queries |
| 4 | RFC is withdrawn or superseded | Terminal disposition remains legible without active attention | Pass | Disposition and successor relationship projection |
| 5 | Delivery has zero, one, or several artifacts | Delivery cardinality does not create lifecycle authority | Pass | Provider, freshness, identity, and cardinality contracts |
| 6 | One consumer receipts an event | Receipt changes delivery only for that consumer and revision | Pass | Consumer identity, revisions, storage, and replay |
| 7 | Project, RFC, and PR views share relationships | Views are projections, not independent attachment systems | Pass | Snapshot and query contracts |
| 8 | Completed campaign records a coherent checkpoint | Completion can preserve known divergence and successor motion | Pass | Structured outcome and coherence-evidence schema |

## History 1: One Lane, Several Campaigns

### Representative history

Use the durable-workbench stream as the representative case. Its intent remains
stable while bounded work moves through several deltas:

1. define browser-safe workbench entry;
2. add durable browser pairing;
3. publish the workbench through locald and restore publication after daemon
   replacement;
4. harden launch replay and replacement authority; and
5. validate ordinary return-to-workbench behavior.

Several of these deltas produced separate RFC, implementation, review, and
acceptance outcomes. They nevertheless belong to one durable intent: make the
workbench a place a person can leave open, return to, and trust.

### Expected relationships

- One lane contains an ordered sequence of campaigns.
- Each campaign has its own goals, tasks, owner, completion outcome, RFC
  objectives, delivery records, and coherence evidence.
- Completing one campaign leaves the lane available for the next campaign.
- At most one campaign executes in the lane in the initial model; a future
  campaign may be prepared.

### Expected cockpit projection

The lane view leads with the active campaign and collapses prior campaigns as
history. The project home shows one durable stream rather than several unrelated
completed lanes. A prior campaign remains inspectable without becoming current
attention.

### Migration consequence

Today's one-lane/one-phase relation cannot represent this history directly. The
initial migration should preserve each existing phase identity as a campaign
identity and turn the current lane's execution-phase reference into an explicit
lane/campaign relationship. Joining separate historical phases into one durable
lane requires an explicit reviewed relationship; chronology or title similarity
is insufficient.

### Result

Pass. Lane identity and campaign closure answer different questions without
competing for lifecycle authority.

## History 2: A Campaign Targets RFC Advancement

### Representative history

The current RFC 10207 work provides the representative case. A campaign begins
while RFC 10207 is at Stage 0. Its drafting and validation goals aim to make a
Stage 1 proposal decision possible.

### Expected relationships

- The campaign owns an RFC objective with an observed stage of `0`, a targeted
  stage of `1`, and a typed relationship such as `drives`.
- Goals and tasks produce design, compatibility, and validation evidence toward
  that objective.
- Completing the drafting or validation goal can establish readiness evidence.
- Campaign completion records its approved outcome.
- RFC promotion remains a distinct, explicitly approved lifecycle action.

### Expected cockpit projection

The lane and campaign views show the objective and its readiness evidence. The
RFC view shows the current stage, targeted advancement, supporting campaign,
open questions, and the separate promotion gate. Completing a task cannot make
the RFC appear promoted.

### Migration consequence

Current `goals.rfc`/`target_stage` values and `phase_rfcs` rows are inputs to one
campaign-owned objective model. When both surfaces identify the same RFC, the
migration must reconcile their target and relationship semantics explicitly.
A missing or generic `related` relationship cannot be upgraded to `drives`,
`implements`, or `validates` by inference.

### Result

Pass. One objective can coordinate readiness evidence while preserving RFC
promotion authority.

## History 3: An RFC Reaches Stage 4 Canon

### Representative history

RFC 0108, the refined staged RFC process, is a representative stable decision.
Its durable value is not that an old campaign remains active. Its value is that
the project can still explain the staged lifecycle, the authority of human
promotion decisions, and the historical relationship to RFC 0106.

### Expected relationships

- Each campaign objective that targeted an earlier advancement becomes realized
  when that advancement is explicitly approved.
- The Stage 4 RFC remains project canon after its final objective is realized.
- Supporting campaign outcomes and delivery/coherence evidence remain
  provenance when available.
- Absence of historical campaign data is represented as absent provenance; the
  system does not invent it.
- Later changes create successor objectives or RFCs rather than silently
  reopening the stable decision.

### Expected cockpit projection

The RFC view presents an established decision or capability, its rationale,
current-law status, successor relationships, and known provenance. It does not
show permanent unfinished work merely because the RFC remains linked to project
history. The project home can count the stable decision as part of the current
baseline.

### Result

Pass. Active RFC motion ends without erasing the decision or its history.

## History 4: Withdrawal or Supersession

### Representative history

RFC 0114 is a representative withdrawal: its advanced phase-transition model
depended on the file-backed phase context and archived phase snapshots that the
project retired. RFC 0106 is a representative supersession: RFC 0108 is the
refined stable authority.

### Expected relationships

- Withdrawal records the terminal disposition, reason, approving decision, and
  supporting campaign/delivery evidence when available.
- Supersession additionally records the successor RFC.
- Objectives targeting further advancement stop appearing as active motion.
- Campaign outcomes that performed the withdrawal or supersession remain
  provenance.
- The durable record remains discoverable and cannot be mistaken for current
  law.

### Expected cockpit projection

The RFC view distinguishes withdrawn and superseded history from active
proposals and canon. The project home may show the disposition as a recent
project delta, then let it recede into history. No unresolved attention is
implied solely by retaining the record.

### Result

Pass. Terminal dispositions are durable outcomes rather than deletion or
permanent active work.

## History 5: Zero, One, or Several Delivery Artifacts

### Representative history

Three variants exercise the delivery relationship:

- **No pull request:** a research campaign produces a reviewed design inventory
  or validation artifact and closes with no code delivery.
- **One pull request:** one implementation or RFC promotion is reviewed and
  merged through a single PR.
- **Several artifacts:** durable workbench entry is delivered through separate
  pairing, locald publication, restoration, and replay-hardening PRs and release
  artifacts.

### Expected relationships

- A campaign may have zero or more delivery records.
- Every delivery record identifies its artifact and role explicitly.
- Provider observations such as checks, reviews, mergeability, merge, or revert
  state carry source identity, observation time, and freshness.
- A merged artifact can satisfy required delivery evidence but cannot complete
  the campaign or promote an RFC.
- If one artifact serves several campaigns, each campaign retains an explicit
  relationship to the shared artifact identity; ownership is not inferred from
  branch or timing.

### Expected cockpit projection

The campaign view explains how its delta is being delivered. The pull-request
view groups campaign and RFC-objective relationships around the artifact. A
merged PR can truthfully display `Outcome review required`, and a completed
campaign can truthfully retain an unmerged optional artifact.

### Result

Pass. Delivery cardinality is independent from lifecycle authority.

## History 6: One Consumer Receipts an Open Event

### Representative history

A dogfooding concern is open at revision 3. Agent A incorporates revision 3 and
records a receipt. Agent B has not seen it. The concern remains unresolved.
Later, the user adds material information, producing revision 4.

### Expected relationships

- The event's shared lifecycle remains open after Agent A's receipt.
- Agent A's revision-3 receipt suppresses repeated delivery of that revision to
  Agent A only.
- Agent B continues to see revision 3 as new.
- Revision 4 is new to both consumers unless a later receipt covers it.
- Resolving, superseding, or archiving the event requires a shared lifecycle
  action.
- Approving a requested outcome is a typed human decision associated with the
  event, not a receipt and not an event lifecycle state.

### Expected cockpit projection

The Coordination inspector can show shared event state and the current
consumer's receipt state simultaneously. It never claims that an event is
resolved because the current browser or agent has incorporated it.

### Result

Pass. Consumer delivery bookkeeping composes cleanly with shared project
judgment.

## History 7: Project, RFC, and Pull-Request Projections

### Representative history

A campaign in the durable-workbench lane has an objective to advance RFC 10206,
delivery records for a worktree and pull request, validation evidence, and an
open dogfooding concern.

### Shared graph

```text
durable-workbench lane
  -> pairing campaign
    -> RFC objective (implements RFC 10206, observed Stage 1, target Stage 2)
    -> delivery record (worktree)
    -> delivery record (pull request)
    -> coherence evidence (tests and browser acceptance)
    -> coordination event (open dogfooding concern)
```

### Expected projections

- **Project home:** shows the durable-workbench delta, its decision/delivery/
  coherence state, and the open concern.
- **Lane view:** shows the active campaign, plan, objective, delivery, evidence,
  and prior campaign history.
- **RFC view:** shows RFC 10206's stage motion, campaign support, delivery
  evidence, and unresolved concern.
- **Pull-request view:** shows the artifact's checks/reviews/merge state and the
  campaign and RFC objective it serves.
- **Coordination view:** shows the concern's shared state and consumer-specific
  receipts.

Each projection follows explicit relationship identities. Updating a provider
observation changes every relevant projection without creating a second PR-RFC
or PR-campaign attachment.

### Result

Pass. The views answer different questions about one graph.

## History 8: A Coherent Completed Checkpoint

### Representative history

A campaign completes after its RFC objective is approved, implementation is
merged, required checks pass, and dogfooding establishes the intended core
experience. One limitation remains: a browser on another platform still needs
validation. The limitation is recorded with a successor objective or campaign.

### Expected relationships

- The campaign completion outcome records the intended delta, established
  evidence, explicit human approval, and remaining divergence.
- Specification, realization, coherence, and learning motion remain separately
  inspectable.
- The remaining limitation does not make the completed outcome false.
- The limitation is not hidden; it scopes the claim and creates explicit
  successor motion when warranted.
- The lane remains available after the campaign closes.

### Expected cockpit projection

The project home shows the campaign as a recent baseline rather than active
work. Historical inspection explains what became reliable, why that conclusion
was credible, and what remained. The successor campaign, when started, appears
as new motion in the same lane rather than rewriting the old outcome.

### Result

Pass. A campaign can close truthfully without claiming perfection or leaving
its durable lane permanently active.

## Cross-Scenario Migration Direction

The histories support one migration direction for Stage 1 consideration:

1. Preserve existing phase identities and outcomes as the transitional campaign
   records. Do not mechanically rename user-facing history without retaining
   provenance.
2. Replace the lane's single execution-phase ownership with ordered explicit
   lane/campaign relationships.
3. Reconcile `phase_rfcs` and goal RFC targets into campaign-owned RFC
   objectives. Preserve observed values and require review where relationship
   meaning conflicts or is absent.
4. Add explicit delivery relationships rather than deriving them from Git or
   GitHub naming conventions.
5. Separate existing global inbox lifecycle and completion approval from future
   consumer receipts. Existing `acknowledged` rows require semantic migration,
   not a blanket conversion.
6. Derive every cockpit view from the same identities and relationships.

This is a direction, not an implementation-ready migration algorithm. Exact
table changes, backfill rules, compatibility windows, and command behavior
belong in Stage 2.

## Deferred Contract Details

The representative histories do not require Stage 0 to settle:

- whether the public CLI adopts `campaign` immediately or aliases `phase`;
- whether a prepared campaign can overlap an active campaign;
- the physical owner and specialization schema for campaign and goal RFC
  objectives;
- pull-request provider selection, refresh cadence, offline behavior, or
  multi-campaign cardinality;
- consumer identity across browser profiles, Codex tasks, VS Code, and agents;
- event revision and receipt retention rules;
- snapshot/API schemas and compatibility versions; or
- the exact lane fulfilled, parked, or closed lifecycle.

Those questions affect implementation shape. None requires a second conceptual
authority model.

## Pass Criteria

RFC 10207's conceptual contract passes because:

- all eight histories use the same entity and relationship vocabulary;
- RFC stage authority and Exo outcome authority remain explicit and separate;
- delivery and coordination observations inform decisions without performing
  them;
- terminal outcomes retain provenance without remaining active attention;
- all cockpit views derive from one relationship graph; and
- migration from current state can be explicit without relying on labels,
  branches, paths, or chronology as authority.

The contract would fail if implementation required an independent goal-RFC
attachment model, allowed provider observations to complete lifecycle state, or
gave a cockpit projection authority to invent relationships. No representative
history requires any of those compromises.

## Evidence Boundary

This validation establishes conceptual and corpus-level coherence. It does not
prove that the proposed schema can be migrated safely, that external provider
observations remain fresh, that consumer receipts behave correctly under
concurrency, or that the cockpit communicates the model well in ordinary use.

Those claims require Stage 2 specification, implementation tests, linked-
worktree and multi-consumer validation, and dogfooding. Stage 1 can now make the
public case for the model and settle naming and migration direction without
claiming that those later properties are already proven.
