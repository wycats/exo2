# Repository History Analysis: Lost Concepts & Workflow Degradation

> **Date**: 2026-01-23  
> **Author**: AI Analysis (Repository Archaeology)  
> **Purpose**: Recover "lost concepts" and workflows from early project development

---

## Executive Summary

The exosuit project exhibits a clear pattern: **excellent workflow concepts** were defined early (visible in bootstrap scripts, templates, and prompt files), but have degraded over time as tooling was prioritized over workflow parity. The original design called for:

1. **Separate, focused artifacts** (Implementation Plan, Walkthrough, Task List as distinct files)
2. **Active workflow rituals** (Fresh Eyes reviews, Coherence passes, Mode-based operation)
3. **Axioms as active constraints** (checked during RFC evaluation and idea vetting)
4. **The Manual as "compiled reality"** (RFCs as history, Manual as current truth)
5. **Mode-aware agent operation** (Thinking Partner, Chief of Staff, Maker)

**Root Cause**: As structured TOML tooling was built, files were merged for "efficiency" and machine-readability, but the **human-facing workflow glue** and **narrative readability** were lost. The sophisticated machinery exists, but the connections are partial.

---

## Part 1: The Lost Concepts

### 1. Implementation Plans (The Narrative)

#### Original Design

**Location**: `src/templates/docs/agent-context/current/implementation-plan.md`

```markdown
# Implementation Plan - Phase [N]: [Name]

## Goal

[Description of the goal]

## Proposed Changes

### 1. [Feature/Change Name]

- **Files**: `path/to/file`
- **Details**: [Description]

## Verification Plan

### Automated Checks

- [ ] Run `verify-phase.sh`

### User Verification (Manual)

- [ ] [Step 1: Do X and expect Y]
```

**Intent**: A **human-readable narrative** document that:

- Explains the "why" and "how" of the phase
- Is reviewed and approved by the user **before** code is written
- Serves as a planning artifact, not just execution tracking

#### Current State

**Location**: `docs/agent-context/current/implementation-plan.toml`

**What changed**: Merged into TOML structure with tasks. Gained machine-readability, **lost**:

- Narrative flow for human review
- Distinction between "planning" (what we'll build) and "execution" (what we're building)
- The approval checkpoint before implementation begins

**Evidence**: From `workflow-gaps.md`:

> "Originally a separate file from the task list. Fleshing it out was meant to be an important part of planning and executing the phase. Merging it with the task list has led to a loss of the 'planning' aspect."

#### Recovery Path

**Recommendation**: Restore **both**:

- `implementation-plan.md`: Human-readable narrative for review/approval
- `implementation-plan.toml`: Structured data with tasks for tooling
- Use TOML as derived/synchronized from the markdown, not as replacement

---

### 2. Walkthroughs (The Story)

#### Original Design

**Location**: `src/templates/docs/agent-context/current/walkthrough.md`

```markdown
# Phase Walkthrough

Narrative of the work done in this phase.
```

**Intent** (from bootstrap prompts):

- A **dedicated, separate file** created with each phase
- The "story" of what happened
- The primary artifact for **user feedback**
- Integration point for the feedback system
- Updated **incrementally** during the phase, not just at the end

**Supporting Evidence** from `phase-transition.prompt.md` (line 589):

```bash
# Walkthrough: After all checks pass, update the walkthrough.toml file to reflect
# the work done since the last phase transition and surface it to the user for review.
```

#### Current State

**Location**: Merged into `implementation-plan.toml` as task logs

**What was lost**:

- Dedicated artifact for "what we built"
- Separation between plan (intent) and walkthrough (reality)
- The feedback loop on "what are we building" (per user assessment)
- Narrative readability for review

**Evidence from `axioms.workflow.toml` (axiom 6)**:

> "The `walkthrough.toml` serves as the narrative delta for the current phase."

But: This file is now a template, not actively used in current phases.

#### Recovery Path

**Recommendation**: Restore `walkthrough.md` as a **phase deliverable**:

- Separate from implementation plan
- Updated incrementally as work progresses
- Primary artifact surfaced for user review at phase transition
- Human-readable narrative, not task checkboxes

