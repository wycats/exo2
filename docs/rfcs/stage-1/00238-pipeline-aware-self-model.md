<!-- exo:238 ulid:01kmzxey1h0f2f7er2y2119zj5 -->

# RFC 238: Pipeline-Aware Self-Model: Making the System Perceive and Steer Itself


# RFC 00238: Pipeline-Aware Self-Model: Making the System Perceive and Steer Itself

## Summary

The system should perceive itself well enough to steer itself. Today, the user carries a mental model — what's important, what blocks what, what kind of work something is, where RFCs are in their pipeline — that the system doesn't encode. This RFC proposes making the RFC pipeline the central organizing principle of the system, expressing pipeline state through VS Code affordances that both human and AI perceive, differentiating work types so that non-RFC work has proportional ceremony, and surfacing enough context that the system replaces manual "Yehuda steering" with encoded self-knowledge.

## Motivation

### The Problem: Manual Steering

When the user sits down to work, they carry knowledge the system doesn't encode:

- "This phase is really about getting RFC 00215 to Stage 3"
- "These three tasks are the remaining work before that RFC can promote"
- "That RFC is a project plan — it did its job, it doesn't need to be canonical"
- "This is a chore, not RFC work — it should feel different"
- "I can see 3 inbox items and one is blocking the current RFC"
- "We've been piling up RFCs without process, and I can't feel momentum through them"

The agent lacks all of this. It sees tasks and goals but not pipeline state. It sees RFCs but not which ones are in-flight vs. reference vs. archive candidates. It can't tell the user "RFC 00236 is blocked on RFC 00188" or "promoting RFC 00224 to Stage 3 requires updating the manual." The user must provide this steering manually, session after session.

### The Friction Feedback Loop

This lack of encoding creates a compounding problem:

1. User notices friction while working
2. It doesn't fit neatly into the current phase/RFC structure
3. Capturing it properly means interrupting flow to navigate the RFC pipeline
4. So it lives in the user's head, or on a whiteboard, or in a quick inbox item
5. Periodically the user tries to triage — but triage itself is heavyweight because the agent doesn't share the user's perception of what matters
6. Things get stale, context is lost, the pile grows

The system optimizes for the happy path (RFC → phase → implement → verify → promote). But friction points, chores, and tech debt are off the happy path — and the system makes them _harder_ to address, not easier.

### The Perception Gap

The user sees VS Code: sidebar panels, status bar badges, problems pane, test explorer results. The agent sees: tool outputs, file contents, search results. These are _different perceptions of the same workspace_, and the gap between them causes confusion.

When the user sees a red squiggle and the agent doesn't know about it — that's a perception gap. When the user can see 80 RFCs in the file tree but the agent has no sense of which ones matter — that's a perception gap. When exohook produces terminal output that the user can parse but the agent sees as noise — that's a perception gap.

VS Code is already full of affordances designed to provide information to humans, and these affordances are exposed via both UIs and APIs. Investing in them pays double: the human sees it in the UI, the agent perceives it through the API. Every shared perception channel reduces the need for manual steering.

### The Pipeline That Exists But Isn't Visible

The RFC pipeline already exists structurally. RFCs have stages (0-4). Phases link to RFCs. Promotion has rules. But the _experience_ doesn't convey it:

- The sidebar shows "phases and tasks," not "RFCs moving through a pipeline"
- Steering talks about tasks and goals, not "this RFC needs X to advance"
- 80+ RFCs are all equally visible regardless of whether they're active, canonical, or historical
- There's no sense of throughput — "we promoted 3 RFCs this epoch" isn't surfaced
- Dependencies between RFCs aren't tracked or visible

The user wants to feel like they're "chugging through" a pipeline — sit down, see what's in motion, see what's next, make progress, feel the rate. Instead they see a pile.

## Core Thesis

**The system is an RFC pipeline.** Epochs and phases are views into that pipeline — they organize work on moving RFCs forward. The system should encode this deeply enough that:

