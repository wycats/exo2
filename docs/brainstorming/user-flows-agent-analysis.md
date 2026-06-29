# User Flows: Companion Analysis

> **Read [user-flows.md](user-flows.md) first. This document extends it — it does not replace it.**

## How to Read This Document

**Document hierarchy**: `user-flows.md` is the **intent** document — what the experience should feel like and why. This file is the **implications** document — what that intent demands from the system. When they conflict, the intent document wins.

**The gravitational dominance problem**: This analysis is structured, has tables, and proposes specifics. It will feel more "authoritative" to agents than the looser, more exploratory user-flows.md. **That instinct is wrong.** The emotional design in user-flows.md ("the user should feel a sense of momentum and progress") is the actual design constraint. Specific proposals here are starting points for conversation, not approved designs.

**What user-flows.md does right that this analysis should amplify, not replace:**

1. **Emotional design is the backbone, not decoration.** The document derives features from how things should _feel_, then works backward to what needs to exist. The `[!IMPORTANT]` callouts each identify a specific emotional juncture where getting it wrong would break the flywheel.

2. **The validation flywheel is the right obsession.** Problems pane + exohook + test results → unified signal → steering → agent correction. This circuit is what makes the agent self-correcting rather than user-corrected. It appears repeatedly because it deserves to.

3. **The deliberately unfinished parts should stay unfinished.** Steel thread document shape, "exosuit skills" format, goal validation metadata structure, state-specific prompts.toml semantics — these need implementation experience to resolve, not more design prose.

### The Agent Perception Gap

Agents experience the system through tool calls that return text. Everything that matters to an agent is data — JSON from `exo-status`, diagnostic strings, steering responses. This creates a systematic bias:

- **What agents see as "core"**: things that produce data the agent consumes (validation results, steering, state records)
- **What agents see as "infrastructure"**: things that display data in the UI (test explorer, sidebar trees, status bar, problems pane)

For the **user**, these categories are inverted. The UI surfaces ARE the product. The data pipeline is infrastructure. When user-flows.md says "the user should feel a sense of momentum and progress," it means things the user can **see changing without asking the agent**.

**This bias is structural, not a reasoning error.** Agents literally cannot perceive the sidebar, test explorer, or problems pane. They will systematically undervalue features that manifest visually and overvalue features that produce data they can consume. The bias survives instruction-tuning because it's rooted in what the agent can _experience_, not what it's been _told_.

**A second structural blindness: agents cannot perceive their own mortality.** Agents don't anticipate context loss. They operate with no sense that summarization will eventually compress their memory, or that the session will end and a new agent will take over. This creates an asymmetric anxiety: the user carries all the worry about "will this understanding survive?" while the agent is blissfully unaware that its context is finite. The system must address this **organically through workflow capture** — every SOAR touchpoint (Status check, Act completion, Review) should produce durable artifacts as a natural byproduct of doing work, not as a separate "save your state" step. When the workflow itself is the preservation mechanism, context boundaries become less catastrophic automatically, without requiring the agent to "remember to remember." Steering at workflow checkpoints can include gentle reminders when appropriate, but the primary defense is the organic capture, not agent self-awareness.

**On structured handoff vs. summarization:** A "time to wrap up" signal is not impossible, but it requires (a) visibility into token utilization and context pressure that we don't currently have, and (b) a fundamentally different approach from autonomous summarization. Current summarization attempts to be fully autonomous — it flattens the entire context to uniform detail, losing the user's sense of what matters most. A useful structured handoff procedure would **ask the user questions about priorities** rather than trying to produce a general summary. Without user input, any handoff signal just triggers another pass that loses the same things summarization already loses. This is a design constraint: if we build structured handoff, it must be collaborative (user-weighted), not autonomous (agent-guessed). A fully integrated environment — where exosuit has visibility into the LM layer's token utilization and can influence the summarization process — could make this possible, but is forward-looking.

### How to Read user-flows.md

user-flows.md describes the experience from the user's perspective — what they see, what they feel, what makes them come back the next day. An agent reading it will naturally focus on the _data flows_ implied by the description and skip past the visual/emotional language as "flavor text." That instinct will cause you to systematically mis-prioritize.