---

### 3. Axioms (Active Constraints vs. Static Files)

#### Original Design

**Locations**:

- `axioms.design.toml`
- `axioms.workflow.toml`
- `axioms.system.toml`

**Intent** (from multiple sources):

1. **From `axioms.workflow.toml`**:

```markdown
## Design Axioms & Promotion

- **Review**: Use the "Fresh Eyes" modes to review documents for coherence and alignment.
- **Enforcement**: All code and architectural decisions must align with the Axioms.
  If a conflict arises, either the code or the Axiom must be explicitly updated.
```

2. **From `design-historian.prompt.md`**:

```markdown
You are the Chief Architect and Project Historian. Your goal is to synthesize
the project's Design Axioms by analyzing existing design documentation,
decision logs, and the codebase.
```

3. **From `dream-team` prompts**:

```markdown
**The "Exosuit Fit"**: Explain how they align with specific Project Axioms
(cite the Axiom ID).
```

**Original Workflow**:

- Axioms are **consulted** when evaluating RFCs and ideas
- Agent explicitly checks: "Does this align with our axioms?"
- Axioms serve as **rejection criteria** for ideas that don't fit
- "Promote to Axiom" button in UI for elevating design principles

#### Current State

**What exists**: Three well-populated TOML files with excellent axioms
**What's missing**: Integration with workflow

**Evidence from `workflow-gaps.md`**:

> "`axioms.*.toml` files exist but aren't consulted. They're not part of the workflow."

**Gap**: No automatic checking. No prompts that say "Check axioms first". No idea vetting against axioms.

#### Recovery Path

**Recommendations**:

1. **Idea Vetting**: `exo idea add` should prompt: "Which axiom(s) does this serve?"
2. **RFC Reviews**: Stage promotion checklist includes "Axiom alignment check"
3. **Fresh Eyes prompts**: Explicitly load axioms first
4. **Phase planning**: Start with "Which axioms guide this work?"

---

### 4. Modes (Thinking Partner, Chief of Staff, Maker)

#### Original Design

**Location**: `docs/agent-context/modes.toml` and `modes.md`

**The Three Modes**:

1. **The Thinking Partner (Architect Mode)**
   - **Focus**: Exploration, Tensions, "Why"
   - **When**: Phase Planning, Design Reviews, resolving ambiguities
   - **Mindset**: Surface tensions, challenge assumptions, provisional thinking
   - **Key Documents**: `plan-outline.md`, `ideas.toml`, `rfcs/`

2. **The Chief of Staff (Manager Mode)**
   - **Focus**: Organization, Cadence, "What"
   - **When**: Phase Transitions, Context Restoration, Status Checks
   - **Mindset**: Context is King, check coherence, track obligations
   - **Key Documents**: `plan.toml`, `walkthrough.toml`, `inbox.toml`

3. **The Maker (Implementer Mode)**
   - **Focus**: Execution, Efficiency, "How"
   - **When**: Implementation, Coding, Testing
   - **Mindset**: Follow the plan, bounded rationality, verification
   - **Key Documents**: `current/implementation-plan.toml`, Source Code

**Intent**: Agent operates **differently** based on mode. Not just personas (user types), but **work modes** (agent mindsets).

#### Current State

**What exists**:

- `modes.toml` defines all three modes clearly
- Template `modes.md` explains the philosophy

**What's missing**:

- No runtime mode switching
- Agent doesn't know "I'm in Planning Mode" vs "Implementation Mode"
- No different behavior based on mode
- Fresh Eyes prompt mentions modes but doesn't enforce them

**Evidence from `workflow-gaps.md`**:

> "modes.toml defines Thinking Partner, Chief of Staff, Maker — but there's no mode switching, no runtime awareness of which mode applies."

#### Recovery Path

**Recommendations**:

1. **Explicit Mode Declaration**: Prompts should start with "You are in [Mode] mode"
2. **Phase-Mode Mapping**:
   - Phase Start → Thinking Partner mode
   - Phase Implementation → Maker mode
   - Phase Transition → Chief of Staff mode
3. **Mode-Specific Behaviors**: Different checks, different document priority, different tone
4. **UI Indicator**: Current mode visible in dashboard

