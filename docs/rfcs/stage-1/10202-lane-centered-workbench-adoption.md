<!-- exo:10202 ulid:01kxe1fzcamzgrk6hnqcbb6byd -->

# RFC 10202: Lane-Centered Workbench Adoption

**Feature**: lane-centered-workbench

## Summary

Exo adopts the lane-centered workbench as its product model for concurrent agentic work. A workbench lane is the durable identity of one stream of change: the intent that started it, the workspace activity that advances it, the goals and observations that describe it, the review artifacts that assess it, and the outcome that closes it.

A lane connects these artifacts without collapsing their meanings. A branch remains a version-control handle, a worktree remains a checkout, a pull request remains a review artifact, an RFC remains a design record, and a chat session remains situated conversational context. The lane is the project object that lets Exo explain how those pieces belong to the same work.

This RFC adopts that direction and defines its first proof. The [lane-centered workbench design package](../../design/lane-centered-workbench/README.md) remains the detailed authority for product behavior, system shape, interaction design, and implementation planning. A later Stage 2 revision of this RFC will turn the adopted direction into an implementation-ready contract.

## Motivation

Agentic project work rarely advances as one linear conversation. A request may produce research, an implementation branch, a review cycle, a repair, an RFC decision, and follow-up learning. Each part already has an appropriate artifact, but the project has no durable object representing the stream of work that joins them.

Users and agents therefore reconstruct active work indirectly. A branch name may suggest an implementation topic, a worktree may reveal where the files are checked out, a pull request may expose the latest review state, and a chat may remember why the work began. None of those surfaces can answer the whole question. They cannot reliably say which intent the work serves, which workspace currently has focus, which evidence supports its status, or what knowledge should survive when the work closes.

This becomes more costly as work proceeds concurrently. One workspace may be implementing a feature while another investigates a defect and a third reviews an RFC. Project-wide state must remain shared, but each workspace also needs an honest account of what it can see and what it is doing. Treating one checkout, one phase, or one conversation as the global point of view makes those distinctions disappear.

A workbench lane supplies the missing identity. It gives Exo a stable subject for questions such as: What work is active? Which stream is focused here? What needs attention? Why is it considered blocked or ready? Which observations are current? What should be carried back into the project when the work is complete?

## Proposed Model

### The lane is the active stream of work

A workbench lane represents a coherent stream of change within a project. It begins from intent and remains identifiable as the work moves through preparation, execution, review, repair, and closure. Goals, tasks, branches, worktrees, pull requests, RFCs, validations, signals, and sessions may all attach to a lane, but none of them defines the lane by itself.

This distinction lets each artifact keep its existing job. Git continues to describe source history and checkout topology. GitHub continues to describe public review. RFCs continue to describe durable design decisions. Phases, goals, and tasks continue to organize current Exo work. The lane adds the connective identity that makes those artifacts legible as one execution stream.

The repository already uses the word "lane" for Exohook validation groupings. Those remain validation lanes. Product prose may use "lane" where the workbench context is clear; implementation and cross-system documentation use "workbench lane" or "work lane" when the distinction matters. The final command namespace remains a Stage 2 decision.

### Lane state preserves three points of view

Lane-centered work depends on the separation established by Exo's project and workspace model.

**Project state** carries durable facts shared by every workspace in the project: lane identity, intent, lifecycle, linked goals, accepted outcomes, and relationships to durable artifacts.

**Workspace view** describes the reality of one checkout: its branch, visible documents, dirty state, local focus, and other facts that may differ across linked worktrees. Focusing a lane is therefore workspace-local. One worktree can concentrate on an implementation lane while another reviews a different lane without either overwriting the other's attention.

**Observations** are provenance-bearing claims about project or workspace reality. A validation result, pull-request status, review comment, or Git state is meaningful because Exo can say where it was observed, when it was observed, and which content it described. Observations support a lane's status; they do not silently become timeless project truth.

The separation matters most when the surfaces disagree. A pull request can be green while a workspace contains uncommitted changes. A branch can exist while its lane is parked. A validation result can be correct for one commit and stale for the files now under review. Lane status must preserve those distinctions rather than reducing them to a single ambient answer.

### Exo owns the semantic contract