1. An agent opening the workspace can orient itself without the user explaining the state
2. A human looking at the sidebar feels pipeline momentum, not a pile of tasks
3. Chores and observations don't require RFC-level ceremony
4. RFCs in flight feel like they're moving through a pipeline the user is "chugging through"
5. Dependencies between RFCs are visible, so both human and agent know what unlocks what
6. Reviews and wrap-up have a place in the lifecycle instead of dangling

## Detailed Design

### Work Types

The system currently treats everything as "RFC → phase → implement." But at least three distinct workflows exist with different appropriate ceremony:

| Kind             | Examples                                                | Appropriate Ceremony                                     | Current Friction                                                              |
| ---------------- | ------------------------------------------------------- | -------------------------------------------------------- | ----------------------------------------------------------------------------- |
| **RFC work**     | New feature design, architecture change                 | Full pipeline (Stage 0→4), phases, goals, TDD            | Works, but pipeline momentum isn't visible                                    |
| **Chores**       | Merge a PR, rename a prefix, fix a bug, tech debt       | Minimal — do it, commit, move on                         | Forced through RFC pipeline; agent loses context; feels like "messing around" |
| **Observations** | "This steering is confusing," "inbox needs a tree view" | Capture → triage → maybe becomes RFC, maybe just a chore | Lives in user's head or whiteboard; capture friction is high                  |

**Chore lane**: Ad hoc phases that can be inserted (like surgical strikes) without disrupting the metadata and structure of the plan. No RFC required. Proportional ceremony. Ergonomic improvements to surgical strikes would help here too.

> **Design caution**: The chore lane described here is a first pass. As the RFC pipeline becomes more central to the system's self-model, chores will feel _even less_ aligned with the pipeline flow — making the mismatch more acute, not less. Expect this design to need refinement once pipeline-aware steering is operational and we can see where chores actually create friction in the new model. Don't lock in prematurely.

**Review phases**: A structured place for wrap-up work — merging PRs, cleaning up after a phase, assessing whether an RFC is ready for promotion. Currently this work dangles because there's no workflow for it. Goals and phases close, and the review process has no home.

**Observation capture**: Lower-friction than current inbox. When the user notices something while working, the capture cost should be near zero, and the triage cost should be near zero for chores.

### RFC Lifecycle Refinement

#### Stage 4 as Living Canon

Stage 4 ("Stable") should be the home of the project's canonical state — the living documentation of how the system works right now. These are documents that get edited, maintained, and kept coherent. The manual (`docs/manual/`) is the rendered view of this canon.

Today there is no first-class concept of "project canon" and no processes for consolidating, updating, or maintaining coherence across canonical documents. Stage 4 should be that concept.

#### Ephemeral RFC Retirement

Not every RFC should reach Stage 4. Some RFCs are project plans (like RFC 00235 "Whiteboard Priorities") — they organize a body of work, and when that work is done, they've served their purpose. These shouldn't rot at Stage 4 pretending to be canonical.

Options for retirement:

- **Archive folder**: `docs/rfcs/archive/` for RFCs that did their job
- **PR association**: Ephemeral planning RFCs get associated with the PR(s) they guided. A tool could retrieve "RFCs for this PR." When the PR merges, the RFC retires naturally.
- **Withdrawn with reason**: Current "withdrawn" stage, but with a "completed" reason vs. "abandoned"

The PR association approach is attractive because it turns the lifecycle problem into a structural feature — PRs become pipeline artifacts, and the RFC lifecycle aligns with the code lifecycle.

### Pipeline-Aware Steering

Steering should understand the RFC pipeline:

**Today**: "A pending goal exists. Start TDD before implementing."

**Proposed**: "Phase is advancing RFC 00236 (Stage 0). The steel thread spike goal is unstarted. Promoting to Stage 1 requires user approval — consider starting implementation to build evidence for promotion."

Concretely:

