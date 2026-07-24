# RFC Reconstruction Plan

Final outcome: [RFC Reconstruction Final Disposition](./rfc-reconstruction-final-disposition.md).
That disposition records the completed public-tree reconstruction and
supersedes the next-step recommendations in this historical execution plan.

This plan turned the inventory into an evidence-driven reconstruction sequence.
It avoided editing RFC files directly until each action could be expressed
through the Exo RFC command surface (`supersede`, `archive`, `withdraw`, `edit`,
`promote`, `repair`, or `rename`).

## Findings At Plan Time

- The corpus has 334 markdown files under `docs/rfcs`.
- 332 of those files are managed RFC records; `README.md` and
  `0000-template.md` are support files.
- 193 managed RFC records belong to 96 duplicate-title families.
- Duplicate numeric ID collisions have been reduced to one remaining group:
  `0060`.
- Stage 3 is mostly closer to the current Rust/SQLite/sidecar implementation
  than Stage 4.
- Stage 4 mixes canonical current law, duplicate stable records, and historical
  transition plans.
- `0022` (`Unified Project State`) has been archived and superseded by
  `10176`; it is no longer pending reconstruction work.
- The most important remaining stale stable record is `10154` (`Context
  Persistence`), which still describes checking in `docs/agent-context` rather
  than the current SQLite plus repo/sidecar/shadow projection model.