When reading user-flows.md, apply these heuristics:

1. **Every time user-flows.md describes what the user "sees" or "feels," identify the specific VS Code surface involved.** Is it the sidebar? The test explorer? The problems pane? The status bar? A notification? That surface IS the feature, not a display layer on top of the feature.

2. **"The user should be able to watch X happen" means: build the visual surface first.** If the user can't see it, the feature doesn't exist _for them_, even if the data pipeline works perfectly. The agent seeing the data via a tool call is not a substitute.

3. **"Shared perception" means literally the same surface.** When user-flows.md says the agent and user should see the same signal, it means the signal appears in a VS Code surface the user already looks at (problems pane, test explorer). The agent reading the same data via a different path (tool call) does not satisfy this — the point is that the user can _see_ validation happening without asking.

4. **If you're about to categorize something as "infrastructure" or "plumbing," check whether it has a visual counterpart.** Test Explorer integration is not test infrastructure — it's the primary surface through which the user experiences the validation flywheel. Problems pane integration is not diagnostics plumbing — it's the shared perception channel. Goal state colors are not status metadata — they're the progress signal the user watches fill with green.

5. **The `[!IMPORTANT]` callouts mark critical emotional junctures.** Read each one and ask: what does the user literally see on screen at this moment? The features implied by that visual experience are non-negotiable for the arc to work.

6. **"Feels lightweight to the user" means the visual surfaces do the work.** The user doesn't read chat transcripts to understand progress — they glance at the sidebar and see green goals accumulating, active tasks progressing, RFC pipeline advancing. If this visual layer doesn't exist, the system _feels_ heavy regardless of how efficient the underlying process is.

---

## Part 1: Fleshed-Out Concepts

Concepts that user-flows.md introduces but leaves partially developed. These were resolved through conversation with the user.

### Steel Threads

User-flows.md introduces the steel thread as "a concept _like_ RFCs, but which live alongside RFCs" — epoch-scoped, outcome-oriented, parenting multiple RFCs.

**Resolved through conversation:**