- Steering knows which RFCs a phase is advancing
- Steering knows what stage each RFC is at
- Steering knows what's required for promotion (implementation, tests, manual updates)
- Steering differentiates chore work from RFC work in its recommendations
- Steering surfaces dependencies: "RFC X is blocked on RFC Y reaching Stage 2"

### Shared Perception Channels

VS Code affordances that serve as shared perception channels for human and agent:

| VS Code Affordance                   | Human Perceives                            | Agent Perceives                 | Pipeline Role                                   |
| ------------------------------------ | ------------------------------------------ | ------------------------------- | ----------------------------------------------- |
| **Problems pane**                    | Red squiggles, warning count in status bar | `DiagnosticCollection` API      | "What's wrong / blocking RFC promotion?"        |
| **Test Explorer**                    | Pass/fail tree with structured results     | `TestController` API            | "Has this RFC's implementation been verified?"  |
| **Tree views**                       | Sidebar panels with expandable trees       | `TreeDataProvider` API          | "What's the pipeline state? What's in flight?"  |
| **Status bar**                       | Badges, text, icons                        | Status bar item API             | "What's active right now?"                      |
| **Resource projections** (RFC 00236) | `#` picker items in command palette        | `search_workspace_symbols` tool | "What entities exist and what's their content?" |
| **chatContextProvider** (proposed)   | `#` picker items in chat                   | Auto-injected context           | "What context matters for this conversation?"   |

Each investment in a shared perception channel reduces the perception gap and replaces manual steering with encoded self-knowledge.

#### Diagnostics as Steering

The Problems pane is a steering surface. "Problems pane integration" isn't a separate feature — it's steering expressed through VS Code's native attention system. Validation that currently requires running a tool (`exo verify`) could surface proactively as diagnostics:

- "RFC 00236 is at Stage 2 but the manual hasn't been updated" → Warning
- "Phase has 0 completed goals after 3 sessions" → Info
- "Inbox item is blocking RFC 00224 advancement" → Warning
- "Task references a deleted goal" → Error

#### Exohook as Test Explorer

The base exohook → Test Explorer integration exists: `ExohookTestController` discovers checks via `exohook discover --format=jsonl` and runs them via `exohook validate <lane> --format=jsonl`, presenting results as a pass/fail tree. However, the integration is not yet a true shared perception channel — results are visible to the human in the Test Explorer UI but invisible to the agent through steering.

Closing this perception gap requires:

1. **JSONL alignment**: Rust events lack `lane` on check-level events; `CheckOutput` is defined but never emitted (output is buffered); no `enqueued` state
2. **Test Explorer fidelity**: Streaming output via `appendOutput()`, `enqueued` lifecycle state, restage error surfacing
3. **Continuous validation**: `supportsContinuousRun` profile with filter-aware file watching (re-run only checks whose `filters` match the saved file)
4. **Loop wiring**: Publish validation results to world state, apply `validation_penalty` to steering confidence (same pattern as `error_penalty`)

Once complete, the Test Explorer becomes a fully shared perception channel:

- Human sees: live pass/fail tree with streaming output, continuous re-validation on save
- Agent sees: `validation_status` in world state, confidence penalties when checks fail
- Pipeline role: "Has this RFC's implementation passed validation?"
- Gate role: validation failures reduce `ship` confidence, preventing phase finish with broken checks

### Pipeline Visualization

#### Bounded WIP: In-Flight RFCs

The sidebar should make clear which RFCs are actively in-flight (in the current epoch/phase), vs. the full 80+ RFC inventory. A tree view (or augmented epoch/phase view) showing:

- RFCs linked to the current phase, with their current stage
- What's needed to advance each RFC
- Progress indicators (goals completed, tasks remaining)

This is the "chugging through" feeling: here are the 3-5 RFCs in motion, here's where each one is, here's what's next.

#### Zoom Out: Full Pipeline View