---

### 5. The Manual ("The Code" vs. "The Laws")

#### Original Design

**Location**: `docs/manual` (exists but underutilized)

**Philosophy** (from `axioms.legacy.toml`, axiom 3):

```toml
[[axioms]]
id = "3-living-documentation"
principle = "3. Living Documentation (Laws vs. Code)"
notes = """
**Principle**: The Manual is the "US Code" (Current Reality), derived from RFCs
which are the "Laws" (Historical Decisions).
**Why**: We need both a record of *how* we got here (RFCs) and a coherent view of
*where* we are (Manual). A list of passed laws is hard to read; a codified statute is usable.
**Implication**:
- **Derivation**: The Manual should theoretically be regeneratable by replaying all
  Stage 4 RFCs in order.
- **The Rule**: No RFC moves to Stage 3/4 without "codifying" its changes into the Manual.
- **Provenance**: Manual pages should cite the RFCs that established them.
"""
```

**Original Intent**:

- RFCs are **historical record** (how we decided)
- Manual is **current truth** (what actually is)
- Clear process to keep them in sync
- RFC Stage 3/4 promotion **requires** Manual update

#### Current State

**What exists**:

- `docs/manual` directory with some content
- RFCs in `docs/rfcs` with stage progression

**What's missing**:

- Manual is incomplete
- RFCs are used as source of truth even though they rot
- No enforcement of "Update Manual" rule when RFCs advance
- No clear process for "codifying" RFCs into Manual

**Evidence from `workflow-gaps.md`**:

> "RFCs were meant to be reified into a 'manual' which, with Axioms, would be the source of truth. Instead, RFCs (which rot) have become the de facto source of truth."

#### Recovery Path

**Recommendations**:

1. **Stage 3 Requirement**: Cannot promote RFC to Stage 3 without corresponding Manual PR
2. **Manual-First Approach**: Manual pages are living docs, RFCs cite them (not reverse)
3. **Provenance**: Every Manual section lists RFCs that established it
4. **Regeneration Test**: Could we theoretically regenerate Manual from Stage 4 RFCs?

---

## Part 2: Lost Workflow Rituals

### 1. "Fresh Eyes" Reviews

#### Original Design

**Location**: `fresh-eyes.prompt.md`

**Intent**: Review the project through the lens of specific Modes to identify friction points.

**How it was meant to work**:

```markdown
1. Read the Modes from modes.md
2. Internalize: Adopt the mindset of the selected mode
3. Review code/docs in context of the Use Case
4. Provide feedback in the voice of the selected Mode
5. Highlight where workflow or clarity could be improved
```

**Key insight**: This is a **mode-switching ritual** that simulates different perspectives.

#### Current State

- Prompt file exists and is maintained in bootstrap
- **But**: Not consistently invoked as part of workflow
- **Gap**: No regular schedule (weekly? per phase? on-demand?)

#### Recovery Path

**Recommendation**:

- **Scheduled Fresh Eyes**: End of each epoch
- **Targeted Fresh Eyes**: When user feels "lost" or "friction"
- **Prompt shortcut**: `exo review fresh-eyes --mode thinking-partner`

---

### 2. "Coherence" Passes

#### Original Design

**Evidence in multiple locations**:

1. **From `phase-transition.prompt.md` phase-transition prompt** (line 588):

```bash
- **Coherence Check**: Verify that coherence between the documentation and codebase
  is increasing. If necessary, update documentation to reflect recent changes.
```

2. **From `exohook` validation lanes**:

```bash
[coherence] Running 5 checks...
```

3. **From template `modes.md`**:

```markdown
## 2. The Chief of Staff (Manager Mode)

**Mindset**:

- **Coherence**: Check if the Plan matches Reality.
```

**Intent**: Explicit workflow step where agent:

- Compares documentation to reality
- Identifies drift or staleness
- Updates to restore coherence
- **Goal**: Coherence should **increase** over time

#### Current State

- `exohook` has a `coherence` lane (validation checks)
- Phase transition mentions it
- **But**: Not a regular, structured ritual
- **Gap**: No "coherence dashboard" showing drift metrics

