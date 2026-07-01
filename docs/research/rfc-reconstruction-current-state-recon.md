# RFC Reconstruction Current-State Recon

This checkpoint refreshes RFC reconstruction after the clean public-root
cutover. It establishes the current goal, corpus evidence, completed work, and
next action queue before the next RFC rewrite PR.

Scanned baseline: public `origin/main`
`61161ba0d91fae95918997c49595865d4195ce7e`.

## Reconstruction Charter

The RFC reconstruction goal is to make `docs/rfcs/**` usable as durable project
reference again. Stage 3 and Stage 4 RFCs should describe implemented behavior
or explicitly record implemented history. Earlier-stage RFCs should preserve
future plans only when they still map to current architecture and a plausible
implementation path.

The active stabilization phase uses the evidence-driven reconstruction method
recorded in [RFC Reconstruction Plan](./rfc-reconstruction-plan.md). Each RFC
cluster starts from implementation, tests, CLI/MCP behavior, Exo state,
`docs/vision.md`, specs, and accepted design packages. Implemented behavior
determines Stage 3/4 readiness; durable direction determines how drift should
be stabilized, rewritten, superseded, archived, withdrawn, or preserved as
future work.

Success means:

- Stage 3/4 records are current, archived, withdrawn, or clearly superseded;
- stale stable records no longer steer agents toward retired architecture;
- duplicate numeric IDs no longer make RFC commands ambiguous;
- duplicate title families have one reviewed survivor or an explicit
  historical/supersession relation;
- current architecture is described by durable RFCs, specs, design notes, and
  `docs/vision.md` without depending on private-history context; and
- the next implementation phase can rely on the RFC corpus without first
  reverse-engineering which records are current.

Scope boundaries:

- This checkpoint does not edit `docs/rfcs/**`.
- Public-readiness remediation is now a separate stream. It can prune public
  documentation, but RFC lifecycle work remains governed by the reconstruction
  queue.
- The lane-centered design package is current durable design direction, while
  implementation adoption still needs explicit RFC or implementation work.
- Exo task/goal state and sidecar projections remain operational state; this
  note records their relevant outcomes for RFC planning.

Reconstruction is complete when the Stage 3/4 surface has no known stale law,
all numeric collisions are resolved or intentionally documented, and remaining
Stage 0/1/2 drift is converted into reviewed future-work or historical records.

## Corpus Inventory

The public tree currently contains 334 Markdown files under `docs/rfcs/**`.
Two of those are support files (`README.md` and `0000-template.md`), leaving
332 managed RFC records.

| Status Directory | Managed Records |
| --- | ---: |
| `stage-0` | 90 |
| `stage-1` | 71 |
| `stage-2` | 12 |
| `stage-3` | 16 |
| `stage-4` | 25 |
| `archive` | 3 |
| `withdrawn` | 115 |
| **Total managed records** | **332** |

The current managed corpus still has broad title duplication, but numeric ID
ambiguity is now narrow.

| Inventory Surface | Current Finding | Reconstruction Meaning |
| --- | --- | --- |
| Duplicate numeric IDs | One remaining group: `0060` has active prompt-patterns and withdrawn dirty-tree steering records. | The clear-survivor numeric collision work landed; do not run ID-only lifecycle operations for `0060` until the prompt-patterns survivor is reviewed. |
| Duplicate title families | 96 duplicate-title families affecting 193 managed RFC records. | Most remaining duplicate work is title/family consolidation rather than command-surface ambiguity. |
| Stage 3/4 records | 42 managed records: 17 Stage 3 and 25 Stage 4. | This remains the first reconstruction surface because these records claim implemented or stable status. |
| Archived records | `0022`, `0116`, and `0124` are archived. | `0022` is now completed project-state replacement history, not pending work. |
| Withdrawn records | 114 records, including `0021` and the `0114` phase archive/current-file family. | Withdrawal work has removed several stale command/file-surface claims from the active corpus. |