Sometimes the user needs to zoom out and see the entire remaining project plan — not just the current phase but the full pipeline of RFCs across all stages, with a sense of overall momentum. This is tricky to get right:

- Too much detail → overwhelming, defeats the purpose
- Too little → doesn't convey momentum or help with planning
- The right level → shows the "shape" of the pipeline (how many at each stage, what's moving, what's stuck)

This is an open design problem. The zoom-out view might be:

- A summary in the epoch context view ("Stage 0: 12, Stage 1: 5, Stage 2: 3, Stage 3: 1, Stage 4: 8")
- A separate command/webview for periodic review
- A steering output that occasionally says "pipeline health: 5 RFCs stuck at Stage 1"

#### Dependencies

RFC dependencies should be tracked and visible. "RFC 00236 depends on RFC 00188 (computed roots)" isn't encoded anywhere today — the user carries it. Making it explicit enables:

- Steering: "RFC 00236 is ready but blocked on RFC 00188 reaching Stage 2"
- Visualization: dependency graph or blocked indicators in the pipeline view
- Planning: "these 4 RFCs form a chain; sequencing them correctly matters"

## Relationship to Existing RFCs

| RFC                                   | Relationship                                                                                                                                                          |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **00225** (Problems Pane Integration) | Detailed spec for Phase 3's shared perception channel. Diagnostics as the first concrete implementation of this RFC's perception thesis.                              |
| **00236** (Resource Projections)      | Technical approach for one shared perception channel (resource discoverability). Serves this RFC's "shared perception" design.                                        |
| **00235** (Whiteboard Priorities)     | The whiteboard analysis that surfaced the friction points this RFC addresses. Historical context.                                                                     |
| **00224** (SOAR Loop)                 | SOAR's Status/Orient/Act/Review maps to pipeline-aware steering. The Review gap identified in 00224's audit is the same "dangling review" problem this RFC addresses. |
| **00231** (Quick/Chore Phases)        | Early attempt at the "chore lane" concept. This RFC subsumes it with a clearer framing.                                                                               |
| **00188** (Derived/Computed Roots)    | Foundation for reactive state that powers resource projections and pipeline-aware views.                                                                              |

## Alternatives Considered

### Keep the task-centric model

Treat the system as a task tracker that happens to reference RFCs. Rejected: this is the status quo, and it's the source of "Yehuda steering" — the system sees tasks and goals but not what they're advancing in the pipeline. The task model is correct at the execution layer but wrong as the organizing principle.

### Build a separate pipeline dashboard

Create a standalone webview or external tool for pipeline visualization, separate from the existing VS Code affordances. Rejected: this creates another perception channel to maintain instead of investing in shared ones. The insight is that VS Code already has affordances (Problems pane, Test Explorer, tree views) designed for exactly this — using them pays double (human UI + agent API).

### Organize by priority instead of pipeline stage

Use a priority-based system (P0/P1/P2) instead of pipeline stages. Rejected: priority is a property of an RFC, not a workflow. Pipeline stages encode _where work is in a process_, which is what steering needs to reason about. Priority can coexist with stages but can't replace them.

## Drawbacks

- **Increased steering complexity**: Pipeline-aware steering requires the steering engine to understand RFC stages, promotion requirements, and dependencies — significantly more complex than the current task-centric model.
- **RFC pipeline as organizing principle may not fit all projects**: Exosuit currently assumes a project's work is organized around RFCs. Projects with different governance models would need adaptation.
- **Chore lane risks becoming a dumping ground**: If the ceremony differential between RFC work and chores is too large, users may route everything through the chore lane to avoid the pipeline. The design caution in the chore lane section acknowledges this.
- **207 existing RFCs create audit burden**: The RFC inventory has grown without pipeline discipline. Classifying them is necessary but time-consuming.

## Implementation: Steel Thread Epoch

Each phase is a steel thread — a narrow end-to-end proof of one piece. Not horizontal infrastructure layers, but vertical slices that deliver something narrow but complete.