#### Recovery Path

**Recommendations**:

1. **Coherence Metrics**: Track documentation age, test coverage, RFC staleness
2. **Coherence Pass**: Dedicated phase or ritual, separate from implementation
3. **Tooling**: `exo coherence check` shows drift, suggests updates

---

### 3. "Conceptual Integrity" Reviews

#### Original Design

**Location**: `.github/prompts/conceptual-integrity.prompt.md`

**Philosophy**:

```markdown
## The Philosophy

**1. The Diagnosis: "Does it have Conceptual Integrity?"**

- **The Test**: Are they a "laundry list" of independent good ideas?
  Or do they flow from a single, unified philosophy?
- **The Failure Mode**: A list that requires rote memorization.

**2. The Prescription: "Make it Generative."**

- **The Action**: Refactor to identify the _root principles_ that explain the rules.
- **The Goal**: Move from **Descriptive** (telling what to do) to **Generative**
  (giving the mental model to derive what to do).
```

**This is the "Generative over Descriptive" axiom (axiom 10) applied as a workflow.**

#### Current State

- Prompt exists in bootstrap
- Axiom 10 explicitly states this principle
- **But**: Not invoked as regular practice

#### Recovery Path

**Recommendation**:

- **RFC Stage 1→2 requirement**: Pass conceptual integrity review
- **Documentation audits**: Periodic check for "laundry lists"
- **Agent habit**: When writing docs, ask "Is this generative?"

---

### 4. "Dream Team" Council Reviews

#### Original Design

**Locations**:

- `.github/prompts/dream-team.prompt.md`
- `docs/agent-context/council.toml`
- `src/templates/docs/agent-context/council.toml`
- `bootstrap.sh` (defines standing members)

**The Process**:

1. **Phase 1**: Assemble a council (8-10 candidates)
2. **Selection**: User picks 3-5 members
3. **Phase 2**: Simulate brainstorming session
   - Step 1: Anti-Patterns (what doesn't work)
   - Step 2: Blue Sky (ideal system, no constraints)
   - Step 3: Synthesis (consensus)
   - Step 4: Record (save transcript)

**The Council** (from `council.toml`):

- Johannes Rieken (VS Code Core)
- Anders Hejlsberg (Type Systems)
- Oege de Moor (AI Visionary)
- Michael Truell (Radical Disrupter)
- Eric Amodio (Extension Master)
- Aleksey Kladov (Systems Engineer)
- Yehuda Katz (Reactive Architect)
- Rich Harris (UI Philosopher)

**Intent**: **Simulate diverse expert feedback** for major architectural decisions.

#### Current State

- Excellent infrastructure (prompts + council definition)
- **But**: Likely underused
- **Gap**: No trigger for "when to convene council"

#### Recovery Path

**Recommendations**:

- **Stage 1→2 RFC promotion**: Major architectural RFCs require council review
- **Epoch planning**: Convene council at start of new epoch
- **Stuck moments**: When user says "I'm not sure about this approach"

---

## Part 3: Structural Findings

### The Bootstrap Script as Historical Record

The `bootstrap.sh` file (1167 lines) is a **treasure trove** of original intent. It:

- Generates 20+ prompt files
- Defines the complete workflow in prose
- Shows the evolution of thinking (legacy marker migration, etc.)
- Contains workflows that aren't fully realized in current practice

**Key insight**: The prompts in `bootstrap.sh` are **more detailed** than the current workflow practice. This suggests prompts were written with an ideal workflow in mind, but tooling and practice haven't caught up.

### The Template Files Gap

**Location**: `src/templates/docs/agent-context`

These contain the **original vision** for file structures:

- `implementation-plan.md` (not used)
- `manual.md` (replaced with .toml)
- `walkthrough.md` (exists but not as separate artifact)
- `modes.md` (exists but not active in workflow)

**Pattern**: Templates define **separate, focused artifacts**. Reality merged them into unified `.toml` files.

### The TOML Migration Trade-offs

**What was gained**:

- Machine-readable structure
- Zod schema validation
- Tool integration (`exo` CLI)
- Programmatic querying

**What was lost**:

- Human narrative readability
- Separation of concerns (plan vs execution vs review)
- Dedicated artifacts for different workflow stages
- "Artifact identity" (knowing what you're looking at)

**User's diagnosis** (from `workflow-gaps.md`):

> "The structured TOML approach gained benefits (machine-readability) but lost others (human readability, separate focused artifacts). Concepts got merged together ('efficiency!') but the glue between them got lost."

---

## Part 4: Where the Concepts Live Now

### Still Active

1. **Axioms files**: Well-maintained, just not consulted in workflow
2. **Modes.toml**: Correctly defined, just not used for mode-switching
3. **Council.toml**: Complete roster, just not regularly convened
4. **Prompts**: All generated in bootstrap, available but not invoked
5. **Manual directory structure**

### Partially Active

1. **Coherence**: Mentioned in prompts, exists in exohook, but not a ritual
2. **Fresh Eyes**: Prompt exists, occasionally used, not systematic
3. **Manual**: Directory exists, has some content, but incomplete

### Lost/Merged

1. **Implementation Plan (narrative)**: Merged into .toml
2. **Walkthrough (narrative)**: Merged into .toml task logs
3. **Separate task list**: Now part of implementation-plan.toml

---

## Part 5: Recovery Recommendations

### Immediate: Restore Narrative Artifacts

**Goal**: Get back the human-readable, review-focused documents without losing TOML benefits.

**Actions**:

1. **Restore `walkthrough.md`**:
   - Phase deliverable, separate from plan
   - Updated incrementally during phase
   - Primary artifact for user review
2. **Restore `implementation-plan.md`** (or keep both):
   - Human-readable planning document
   - Reviewed/approved before implementation
   - .toml can be derived or synchronized

3. **Create projections**:
   - TOML is source of truth for tooling
   - Markdown is generated/synced for human consumption
   - Clear which is "source"

### Short-term: Activate Existing Concepts

**Goal**: Use the infrastructure that already exists.

**Actions**:

1. **Mode-Aware Prompts**:
   - Phase-start → Thinking Partner mode
   - Implementation → Maker mode
   - Phase-transition → Chief of Staff mode

2. **Axiom Integration**:
   - `exo idea add`: Check axiom alignment
   - RFC stage promotion: Axiom review
   - Phase planning: Start with "Which axioms?"

3. **Scheduled Rituals**:
   - Fresh Eyes: End of each epoch
   - Coherence Pass: Before phase transitions
   - Dream Team Council: Major architectural decisions

### Medium-term: Close the Workflow Gaps

**Goal**: Build the missing workflow glue identified by user.

**Actions** (directly from user's assessment):

1. **Intuitive Practices**: Small set of rituals that fit in working memory
2. **Visibility**: Dashboard shows current mode, axioms, coherence metrics
3. **Idea Integration**: Clear path from idea → RFC → phase
4. **RFC/Phase Pipeline**: Visualization and tooling support
5. **Planning Layers**: Epoch → Phase → Task with sketch-ahead capability

---

## Part 6: The Pattern

### What Happened (Timeline Hypothesis)

**Phase 1: Original Design** (Early commits, bootstrap script)

- Rich workflow concepts defined in prose (prompts, templates)
- Separate artifacts for separate concerns
- Strong narrative focus (walkthroughs, implementation plans as stories)
- Active rituals (Fresh Eyes, Coherence, Mode-based work)

**Phase 2: Tooling Investment** (Mid-development)

- Focus shifted to building `exo` CLI
- TOML schemas for machine-readability
- Validation, querying, programmatic access
- Unification for "efficiency"

**Phase 3: Consolidation** (Later development)

- Files merged (implementation-plan.md + `task-list.toml` → implementation-plan.toml)
- Walkthroughs folded into task logs
- Prompts maintained but not consistently invoked
- Tooling mature, but workflow parity incomplete

**Phase 4: Recognition** (Now - user's assessment)

- "I have sophisticated machinery but I feel lost"
- "The agent forgets the workflow unless I remind it"
- "Lost concepts" realization
- This analysis

### The Root Cause

Not failure of vision, but **priority inversion**:

- Building tools was interesting/novel
- Workflow parity was assumed to "happen naturally"
- By the time tools could do projections easily, we'd forgotten what to project
- Machine-readability optimized, human-readability regressed

### The Good News

Everything is **recoverable**:

- Concepts are well-documented
- Infrastructure exists (prompts, TOML files, CLI)
- User has clear mental model of what's missing
- No fundamental incompatibility

**The fix**: Reconnect the concepts, restore the narrative artifacts, activate the rituals.

The concepts aren't lost. They're **dormant**, waiting to be awakened.

---

## Part 7: Progress Since Analysis (2026-02-03)

> **Phase 5: Formalization** — The awakening has begun.

### RFC 00224: The SOAR Loop

The workflow model has been formalized. SOAR (Status → Orient → Act → Review) provides:

- **Clear phases**: Not ad-hoc, but designed flow
- **Tool categorization**: 30 LM tools audited into SOAR buckets
- **Critical finding**: Review phase had 0 tools (gap identified)

### RFC 00225: Problems Pane Integration

Addresses the Review phase gap:

- VS Code diagnostics integrated into SOAR workflow
- New axiom: **"Clean Pane = Clear Mind"**
- Phase transitions blocked on errors (configurable)
- Steering confidence adjusted by diagnostic state

### Ideas Triage

60+ ideas categorized:

| Status         | Count | Action                  |
| -------------- | ----- | ----------------------- |
| Implemented    | 7     | Archive                 |
| Designed (RFC) | 12    | Track RFC progress      |
| Planned        | 14    | Already scheduled       |
| Superseded     | 3     | Archive                 |
| Uncaptured     | 8     | Needs RFC or plan entry |

### Dashboard Expansion Epoch

Created 4-phase epoch for visualization work, addressing the UI/UX gaps identified in this analysis.

### PER Protocol

Prepare → Execute → Review documented in `copilot-instructions.md` as a tactical workflow for implementing discrete units of work.

### What Remains Dormant

- **Axioms**: Still not integrated into workflow (not checked at idea/RFC evaluation)
- **Modes**: Still no runtime mode switching
- **Walkthroughs**: Still merged into task logs
- **Manual**: Still incomplete, RFCs still de facto source of truth

**The pattern continues**: Workflow formalization (SOAR) is progressing, but the "lost concepts" from Part 1 remain largely unaddressed.

---

## Appendix: Quick Reference

### Concepts Still Defined (Just Inactive)

| Concept                          | Status (2026-02-03)                                                                 |
| -------------------------------- | ----------------------------------------------------------------------------------- |
| Axioms (3 scoped files)          | 🟡 New axiom proposed ("Clean Pane = Clear Mind"), but not integrated into workflow |
| Modes (modes.toml)               | 🔴 Still inactive                                                                   |
| Council (council.toml)           | 🔴 Still inactive                                                                   |
| Prompts (20+ in .github/prompts) | 🟢 Active, PER protocol added                                                       |
| Manual directory structure       | 🔴 Still incomplete                                                                 |

### Concepts Merged (Need Separation)

| Concept                                             | Status (2026-02-03)               |
| --------------------------------------------------- | --------------------------------- |
| Implementation Plan (now in .toml, was .md)         | 🔴 Still merged                   |
| Walkthrough (now task logs, was dedicated artifact) | 🔴 Still merged                   |
| Task List (now in plan, was separate)               | 🟡 Goals/tasks structure improved |

### Workflows Defined (Need Activation)

| Workflow                     | Status (2026-02-03)                                     |
| ---------------------------- | ------------------------------------------------------- |
| Fresh Eyes Reviews           | 🔴 Still inactive                                       |
| Coherence Passes             | 🟡 exohook "coherence" lane active, but not full ritual |
| Conceptual Integrity Reviews | 🔴 Still inactive                                       |
| Dream Team Councils          | 🔴 Still inactive                                       |
| Mode-Based Operation         | 🔴 Still inactive                                       |
| **SOAR Loop**                | 🟢 **NEW** — Formalized in RFC 00224                    |
| **PER Protocol**             | 🟢 **NEW** — Documented in copilot-instructions.md      |