## Stage 3/4 Current-State Refresh

The earlier Stage 3/4 classification remains useful background, but several
rows have changed since it was written.

| RFC | Current State | Evidence | Reconstruction Action |
| ---: | --- | --- | --- |
| `0021` | Withdrawn. | `exo rfc show 0021` reports withdrawn with the `exo rfc triage` rationale. | Complete. Preserve the review-report idea in research history; no active Stage 3 rewrite. |
| `0022` | Archived and superseded by `10176`. | `exo rfc show 0022` reports archived and `Superseded by: RFC 10176`. | Complete. Keep as implemented history; use `10176` as the project-state anchor. |
| `0114` family | Withdrawn with the phase archive/current-file cleanup. | PR `#187` outcome and withdrawn RFC files. | Complete. Do not revive `docs/agent-context/current` or archive-file persistence language. |
| `0121` | Implemented core remains current, with adjacent runtime drift. | VS Code `AgentRuntime` exists; MCP proxy/daemon lifecycle are separate strata. | Keep, then trim if later stabilization work needs sharper boundaries. |
| `0129` | Withdrawn. | No registered `exo tdd` namespace or runner dispatch exists in the current command registry. | Complete. Preserve adjacent TDD steering and metadata as implementation evidence; start future configurable-runner work as a fresh Stage 0/1 RFC. |
| `10154` | Stable text is stale. | The RFC still says to check in `docs/agent-context`; current policy uses SQLite plus repo/sidecar/shadow projection rules. | Next rewrite target. Rewrite in place as current Stage 4 persistence policy. |
| `10165` | Storage contract mostly implemented; text still contains an internal shadow-boundary contradiction. | `ReactiveVTab` implements `UpdateVTab`; `SqliteWriter` writes through reactive tables; the RFC says both that defensive shadow boundary is complete and that `xShadowName` enforcement remains incomplete. | Follow-up correction after `10154`, or pair with storage-status cleanup if the contradiction blocks readers. |
| `10176` | Current project-state anchor. | SQLite migrations and Exo context commands use the epoch/phase/goal/task model; `0022` now points here. | Keep as the current reference. Later stabilization can promote or rewrite after adjacent stable records are cleaned. |
| `10184` | Current project/workspace/sidecar identity model, still Stage 1. | RFC now documents `sidecar bootstrap`, lower-level sidecar binding commands, sidecar repo operations, and `project move-root`. | Keep as the current identity model. Later decide whether to promote after sidecar cutover usage has settled. |
| `10200` | Stage 1 file exists but Exo reports identity repair needed. | `exo rfc show 10200` returns not found with a `metadata_relink` repair reminder. | Preflight before the next RFC mutation batch: run reviewed `exo rfc repair 10200` in a focused PR or include it with the next RFC command-surface repair. |

## Completed Work Ledger

| Area | Completed Outcome | Current Effect |
| --- | --- | --- |
| Corpus inventory and Stage 3/4 classification | PR `#177` landed the first inventory, Stage 3/4 classification, trajectory overlay, and plan. | Historical baseline; this recon supersedes it as the current status source. |
| Numeric ID collisions | Numeric collision review and clear-survivor cleanup landed in PRs `#180` and `#181`. | Duplicate numeric IDs are reduced to `0060`. |
| RFC `0021` | Withdrawn as stale `exo rfc triage` law. | No active Stage 3 triage-tooling record remains. |
| RFC `0022` | Archived and superseded by `10176`. | Project-state cleanup for the old TypeScript `ContextService` model is complete. |
| `0114` archive/current-file family | PR `#187` withdrew stale phase archive/current-file records and removed the old committed archive corpus. | Phase finish no longer carries the `docs/agent-context/current` archive model. |
| RFC `10165` storage implementation | Writable reactive vtabs, revision schema/backfill, `SqliteWriter` routing, and storage trace invalidation landed. | Storage write mediation is implemented for ordinary Exo state writes; RFC wording still needs a precise shadow-boundary correction. |
| RFC `10184` sidecar identity alignment | `project move-root` and sidecar command-surface behavior were added to the RFC. | Public cutover can rely on the current project/workspace/sidecar identity model. |
| Lane-centered design | The lane-centered design package landed and `docs/vision.md` became the current workspace-centered overview. | Lane direction is durable design context, not a replacement for current RFC lifecycle cleanup. |
| Public cutover | The new public repository was minted from signed clean root `61161ba0d91fae95918997c49595865d4195ce7e`; private history remains in `wycats/exo2-private-history`. | Reconstruction now runs in the public tree and should avoid relying on private-history-only artifacts. |