### Phase 1: Vision Capture + RFC Audit

- [ ] This RFC: finalize and promote to Stage 1
- [ ] Audit 80+ existing RFCs: classify as active-pipeline / canon / archive-candidate
- [ ] Define work types (RFC work, chores, observations) and their ceremony levels
- [ ] Define RFC lifecycle refinements (ephemeral retirement, Stage 4 as canon)
- [ ] Identify RFC dependencies for in-flight RFCs

### Phase 2: Steel Thread — Pipeline-Aware Steering

- [ ] Steering knows "this phase is advancing RFC XXXX"
- [ ] Steering knows RFC stages and what blocks promotion
- [ ] Steering differentiates chore work from RFC work
- [ ] Steering surfaces dependencies between RFCs
- [ ] Validate: steering output for a typical session replaces manual context the user would have provided

**Concrete example 1 — steering output** (observed Feb 2026): When Phase 1 of this epoch started, steering said _"A pending goal exists. Start TDD before implementing."_ It should have said something like: _"Phase is advancing RFC 00238 (Stage 0). The first goal is finalizing the RFC for Stage 1 promotion. Consider reviewing the RFC design for completeness."_ Steering has no awareness of the RFC pipeline — it sees tasks and goals but not what they're advancing or why. This steel thread should make that example work correctly.

**Concrete example 2 — goal metadata** (observed Feb 2026): The goal `rfc-238-promote` has label text "Finalize RFC 00238 and promote to Stage 1" — but no structured metadata linking it to RFC 00238 or expressing that it's a _promotion_ goal targeting Stage 1. The phase links to RFC 00238 via `rfcs = ["00238"]`, but goals don't inherit or specialize that link. Steering can't say "this goal will advance RFC 00238 from Stage 0 → 1" because the relationship is prose, not data. Goals should be able to express their pipeline intent (e.g., `rfc = "00238"`, `target_stage = 1`) so that steering can reason about what completing the goal means for the pipeline.

### Phase 3: Steel Thread — Shared Perception Channel

The diagnostics integration (RFC 00225) is the first shared perception channel, chosen because it has the clearest path to proving the thesis: "same information reaches human AND agent."

- [ ] DiagnosticsService watches `onDidChangeDiagnostics`, fires reactive invalidation
- [ ] `derived:diagnostics.summary` computed root produces `{ errorCount, warningCount, bySource, blocking, topErrors }`
- [ ] Status bar shows error/warning count (human perceives)
- [ ] `exo-status` enriched with diagnostic summary (agent perceives via LM tool)
- [ ] `exo-diagnostics` LM tool for detailed queries
- [ ] Validate: agent can perceive workspace errors it previously couldn't, and human sees the same data in status bar

**Validation criteria**: Run a session where the agent encounters a TypeScript error. Before this phase, the agent wouldn't know about it until `exo verify` or manual notification. After this phase, `exo-status` shows the error count, and the agent can query `exo-diagnostics` for details. The human sees the same error count in the status bar. Shared perception proven.

**Architectural note**: This phase extends ReactivityService with programmatic invalidation (`invalidateRoots`), establishing the pattern for event-driven derived roots that don't back to files. This pattern enables future shared perception channels (test explorer results, exohook output, etc.) to follow the same architecture.

### Phase 4: Steel Thread — Chore Lane + Review Flow

- [ ] Ad hoc phases that don't require RFC creation or plan restructuring
- [ ] Review phases for wrap-up (PR merge, cleanup, promotion assessment)
- [ ] Surgical strike ergonomic improvements
- [ ] Validate: a chore can be captured and completed without disrupting the current plan

### Future Phases (Post-Epoch Assessment)

- Pipeline visualization (bounded WIP view, zoom-out view)
- Inbox tree view
- RFC dependency tracking and visualization
- Resource projections production deployment (RFC 00236 Phases 1-6)
- chatContextProvider integration
- Project canon process (Stage 4 consolidation)

## Open Questions