Canonical lane state belongs to the resolved Exo project. The CLI, MCP transport, editor integration, and future browser workbench are adapters over the same operations and state. A surface may present the work differently, but it should not invent a parallel lane identity or derive canonical truth from chat history, a branch name, raw GitHub state, or a commit SHA alone.

This keeps the product model compatible with the system that exists today. Phases remain useful planning bands and current ownership structures. Goals and tasks remain the units through which work is prepared, executed, and reviewed. Shared perception remains the path for scoped signals. The dormant cockpit package remains a possible host for a lane-aware interface. Adoption of lanes gives these systems a common subject without requiring their immediate replacement.

## The First Proof

The first implementation slice should prove one sentence:

> An agent can create, focus, and resume a lane from canonical project state without relying on chat history.

Creating the lane establishes a stable project identity for the stream and enough intent to recognize it later. Focusing it associates the current workspace with that lane while leaving other workspaces free to focus their own work. Resuming it demonstrates that a later command or session can recover the same lane, its intent, and its current place in the workflow from Exo itself.

The proof is intentionally narrow. It should preserve current phase, goal, task, inbox, and validation-lane behavior while establishing the new identity and focus relation. It does not need to deliver the complete workbench UI, automatic worktree creation, pull-request repair, cross-lane conflict detection, or the final lane-close experience. Those capabilities can grow from a durable core once the project can create, focus, and resume a lane reliably.

Success is behavioral rather than cosmetic. Renaming a phase, decorating a branch, or retaining a chat thread would leave the underlying reconstruction problem intact. The proof succeeds when lane continuity survives the loss of conversational context and remains coherent across linked workspaces.

## Design Authority And RFC Context

The design package carries the richer material that benefits from coordinated artifacts. Its [product and system proposal](../../design/lane-centered-workbench/proposal.md) develops the complete model; the [mockups](../../design/lane-centered-workbench/mockups.html) establish visual hierarchy and interaction taste; the [implementation brief](../../design/lane-centered-workbench/implementation-brief.md) describes an incremental first slice; and the [source map](../../design/lane-centered-workbench/source-map.md) relates the direction to the current codebase. This RFC records the adoption decision so that the corpus has a staged, reviewable account of the direction. It does not flatten the design package into a second copy.

The proposal builds on the project-state model in [RFC 10176](../stage-3/10176-project-state-model.md) and the project, workspace, and worktree distinctions in [RFC 10184](../stage-1/10184-project-workspace-worktree-unbundling-the-conflated-root.md). [RFC 10181](../stage-2/10181-shared-perception-inbox-as-a-steering-channel.md) supplies the scoped perception channel through which lane-relevant signals can arrive. [RFC 10195](../stage-1/10195-daemon-lifecycle-authority-and-shared-perception-surfaces.md) supplies the reliable runtime and shared-perception substrate needed by lane-aware adapters, while [RFC 10197](../stage-0/10197-cockpit-project-catalog-and-commandspec-api-boundary.md) describes the project catalog and transport-neutral cockpit direction.

Two records require later alignment rather than lifecycle changes in this proposal. [RFC 10155](../stage-4/10155-modes-of-collaboration.md) remains stable history for the implemented collaboration model and should eventually explain how those modes appear inside lanes. [RFC 10192](../stage-0/10192-epoch-owned-sidecar-collaboration.md) remains future concurrency design and should be revisited once lane ownership and workspace focus have an implementation-ready shape. Every related RFC retains its current stage and role.

## Compatibility And Transition

Lane adoption is additive. Existing phase, goal, and task commands continue to describe current behavior while lane support develops. Workspace-specific phase focus and ownership remain authoritative for the implemented system. Branches, worktrees, pull requests, RFCs, and sessions remain first-class artifacts, and Exohook validation lanes retain their present semantics.

The transition should begin by adding durable lane identity and workspace-local focus at the current project-state boundary. Existing work does not need to be mass-migrated before the first proof is useful. Early implementations can attach lanes deliberately where continuity matters, learn from those cases, and define broader migration only when the model is mature enough to preserve meaning.

This posture also protects the RFC's stage honesty. The codebase contains the project-state, workspace-view, daemon, shared-perception, and command infrastructure that makes lane adoption credible. It does not yet contain a workbench-lane entity, persistence schema, focus relation, or public command surface. Stage 1 can adopt the direction; Stage 2 must settle the buildable contract before implementation begins.