- PR [#187](https://github.com/wycats/exo2/pull/187) retired agent-context
  phase archives: `phase finish` no longer copies `docs/agent-context/current`
  to `docs/agent-context/archive`, the committed archive corpus was removed,
  RFC `0114` and related current/archive-file RFCs were withdrawn, and RFC
  `10180` now names those phase context files as deleted legacy surfaces.
- Recon validation found that some initial classifications needed sharper
  wording: `0129` has now been withdrawn as an unimplemented configurable-runner
  record, `0121` is materially implemented in the VS Code agent runtime layer,
  and `0123` is best treated as a historical transition plan rather than as
  superseded by a Stage 2 record.
- The lane-centered workbench direction should influence what we do about
  drift, but this checkpoint does not implement lanes or rewrite RFCs around
  that design package.
- The clean public-root cutover is complete; RFC reconstruction now proceeds
  against the public tree and should not rely on private-history-only context.

## Reconstruction Rules

1. Preserve RFC history. Supersede, archive, or withdraw instead of deleting.
2. Do not promote a planning RFC to Stage 3/4 unless the behavior exists in code.
3. Use implemented behavior to determine Stage 3/4 readiness.
4. Keep future plans as Stage 0/1/2 work, not as stable claims.
5. Use high-number canonical records when they better describe the current
   system, but keep an explicit supersession trail from older low-number records.
6. Separate implementation state from design trajectory. A current RFC can be
   transitional or away from the target direction, and a stale RFC can still
   contain an aligned future plan worth preserving.

## Evidence-Driven Reconstruction Method

RFC reconstruction starts from evidence: implementation, tests, CLI and MCP
behavior, Exo state, `docs/vision.md`, specs, and accepted design packages.
Implemented behavior determines Stage 3/4 readiness. Durable direction
determines what to do with drift: stabilize, rewrite, supersede, archive,
withdraw, or preserve as future work.

Existing RFC text remains useful evidence when it explains current behavior,
historical decisions, or aligned future direction. It does not become current
law by being checked in; each reconstruction PR must account for the gap between
the text, the implementation, and the durable direction.

Every RFC rewrite follows this stage-aware loop:

1. Observe the local RFC process, current stage, related RFCs, implementation,
   tests, docs, Exo state, and durable design evidence.
2. Ground the intended design role for the RFC within the workspace-centered
   vision and current phase goals.
3. Draft public design prose that serves the RFC's current stage.
4. Evaluate stage readiness against the evidence.
5. Record remaining gaps and next authoring or implementation work.

Each cluster PR should state the current behavior, intended direction, canonical
RFC role, stage decision, lifecycle decisions, and unresolved design questions.

## Directional Design Lens

The [lane-centered workbench design package](../design/lane-centered-workbench/README.md)
provides a useful target direction for judging drift without expanding this
checkpoint into lane implementation work. Its core framing is that a lane is an
observable execution stream, not an alias for branch, worktree, pull request,
task list, phase, or chat thread.

Use that lens after this checkpoint lands:

| Drift Shape | Interpretation | Cleanup Bias |
| --- | --- | --- |
| Current code and aligned with lane/workbench direction. | Good candidate for canonical current law. | Stabilize, consolidate, and promote carefully. |
| Current code but transitional relative to lanes. | Implementation history is real, but wording should not harden as future law. | Preserve, then rewrite around the intended target. |
| Current code but away from lane/workbench direction. | The RFC may describe a live system that should be replaced. | Mark as implemented history and plan a replacement RFC. |
| Stale code but aligned with lane/workbench direction. | Old prose may contain a useful future slice. | Extract a smaller Stage 0/1/2 plan. |
| Stale code and away from lane/workbench direction. | Low-value reconstruction target. | Withdraw, archive, or supersede. |

## First Managed RFC Operations

These are the safest mechanical cleanup operations because they are duplicate
or supersession relationships, not design decisions.

| Action | Source | Target | Rationale |
| --- | ---: | ---: | --- |
| Supersede | 0002 | 10153 | `0002` is only a stub; 10153 is the substantive modern record, though it still needs drift cleanup. |
| Supersede | 0004 | 10155 | Both are `Modes of Collaboration`; 10155 is the better canonical modern record. |
| Supersede | 0020 | 10159 | Both are `Rich Text DOM`; 10159 should be the canonical modern record. |
| Supersede | 0024 | 10162 | Both are `Exosuit UI Architecture`; 10162 should be canonical. |
| Supersede | 0106 | 0108 | `0108` refines the staged RFC process and should be the live process record. |
| Archive or historical-note | 0123 | none yet | `0123` is a historical transition plan; do not use Stage 2 `10134` as a canonical supersession target without a separate review. |

The phase-archive/current-file family has also been processed. PR #187 withdrew
`0114`, `0107`, `0138`, `0139`, `0148`, `10028`, `10104`, `10105`, `10108`,
and `10128`, removed the checked-in `docs/agent-context/archive` corpus, and
updated code/tests so phase finish has no archive side effect.

## Current-Law Rewrite Candidates

These should be rewritten around the codebase before being treated as stable
reference material.

| RFC | Current Issue | Evidence Direction |
| ---: | --- | --- |
| 10154 | Context persistence is no longer simply checked-in `docs/agent-context`. | Rewrite around repo/sidecar/shadow state policies, SQLite as operational source of truth, and portable SQL projections. |
| 0121 | Core VS Code `AgentRuntime` extraction is implemented, but should not be used as law for MCP proxy or daemon lifecycle. | Keep the shared-agent-runtime record focused on `AgentRuntime`; split MCP proxy and daemon lifecycle surfaces clearly in separate RFCs. |
| 10165 | Storage write mediation has landed, but the RFC wording still contradicts itself on shadow-table enforcement. | Correct status language so ordinary Exo write mediation is complete and `xShadowName` enforcement remains the named hardening gap unless code evidence changes. |

## Demotion Or Withdrawal Status

These Stage 3 records should not be promoted until a new implementation exists
or the RFC is rewritten around current behavior.

| RFC | Status | Why |
| ---: | --- | --- |
| 0021 | withdrawn | Described `exo rfc triage`, but the current `rfc` command namespace has no gardener/triage operation. Withdrawn in the RFC 0021 triage-tooling PR. |
| 0129 | withdrawn | Configurable TDD runners were never implemented in the current Exo command architecture; `0129` now records that lifecycle correction and future runner work should start as a fresh Stage 0/1 RFC. |

## Stage 3 Stabilization Candidates

These appear close to current implemented reality and may become the stable
backbone after review. Items with drift should be rewritten or trimmed before
promotion.

| RFC | Status | Why |
| ---: | --- | --- |
| 10176 | clean | Matches the current SQLite-backed project state model. |
| 10165 | cleanup-first | Ordinary reactive write mediation is current, but the RFC's shadow-boundary status wording needs correction before stabilization. |
| 0137 | clean-with-caveat | Matches generated exohook GitHub Actions projection; sync-check language still needs care. |
| 10179 | cleanup-first | Core re-exec support is current, but extension binary-dir behavior still needs wording cleanup. |
| 0132 | cleanup-first | Command spec/router/parser direction is active, but the full tiny DSL grammar is only partially current. |
| 0136 | cleanup-first | MCP/tool metadata architecture exists, but VS Code still has extension-native tools plus universal `exo-run`. |

## Stage 0/1 Cleanup Pass

After the Stage 3/4 surface is coherent, use the duplicate-family table in
`docs/research/rfc-reconstruction-inventory.md` to process Stage 0/1. Resolve
numeric ID collisions before using Exo-managed `rfc` operations for affected
IDs, because ambiguous IDs can make the command surface select no safe target.

1. Choose a canonical survivor for each duplicate family before withdrawing
   anything; when one member is active and the other is withdrawn, preserve the
   active member unless review explicitly decides otherwise.
2. Consolidate duplicate active Stage 0/1 pairs into a single surviving plan.
3. Repair or rename duplicate numeric IDs so future `supersede`, `archive`, and
   `withdraw` operations can address exactly one RFC.
4. Keep future work only when it still maps to a realistic phase in the current
   architecture.
5. Convert broad, stale "vision" RFCs into smaller future-plan RFCs only when a
   near-term implementation slice is plausible.

## Historical Execution Queue

The action sequence at this checkpoint was:

1. Account for the RFC `10200` identity repair reminder before the next
   ID-addressed RFC mutation batch.
2. Rewrite RFC `10154` as the current Stage 4 persistence policy.
3. Correct RFC `10165` storage status wording around ordinary write mediation
   and the remaining shadow-table enforcement gap.
4. Make stable duplicate supersession readable in markdown/read surfaces for
   `0002 -> 10153`, `0004 -> 10155`, `0020 -> 10159`, `0024 -> 10162`, and
   `0106 -> 0108`.
5. Resolve the remaining numeric collision `0060`.

The completed sequence and its final validation are recorded in the
[RFC Reconstruction Final Disposition](./rfc-reconstruction-final-disposition.md).
Future lifecycle or rewrite work begins through the ordinary staged RFC process
rather than this historical queue.

## Session Note

The first five low-risk `rfc supersede` operations were applied through Exo and
updated the sidecar SQL projection (`superseded_by` / `supersedes`). The markdown
files did not change. Current `rfc show` / `rfc list` output does not clearly
surface those supersession links, so future cleanup should verify through the
SQL projection or improve the read surface before relying on the relationship
being obvious to agents.

PR #187 is the reference example for a thorough withdrawal pass: it withdrew
the archive/current-file RFCs, updated the surviving storage-disposition RFC,
removed the obsolete committed corpus, and changed implementation/tests to
match the recorded disposition.