1. **Pipeline zoom-out**: What's the right level of detail for seeing the entire remaining project plan? Summary stats, timeline, dependency graph, or something else? **Design maturity note**: As of Feb 2026, the user reports being "only just getting to the point where we can speak coherently about this in terms of a UI." This question needs more lived experience with pipeline-aware steering before it's ready for detailed design. Don't rush to specify — let the earlier steel threads (steering, perception, chore lane) inform what the zoom-out view needs to show.
2. **Ephemeral RFC retirement**: Archive folder vs. PR association vs. withdrawn-with-reason? The PR association is attractive but needs design work.
3. **Steel threads as first-class concept**: After this epoch, should steel threads have a formal place in the workflow? (They're a natural phase shape: narrow, end-to-end, validates one thesis.)
4. **Chore ceremony**: What's the minimum viable chore lane? An ad hoc phase with no RFC link? A special task type? Something outside the phase system?
5. **Project canon**: What does the process for maintaining Stage 4 coherence look like? Who triggers consolidation? How do you know when canon has drifted from reality?
6. **Dependency representation**: How should RFC dependencies be encoded? Frontmatter field (`depends_on: [00188]`)? Separate tracking? Inferred from phase structure?
7. **Review phase shape**: Is review a distinct phase type, a phase suffix, or a checklist within phase completion?

## Prior Art

- **Kanban boards**: Visualize WIP, limit work-in-progress, feel throughput. The "chugging through" metaphor is kanban's core value prop.
- **GitHub Projects**: Board views of issues through status columns. Similar pipeline visualization but at issue granularity, not RFC granularity.
- **Shape Up** (Basecamp): Six-week cycles with "bets" (shaped work) and "cooldown" (chores, cleanup). The chore lane concept echoes cooldown periods.
- **OODA/SOAR**: Decision loops that assume the observer can perceive the environment. This RFC is about _making the environment perceivable_.

## Future Possibilities

- **Pipeline velocity metrics**: Track RFC throughput over time — promotions per epoch, average time at each stage, stall detection. Could surface as a periodic steering insight ("pipeline velocity is declining — 3 RFCs stuck at Stage 1 for 2+ epochs").
- **Cross-workspace pipeline views**: If multiple exosuit-managed projects exist, a unified view of RFC pipelines across workspaces.
- **AI-driven pipeline optimization**: Steering could suggest pipeline reordering based on dependency analysis and observed friction patterns.
- **RFC dependency visualization**: A graph view showing which RFCs block which, helping with epoch planning and sequencing decisions.
- **Pipeline templates**: Predefined pipeline shapes for common work patterns (feature RFC, refactor RFC, governance RFC) with appropriate ceremony defaults.

## Appendix: Whiteboard Assessment Baseline (Feb 2026)

During the Whiteboard Spike phase (RFC 00235), a detailed codebase survey was conducted against a physical whiteboard of friction points. This appendix preserves that baseline so future agents can assess progress without re-surveying.

### Item Status

| Whiteboard Item                  | Codebase State      | Notes                                                                                                                                                                   |
| -------------------------------- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ~~reactivity changes~~           | ✅ Done             | 5 derived roots, DocumentService reactive wrappers, full invalidation pipeline                                                                                          |
| ~~StatusBar w/ reactivity~~      | ✅ Done             | Status bar uses reactive computed roots                                                                                                                                 |
| ~~SOAR~~                         | ✅ Done             | RFC 00224 written, tool categorization complete                                                                                                                         |
| epoch UI                         | ✅ Done             | Separate "Epoch Context" tree view in sidebar                                                                                                                           |
| Full Command Spec integration    | ✅ Done             | Extension loads command-spec.json, generates tools via factory                                                                                                          |
| implement files scope on exohook | ✅ Done             | Exohook computes filesets by scope (staged, uncommitted, etc.)                                                                                                          |
| studio diff (short-term fix)     | ✅ Done             | Semantic diff utilities for TOML and RFCs                                                                                                                               |
| extension test working reliably  | ✅ Done             | Vitest for unit tests, @vscode/test-electron for E2E                                                                                                                    |
| tdd review                       | ✅ Done             | CLI has tdd subcommands, extension registers TDD LM tools                                                                                                               |
| "RFC pipeline" mental model      | ◐ Partial           | RFC lifecycle exists structurally, but no guide explaining the pipeline to newcomers. More importantly: the system doesn't surface pipeline momentum (see Core Thesis). |
| Copying instructions/agents      | ◐ Partial           | Files exist but assume exo CLI installed; not fully self-contained for fresh clone                                                                                      |
| Bootstrap                        | ◐ Partial           | Script creates config/templates but doesn't install dependencies or build                                                                                               |
| inbox visualization              | ◐ Partial           | Status bar badge + quick pick only. No tree view, no sidebar presence                                                                                                   |
| more reactivity                  | ◐ Partial           | 5 derived roots operational; question is what else should be reactive                                                                                                   |
| exo history & other worktrees    | ◐ Partial           | Separate `exohistory` CLI exists but no `exo history` command                                                                                                           |
| fix < full, should stage         | ◐ Partial           | Exohook supports workflows and staged scope; unclear if specific bug is fixed                                                                                           |
| "tech debt level" / chores       | ◐ Conceptual        | "Chore" appears as walkthrough category; no formalized work type distinction                                                                                            |
| **exohook → test explorer**      | ✅ Done             | ExohookTestController surfaces checks in Test Explorer with JSONL streaming, continuous run, and validation penalty in steering                                         |
| **problems pane integration**    | 🔴 Not started      | Zero `DiagnosticCollection` usage in extension source                                                                                                                   |
| RFC rendering / other rendering  | ✅ Done             | Custom editor with webview, Svelte "Studio" app, RfcView component                                                                                                      |
| remove "step" more aggressively  | ❓ Needs assessment | Are impl step references still in the codebase?                                                                                                                         |
| remove/invalidate goal/task      | ❓ Needs assessment | Do CLI commands exist for this?                                                                                                                                         |
| re-enter phase                   | ❓ Needs assessment | Can you reopen a finished phase?                                                                                                                                        |
| "quick" phase in transition      | ◐ RFC 00231 exists  | Early attempt at chore lane concept                                                                                                                                     |
| loom demo                        | N/A                 | Not a codebase item                                                                                                                                                     |

### Mapping to This RFC's Design Concepts

| Whiteboard Item                         | RFC 00238 Concept                                             | Notes                                                                                                       |
| --------------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| exohook → test explorer                 | Shared Perception Channels / Test Explorer                    | Terminal noise → structured pass/fail results that both human and agent perceive                            |
| inbox visualization                     | Pipeline Visualization / tree views                           | Status bar badge → browsable sidebar view                                                                   |
| problems pane integration (SOAR?)       | Diagnostics as Steering                                       | Validation errors → Problems pane diagnostics; steering expressed through VS Code's native attention system |
| RFC rendering / other rendering         | Shared Perception Channels / Resource Projections (RFC 00236) | Plan entities discoverable through VS Code affordances                                                      |
| "RFC pipeline" mental model             | Core Thesis: RFC pipeline as central organizing principle     | Not just docs — the system should convey pipeline momentum through its UI and steering                      |
| Copying instructions/agents + Bootstrap | Self-explaining system (prerequisite for pipeline awareness)  | Agent opening workspace should orient without manual steering                                               |
| "tech debt level" / chores              | Work Types / Chore Lane                                       | Proportional ceremony for non-RFC work                                                                      |
| more reactivity                         | Foundation layer (RFC 00188)                                  | Computed roots power resource projections and pipeline-aware views                                          |
| re-enter phase / "quick" phase          | Chore Lane + Review Flow                                      | Ad hoc phases, review phases, surgical strike improvements                                                  |