## Stage 2 Readiness

An implementation-ready revision must first define lane identity and persistence. It needs a minimal portable schema, the lifecycle represented by that schema, the relationship between a lane and its attached workspaces, and a migration story that preserves current project state. It must state which facts travel with the project and which facts, including workspace focus and raw local paths, remain machine-local observations.

The revision must then define the operation surface. That includes the command and API namespace, create and focus semantics, adapter behavior, command effects, machine-channel representation, and the relationship between lane-scoped signals and shared perception. It must explain how lane focus coexists with workspace-active phase focus during the transition.

Finally, the revision must make status evidence testable. Validation freshness needs an identity stronger than a commit SHA when dirty source state is present, such as a reviewed content or fileset fingerprint. Compatibility coverage must demonstrate that lane support preserves existing phase, task, inbox, validation-lane, linked-worktree, and daemon behavior. These decisions mark the boundary between adopting the model and being ready to implement it.

## Drawbacks

A lane adds a durable product object to an already rich project model. During the transition, users and implementers will encounter both the lane direction and phases as the implemented ownership unit. The proposal earns that complexity only if lanes reduce reconstruction, coordinate concurrent work, and preserve evidence more effectively than the existing artifacts can on their own.

The terminology also carries a real collision. Exohook already uses "lane" for validation groupings. Consistent use of "workbench lane" and "validation lane" can keep the concepts distinct, but the final CLI and API language must prove that the distinction remains clear in practice.

The design package is larger than a conventional RFC. Keeping it authoritative avoids duplicating visual and implementation material, but it asks readers to consult several coordinated documents. This RFC therefore needs to remain a concise adoption record with reliable links and a clear statement of which document answers which kind of question.

## Rationale And Alternatives

### Use branches or worktrees as the active-work identity

Branches and worktrees are indispensable source-control mechanisms, but they cannot represent intent, evidence, review, or outcome. Work can begin before a branch exists, continue across more than one checkout, and remain meaningful after its branch is deleted. Making Git topology the product identity would reproduce the same reconstruction problem at a different layer.

### Use pull requests as the active-work identity

A pull request has strong review semantics, but it covers only the public-review portion of a stream. Research, design, local implementation, repair, and post-merge learning all extend beyond that window. A lane should link a pull request and learn from its state without inheriting its lifecycle.

### Extend phases to represent concurrent streams

Phases already organize coherent bands of work and remain valuable. Their current project-wide focus and ownership semantics, however, do not express several workspace-local streams progressing at once. A lane can contain or relate to phased work while giving each workspace an independent focus.

### Keep the direction in vision and design documents

The vision and design package explain the direction well, but they do not give it an RFC lifecycle. A small adoption RFC makes the decision visible to the corpus, preserves stage honesty, and creates a place to reconcile the proposal with implementation as it advances.

## Unresolved Questions

The exact lifecycle of a lane remains open for Stage 2. The first proof needs create, focus, and resume behavior; later work must decide how preparation, execution, review, repair, readiness, parking, and closure appear in the canonical model.

The command namespace also remains open because "lane" already has a validation meaning. The chosen surface must feel natural in the workbench while remaining unambiguous in CLI, API, and implementation contexts.

The persistence boundary requires further design. Stable lane identity and accepted outcomes belong to portable project state, while workspace focus and raw checkout observations are local. Stage 2 must define the relationships between those categories and the behavior when workspaces disappear, reappear, or observe conflicting evidence.

The transition from workspace-active phase focus to lane focus deserves an explicit compatibility model. The first implementation should preserve both; the draft must decide whether one derives from the other, whether they remain independent during migration, and what users see when they disagree.

Finally, lane adoption will create pressure on adjacent collaboration and ownership RFCs. That later corpus work should preserve implemented history while rewriting future direction around the lane model where the evidence supports it.

## Future Possibilities

Once Exo can create, focus, and resume a lane, later RFC stages can develop the full workbench experience: lane closure and outcome review, pull-request repair and reactivation, cross-lane conflict detection, richer editor and browser surfaces, automated worktree attachment, and corpus-level guidance for lane-centered collaboration. Each capability should extend the durable identity established by the first proof rather than introduce another competing representation of active work.