## Current Reconstruction Queue

This is the first execution queue under the evidence-driven reconstruction
method. The queue starts with RFC `10200` so Exo can reliably address that RFC
by ID, then moves to full RFC rewrites and lifecycle cleanup. The separate
`0060` numeric collision remains an ID-addressing exception until row 5 is
processed.

| Order | Work | Reason | Expected PR Shape |
| ---: | --- | --- | --- |
| 1 | Repair or explicitly account for RFC `10200` identity metadata. | Exo currently warns that `10200` needs `metadata_relink` repair, and `exo rfc show 10200` cannot read it by ID. | Small Exo-managed repair PR, unless the next RFC command-surface PR includes it directly. |
| 2 | Rewrite RFC `10154` as the current Stage 4 persistence policy. | It is the highest-value stale stable record left: its old `docs/agent-context` rule conflicts with SQLite, repo/sidecar/shadow policy, and public docs cleanup. | Focused RFC rewrite using `10176`, `10178`, `10180`, `10184`, and `10165` as anchors. |
| 3 | Correct RFC `10165` storage status wording. | The storage implementation landed, but the RFC simultaneously says the shadow boundary is complete and that `xShadowName` enforcement remains incomplete. | Narrow RFC edit that marks ordinary write mediation complete and shadow-table enforcement as the remaining hardening item unless code evidence changes. |
| 4 | Re-check Stage 3/4 canonical stable duplicates. | `0002 -> 10153`, `0004 -> 10155`, `0020 -> 10159`, `0024 -> 10162`, and `0106 -> 0108` were recorded in Exo state, but the active markdown still contains duplicate stable records. | Reviewed lifecycle PRs that make the markdown/read surfaces reflect the selected canonical records. |
| 5 | Resolve remaining numeric ID collision `0060`. | It is the only remaining duplicate numeric ID and still makes ID-only RFC operations unsafe for that number. | Review whether `stage-1/0060-prompt-patterns-promptspec-resourcespec-and-cross-spec-interpolation.md` remains the `0060` survivor, then repair `withdrawn/0060-phase-aware-dirty-working-tree-steering.md` as a duplicate/historical record behind canonical dirty-tree RFC `0117`. |
| 6 | Process high-value Stage 3 drift. | `0129` is withdrawn; `0132`, `0136`, `10170`, and `10179` still need trimming or stabilization decisions. | Continue with `10179` stabilization cleanup, then pair `0132`/`0136`, then trim/stabilize `10170`. |
| 7 | Consolidate Stage 0/1 duplicate-title families. | 96 duplicate-title families remain, mostly future-work/history cleanup. | Batch by topic family after the Stage 3/4 surface is coherent. |
| 8 | Decide lane adoption RFC shape. | The lane-centered package is current design direction but not yet an RFC adoption record. | Small adoption RFC or manual/current-architecture note after initial lane implementation planning starts. |

## Boundary With Public-Readiness Work

Public-readiness work has completed the clean-root cutover and current-tree
blocker scrub. The remaining public docs prune can proceed independently. It
does not change the reconstruction queue unless it deletes or moves a document
referenced by the RFC corpus.

This recon is the current status source for RFC reconstruction. Earlier
research notes remain evidence, but their next-step recommendations should be
read through this queue.