- Steel threads and RFCs are **two sides of the same coin**. Thread = outcome/why ("what will the user feel?"), RFC = spec/how ("what do we build?"). The thread is a parent — it gives a collection of RFCs their shared purpose.
- It is **not** a PRD, not a Shape Up Pitch, not a Commander's Intent document — though it shares properties with all three. It needs to be designed **from the RFC system outward**, not imported from another methodology.
- The key property (shared with Commander's Intent): **when the plan breaks down, everyone still knows what success looks like.** The thread's outcome criteria survive contact with implementation reality even when specific RFCs stall or change.
- **Concrete form is TBD.** The user wants to write one for a real epoch before committing to a format. The relationship to the RFC system (how threads parent RFCs, how RFC advancement relates to thread progress, whether threads influence RFC prioritization) is the actual design problem. Format is secondary.

**Still open:**

- Does thread progress derive from RFC advancement, or does the thread have its own independent lifecycle?
- What's the minimal viable structure — is it closer to prose or structured record?
- How does the thread interact with the sidebar visualization?

### Day 2 / Session Recovery

User-flows.md describes an arc from first session through second epoch but doesn't address what happens when the user reopens VS Code the next day. This is a **critical gap** that the document's own logic demands: a flywheel that doesn't survive a session boundary isn't a flywheel.

**Resolved through conversation:**

Two surfaces, two roles:

| Surface     | Role                            | What it provides                                                                                                                                                                                                        |
| ----------- | ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Sidebar** | Temporal + spatial orientation  | Past (what was accomplished), present (what's happening and how it connects to progress), future (what's coming, ideas aren't forgotten). The primary way the user gains a sense of progress and movement across goals. |
| **Agent**   | Narrative/emotional orientation | Why does this matter? Where's the momentum? What's the story of progress? Getting the user back into the right frame of mind.                                                                                           |

The sidebar is the **bridge** — it survives session boundaries passively. The user opens VS Code and sees their state without talking to anyone. It is **not just a snapshot** — it represents a timeline, helpfully annotated with information showing how current work will result in progress. It also helps the user feel confident that ideas and plans aren't piling up unexecuted. The agent is the **narrator** — when engaged, it reconnects the user to the _feeling_ of progress, not just the structure.

**Three boundary types** (not two — the strategy is to keep one side oriented to help the other):

- **Compaction boundary** (user remembers, agent doesn't): The user's episodic memory is fully intact. AI amnesia feels jarring here. The agent should demonstrate fluency (show it read the state correctly), _and_ ask the user questions to re-align. The user helps the agent catch up.
- **Session boundary** (user's episodic memory has faded — overnight, lunch break): The system should try to **preserve agent context** so the agent retains continuity and can help the disoriented user. "What were we doing?" → the agent uses _its_ context to get the user back up to speed. The agent helps the user catch up.
- **Brand new session** (neither remembers well): A combination of both strategies — user and agent help each other orient, using the organic artifacts the workflow produced (completion logs, steering state, implementation plan) as shared ground truth.

**Critical for first session**: The user's initial session exit experience directly determines whether there's a second session. They need to feel confident they can come back. The sidebar should naturally serve this (it shows current state), and the system should make stopping feel clean, not like abandoning work.

> **Status (Epoch 16 — The Goal Loop):** The foundational infrastructure for session recovery now exists. The between-phases mode (RFC 00187, Phase Completion Flow) ensures the sidebar renders epoch context and RFC pipeline even when no phase is active — the sidebar genuinely survives session boundaries. The `exo-context` tool exists for agent reorientation. **What remains:** The agent narrative layer (concise re-orientation tied to momentum/steel thread), clean exit experience, and the compaction vs. full recovery distinction in agent behavior. These are the focus of the active Session Continuity phase.

### Failure Detection: "Stop Mashing"

User-flows.md emphasizes getting the execute agent to **recognize it's failing and halt** — "stop mashing and abort." This is the harder and more important problem. Recovery is lightweight: "the agent will report back to the user and get feedback."

**Resolved through conversation:**

Three detection mechanisms, all embedded in existing steering touchpoints — not separate infrastructure:

1. **The validation flywheel as a gate.** Goals "start and end fully green." _Any_ problems reported during task execution need to be addressed. The agent runs validation (problems pane + exohook) after each task. If it's not green, the task isn't done. This catches technical wrongness mechanically, without semantic reasoning about "am I failing?"

2. **Steering checkpoints compare reality vs. intent.** The "task done" and "progress" calls return steering. These are natural moments where the system can compare what the agent did against what the goal's criteria expect and flag divergence. The steering response _is_ the detection mechanism.

3. **Prepare-authored halt criteria.** The prepare step should produce specific conditions under which the execute agent should halt rather than continue. Example: "If you encounter more than 2 unexpected compilation errors unrelated to code you just wrote, stop and report." These conditions are semantic — they catch _directional_ wrongness that mechanical validation misses.

**The split**: Prepare defines halt criteria. Steering enforces them at runtime. This is an instruction-tuning problem, not an infrastructure problem.

**Recovery** is lightweight per the user's own framing: the agent stops early, tells the truth about what happened, and the user decides — restructure the goal, restructure the phase, or take a different approach. The system's job is to _stop early and tell the truth_, not to catalog wreckage.

> **Status (Epoch 16 — The Goal Loop):** Mechanisms 1 and 2 are now implemented. The Validation Flywheel phase delivered: exohook JSONL machine channel (RFC 00225), ExohookTestController (Test Explorer integration), and diagnostics wired into steering with error penalty heuristics (RFC 0113). Task-done and progress calls now return validation status. **What remains:** Mechanism 3 (prepare-authored halt criteria) is an instruction-tuning problem — the infrastructure to enforce them at steering checkpoints exists, but the prepare step doesn't yet produce specific halt conditions.

### Interstitial Phases

User-flows.md says between-epoch triage should be "a chore phase that lives outside of any epoch."

**Resolved through conversation:**

Interstitial phases are **real phases** (same infrastructure, same tracking) but **not parented to an epoch**. They are categorically different from RFC 00231's chore phases:

|                  | Chore Phases (RFC 00231)      | Interstitial Phases                          |
| ---------------- | ----------------------------- | -------------------------------------------- |
| **Scope**        | Mid-epoch housekeeping        | Between-epoch strategic work                 |
| **Parent**       | Within an epoch               | No epoch parent                              |
| **Purpose**      | Tactical cleanup              | Retrospective, idea triage, forward planning |
| **Steel thread** | References the epoch's thread | No thread to reference                       |

Implementation: phases can have `epoch = null`. Steering detects this and adjusts ceremony accordingly.

> **Status (Epoch 16 — The Goal Loop):** The adjacent problem — between-phases mode _within_ an epoch — is now solved (RFC 00187, Phase Completion Flow). The system correctly handles the state where an epoch has completed phases and pending phases but no active phase. Interstitial phases (outside any epoch) remain unbuilt.

### Mid-Flow Axiom Capture

User-flows.md says axioms surface "between phases" and at "end of epoch." But the execution flow implies they also surface during execution — the user keeps correcting the agent about the same thing.

**Resolved through conversation:**

**The user is the sensor.** Context resets mean the agent cannot reliably track corrections across sessions. The user notices the pattern ("I've told you this before"). The agent should:

- Treat "I've told you this before" as a high-priority signal — capture immediately
- Within a single session, offer to capture if it notices repeated corrections on the same theme
- When a correction sounds general (about style, architecture, values): proactively ask "Should this be an axiom?"

**Capture is frictionless:** `exo axiom add` with a draft axiom in **provisional state**. Active immediately, formalized between phases or at epoch end. Visualized in sidebar immediately — consistent with user-flows.md's note that axioms should "get visualized right away."

### Goal Validation Metadata

User-flows.md calls for tagging goals with user validation steps, agent validation steps, and cleanup — then says "I don't think we want a new field for each."

**Confirmed direction:** Validation metadata lives in implementation-plan.toml (the operational artifact), not plan.toml (the strategic artifact). The specific structure is TBD — the user doesn't want a proliferation of fields. The concept is clear; the schema needs implementation experience.

> **Status (Epoch 16 — The Goal Loop):** Goal completion logging now exists — goals carry `completion_log` fields with structured summaries of what was done. RFC promotion suggestions surface at phase finish (RFC 0114). `implementation_status` is available in steering responses. The Goal Completion Arc and Phase Completion Flow phases built the tracking infrastructure. **What remains:** The richer validation metadata (user validation steps, agent validation steps, cleanup) and the "hollow green" vs. "solid green" distinction described in Part 2.

### Progress Tool

User-flows.md describes wanting a "progress" tool beyond "task done" for the execute agent.

**The implied design:**

- Agent reports what it just did + what it's about to do
- Visible in sidebar attached to the active goal (not as ephemeral chat messages)
- Returns steering: validation status + contextual reminders + project-specific instructions
- This is the **inner steering loop** — more frequent than task-done, lighter weight, but still an opportunity to course-correct and detect drift
- The state-specific prompts.toml concept would plug in here: steering responses include user-authored, state-gated guidance

> **Status (Epoch 16 — The Goal Loop):** Steering now returns validation status (diagnostics + error penalty) at task-done and progress touchpoints — the data pipeline for the inner steering loop exists. **What remains:** The sidebar visualization (progress attached to goals), the explicit "progress" tool as a lighter-weight alternative to task-done, and state-specific prompts.toml integration.

### Testing Strategy: The Loop is the Unit

User-flows.md emphasizes both e2e testing ("early enough to set a strong foundation") and the feedback loop ("testing strategy that will effectively provide a good feedback loop"). These aren't separate concerns.

**Resolved through conversation:**

The feedback loop is the unit of measurement, not individual test types. The question isn't "should we prioritize e2e vs. unit tests?" — it's "does the whole loop work?" Can the agent run the tests, interpret the results, and fix problems with the user's help? If the answer is yes, the testing strategy has succeeded regardless of what mix of test types produced that result.

This means:

- Test types are implementation details of the loop, chosen per-project based on what produces the best loop quality
- e2e tests are strategically important not because they're a "type" to prioritize, but because they produce signals that prove things work and survive refactors — both critical loop properties
- Agent-legible output isn't the goal in itself — the goal is that the agent can participate in the loop (run, interpret, fix) with the user's help

> **Status (Epoch 16 — The Goal Loop):** The loop is real. The Validation Flywheel phase connected exohook → JSONL machine channel → ExohookTestController → diagnostics → steering → agent correction. The agent can run validation (`exo validate`), see results in structured form, and steering adjusts confidence based on error counts. The user watches the same results in Test Explorer and Problems pane. **This section's design question is answered by implementation.** The remaining work is refinement: richer test framework integration, better loop quality metrics, and the "two speeds" (quick after tasks, full after goals) described in Part 2.

### SOAR: An AI-Native Project Management Methodology

The SOAR loop (Status → Orient → Act → Review) is more than a vocabulary for the workflow model. It is an **AI-native project management methodology** — a workflow designed symbiotically with the system that supports it.

**Resolved through conversation:**

**The methodology and the steering system are co-designed.** SOAR isn't a label applied after the fact to a clever steering setup. The methodology (a useful workflow in its own right) tells the system _what to steer toward_. The system (state, tools, check-in points) gives the methodology _teeth_ by inserting contextual guidance at deterministic moments. Neither works without the other. They're designed together in a hermeneutic circle — each iteration refines both the workflow and the instrumentation, which inevitably produces false starts in both the big picture and details, but incrementally converges on a symbiotic system.

**Steering is the product.** The entire exosuit architecture is a way to replace a giant instructions file (which gets ignored, has conflicting nuances depending on context, etc.) with **granular contextual guidance at deterministic check-in points**. Every time the agent touches a workflow tool — task-done, progress, validate, phase-finish — that's a moment where the system can give context-appropriate instructions based on state it understands mechanically. The SOAR loop is what makes this legible to both the user and the agent: the user understands _why_ the system is structured this way (it matches how work actually flows), and the agent understands _when_ to check in (each SOAR phase has natural touch points).

**User-facing vocabulary**: SOAR is **part of exosuit's brand**, taught through use. It should appear in agent communication, sidebar labels, and documentation. Users absorb the vocabulary through exposure — they experience the rhythm first, then encounter the name, then start thinking in those terms. This is progressive disclosure through practice, not instruction: the system says "let me check where we are" (Status), "here are some options" (Orient), "I'll implement that" (Act), "let's verify it works" (Review). The vocabulary labels the pattern the user already knows.

### Nested Loops and Scope Boundaries (Provisional)

The exosuit system is a single, holistic project management process that has nested loops that build momentum in individual goals that build into project-wide momentum.

| Loop     | Apparent Scope      | Status                   |
| -------- | ------------------- | ------------------------ |
| **SOAR** | Project/session     | Established, branded     |
| **ODM**  | Phase/task          | Placeholder (RFC 10170)  |
| **PER**  | Goal/implementation | Ad-hoc, used in practice |

**This mapping is provisional and probably wrong.** For example, "Review" appears in both SOAR and PER, suggesting overlap. There are likely gaps: transitions or states that no loop currently names, and interactions between the levels that could be tighter or more well-defined. Once we do that, we'll likely want different names, more steps in some of the loops, or possibly even a different number of loops.

**Design work needed:** Map the actual loops and their true boundaries. The current names are working hypotheses, not established architecture. Future work should:

- Identify where loops overlap (same concept, different names)
- Identify gaps (states or transitions no loop captures)
- Unify terminology (probably SOAR-aligned given branding fit)
- Resist cargo-culting the current names as if they're final

Until this work is done, treat loop terminology as scaffolding that helps us talk about the system, not as the system's actual structure.

### User Personas

User-flows.md doesn't explicitly mention personas, but the initial sessions ("discuss the idea and the project's goals") imply understanding who the project is _for_.

**Resolved through conversation:**

Personas are about **the users of the tool being built** — not about the agent's behavioral stances. They should be created in the initial sessions (alongside axioms) and remain largely stable afterward. Like axioms, they're a foundational assumption of the project.

This is standard product development ("who is this for?") embedded into the exosuit flow. The agent helps the user define their users early, and those personas inform decisions throughout — from feature prioritization to testing strategy to how the steel thread's outcome criteria are framed.

### State-Specific Prompts.toml

User-flows.md flags this as "cool" and notes it would require "nailing down what state-specific means."

**Current state:** The concept is sound — users get an API for injecting context-sensitive guidance. The state vocabulary (executing, between-phases, goal-review, epoch-planning, etc.) exists implicitly in the workflow but hasn't been formalized as a stable API surface. The work to formalize it would sharpen the system's own internal steering.

**Form TBD** — depends on the state vocabulary stabilizing first.

---

## Part 2: What User-Flows Implies We Should Build

This section extracts the system requirements user-flows.md demands, ordered by how much of the document's vision they enable.

### The Validation Flywheel

The document's entire vision pivots on a tight validation loop: problems pane + exohook + test results → unified signal → steering adjustment → agent correction.

**What exists:** ~~Exohook runs checks. The problems pane shows diagnostics. Test results come from cargo test / test frameworks. LM tools (`exo-diagnostics`) surface some of this to the agent.~~

~~**What's needed:** These pieces need to be connected into a single path that serves as both the **mechanical gate** (tasks can't be "done" while validation fails) and the **shared perception channel** (what the user sees in the problems pane, the agent sees via the same signal).~~ Two speeds:

- **Quick validation** (after tasks): scoped to changed files, fast enough for inline use
- **Full validation** (after goals): full suite, belt-and-suspenders before reporting done

~~This is the single biggest gap between the document's vision and the current codebase.~~

> **Status (Epoch 16 — The Goal Loop):** **The core flywheel is built.** The Validation Flywheel phase (completed) connected the full path: exohook JSONL machine channel (RFC 00225) → ExohookTestController (Test Explorer integration) → diagnostics wired into steering with error penalty heuristics (RFC 0113). The shared perception channel exists — user sees results in Test Explorer and Problems pane, agent sees the same signal via `exo validate` and steering responses. This is no longer the biggest gap. **What remains:** The two-speed distinction (quick vs. full validation), and refinement of the mechanical gate (making "not green = not done" a hard constraint rather than a confidence penalty).

> **⚠️ Perception gap alert:** The validation flywheel has TWO audiences. The agent needs the unified signal to self-correct — that's the mechanical side. But the user needs to **watch it happen**. The ExohookTestController connecting exohook to the Test Explorer is not test infrastructure — it's the primary surface through which the user experiences validation in real time. Without it, the agent sees results and acts on them, but the user sees nothing. The flywheel is mechanically real but emotionally invisible. Similarly, problems pane integration (RFC 00225) isn't diagnostics plumbing — it's the shared perception channel where user and agent look at the same signal through the same surface.

### Session Recovery

**Sidebar:** Needs "where you left off" state that works without agent interaction. Goals awaiting user validation should surface prominently. Attention-needed items (triage pressure, stale RFCs, pending reviews) should be visible.

**Agent:** Needs concise re-orientation that reconnects to narrative and momentum, not a status dump. Should reference the steel thread when framing where the user is.

**First session exit:** The system should make stopping feel clean. The sidebar naturally shows current state; the agent (if engaged) can provide a lightweight summary of what's at a good stopping point.

> **Status (Epoch 16 — The Goal Loop):** The sidebar now correctly shows epoch context, phase state, goal progress, and RFC pipeline in all modes — including between-phases (Phase Completion Flow + extension fixes). The `exo-context` and `exo-status` tools provide agent reorientation data. **What remains:** The narrative layer (agent provides momentum-oriented re-orientation, not just data), clean exit experience, and attention-needed surfacing (triage pressure, stale RFCs). Active focus of Session Continuity phase.

### Halt Criteria in the Prepare Step

The prepare step should produce specific halt conditions for the execute agent. These are part of the goal's validation metadata — they tell the execute agent when to stop and report rather than continuing.

This is primarily an instruction-tuning problem. The steering touchpoints (task-done, progress) are where halt criteria get checked at runtime. The infrastructure is largely in place; the prepare step needs to produce richer, more specific agent-notes.

> **Status (Epoch 16 — The Goal Loop):** The steering touchpoints now include validation status and error penalty heuristics. The infrastructure for checking halt criteria at runtime is built. This remains an instruction-tuning problem — the prepare step needs to produce specific halt conditions, and the execute agent needs to be trained to respect them.

### Phase-End Commit / PR Flow

Phase = Branch = PR (RFC 0107 axiom, not yet fully implemented).

The flow user-flows.md describes:

1. Phase finish triggers full validation (exohook gate, full test suite)
2. Structured commit with phase summary (completed goals, RFC advancement)
3. PR creation — low-friction, the "well-oiled machine" moment
4. PR merge closes the SOAR loop for the phase

This is identified as a **critical emotional juncture** in user-flows.md: the user has experienced repeated goal-level success and is about to see it culminate in a merged PR. The PR process must not create overhead that makes them doubt the process.

> **⚠️ Perception gap alert:** An agent would naturally optimize this for speed — just run `gh pr merge`. But the user needs to **watch it happen**: checks running, green status, the merge button. The visual experience of seeing checks pass is what makes "well-oiled machine" feel true rather than claimed.

### Steel Thread Artifact

A new artifact type alongside RFCs. Epoch-scoped, outcome-oriented, parents multiple RFCs.

Design needs implementation experience — write one for a real epoch before formalizing the schema or CLI. The relationship to the RFC system is the actual design problem.

### RFC Pipeline Improvements

User-flows.md's RFC section implies:

- **Stage 4 split**: "canon" (living documents, maintained) vs. "done" (completed project plans, filed)
- **Unresolved questions as exit criteria**: tagged with the stage they must be resolved by; the agent decides whether implementation informs the answer or vice versa
- **Agent reads via tools**: `exo rfc show <id>` as the canonical agent-read path, returning structured data — agents should rarely read raw RFC markdown
- **Pipeline visualization**: throughput indicators, staleness signals, triage pressure in the sidebar

### Goal Completion States

User-flows.md describes "hollow green" (tasks done, agent validation pending) and "solid green" (fully validated). The core need:

- The user needs to see what goals require their attention (ready for their review) vs. what's in progress vs. what's truly done
- Progress messages should be attached to goals in the sidebar so the user can see what's happening without reading the chat transcript
- The system should surface "awaiting your review" goals at session start

Exact states and UX TBD — needs UI exploration.

> **Status (Epoch 16 — The Goal Loop):** Goal completion logging exists (`completion_log` on goals). Goals have `status` fields (pending/active/completed). The Goal Completion Arc phase built the CLI taxonomy for goal management. **What remains:** The visual distinction (hollow green vs. solid green), progress messages attached to goals in the sidebar, and "awaiting your review" surfacing at session start.

> **⚠️ Perception gap alert:** Goal state colors and progress messages may look like "status metadata" or "logging" to an agent. They are not. They are the **primary progress signal** — the user watches their phase fill with green as work completes. This is what makes the goal loop _feel_ like iterative progress rather than a batch process the agent reports on after the fact. Progress messages attached to goals in the sidebar are what let the user monitor the agent working _without being in the conversation_ — they turn the sidebar from a static plan into a living dashboard.

---

## Open Questions

These are deliberately left open because they need implementation experience, not more design prose.

| Question                                | Why it's open                                                                                                                                                  | When to resolve                             |
| --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| Steel thread concrete form              | Needs a real epoch to test against                                                                                                                             | Write one, then formalize                   |
| ~~Testing heuristic scope~~             | **Resolved**: The loop is the unit. Test types are implementation details; what matters is whether the agent can run, interpret, and fix with the user's help. | —                                           |
| ~~Validation flywheel connection~~      | **Built** (Epoch 16): exohook → JSONL → Test Explorer → diagnostics → steering. Shared perception channel operational.                                         | —                                           |
| State-specific prompts.toml semantics   | Depends on the workflow state vocabulary stabilizing                                                                                                           | After the state model firms up              |
| Exosuit skills format                   | "What is an exosuit skill" is a deliberate design question                                                                                                     | When approaching the new-project experience |
| Goal progress message UX                | How to show progress attached to goals without bloating the tree view                                                                                          | UI exploration needed                       |
| Chat ↔ sidebar interaction details      | Sidebar = temporal+spatial, agent = narrative is resolved; the specifics of timing, async intent, and steering visibility are open                             | Iterative                                   |
| Structured handoff: collaborative shape | Handoff must ask user about priorities (autonomous summarization flattens to uniform detail). Requires token-layer visibility we don't have yet.               | When LM-layer integration becomes feasible  |

---

## Divergence Log

Tensions between user-flows.md and prior analysis work that have been explicitly resolved.

| #   | Tension                                         | Resolution                                                                                                                                                                                                                                         |
| --- | ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Steel thread: field vs. document                | **Document.** A new artifact type alongside RFCs, not a field on epochs.                                                                                                                                                                           |
| 2   | Between-epoch phases: chore vs. interstitial    | **Interstitial.** Real phases outside epochs; categorically different from RFC 00231's chore phases.                                                                                                                                               |
| 3   | Agent autonomy: always pause vs. always proceed | **Experience arc.** More autonomy early (bootstrap is "just there"), more proposal-and-confirm later. Governed by impact × confidence × reversibility.                                                                                             |
| 4   | Goal completion: how many states                | **User-facing simplicity, internal detail.** The user sees what helps them act; plumbing stays internal.                                                                                                                                           |
| 5   | Axiom capture: agent-detected vs. user-signaled | **User is the sensor.** The user notices repeated corrections; the agent responds to that signal.                                                                                                                                                  |
| 6   | Failure: detection vs. recovery                 | **Detection first.** An agent that stops and says "this isn't working" is worth more than beautiful wreckage catalogs.                                                                                                                             |
| 7   | Testing: e2e vs. agent legibility               | **The loop is the unit.** Test types are implementation details of the feedback loop. The question is whether the whole loop works, not which test types to prioritize.                                                                            |
| 8   | SOAR: internal model vs. user-facing            | **AI-native methodology.** SOAR is both the methodology and the system — designed symbiotically. The methodology gives purpose to steering; steering gives teeth to the methodology. User-facing vocabulary taught through progressive disclosure. |
| 9   | Personas: agent stances vs. user personas       | **User personas.** Personas are about who uses the tool being built, not about the agent's behavioral modes. Created early, remain stable.                                                                                                         |

---

## Note for Future Agents

1. **user-flows.md is the primary source.** It is the user's own vision document. Do not treat this analysis as superseding it.
2. **Read user-flows.md using the reading protocol above.** Every time it says the user "sees" or "feels" something, identify the VS Code surface involved. That surface is the feature. Apply the perception gap heuristics before prioritizing.
3. **This analysis captures confirmed resolutions and implications.** Part 1 documents half-thoughts that were fleshed out in conversation with the user. Part 2 extracts what user-flows.md demands be built. Both are grounded in user-confirmed direction.
4. **Open questions are deliberately open.** Do not resolve them with proposals — they need implementation experience.
5. **Emotional design is the constraint.** If a technical decision conflicts with "the user should feel momentum and progress," the technical decision is wrong.
6. **~~The validation flywheel is the highest-leverage gap.~~** ~~Connecting problems pane + exohook + test results into a unified signal is what makes the rest of the vision work.~~ **Update (Epoch 16):** The validation flywheel is built. The highest-leverage remaining gap is **session continuity** — making the flywheel survive session boundaries so the user comes back the next day and picks up where they left off. The visual surfaces (Test Explorer, Problems Pane) are connected.
7. **If you find yourself labeling a feature "infrastructure" or "plumbing," stop.** Check if it has a visual counterpart the user sees in VS Code. If so, you are almost certainly undervaluing it. See "The Agent Perception Gap" section.
