# Workflow Gaps: A Personal Assessment

> **Author**: Yehuda Katz
> **Date**: 2026-01-23
> **Status**: Working Draft — my own observations about where the system isn't working

---

## The Core Tension

I believe exosuit has **excellent workflow concepts** that I use effectively when I remember to invoke them. The problem is the gap between:

1. **The conceptual workflow** (what we're _trying_ to do)
2. **The daily practice** (what actually happens session to session)

When I _remind_ the agent of the details, things go really well. When I don't, we drift.

---

## The Corrected Model (2026-01-23)

After discussion, I want to be clear about what I believe:

**Tooling doesn't compete with practice — tooling enables practice.**

The theory of this project is that we can "bend the curve" of agent understanding by:

1. Using **structured files** (TOML) that tools can manipulate correctly
2. Creating **projections** for human readability on demand
3. **Contextually reminding** about the operating manual based on current activity

This is not a tradeoff between machine legibility and human ergonomics. We get both: TOML has more information AND is easier for the agent to manipulate correctly via tools. Projections can always be generated when humans need readable views.

**The rot happened because tooling lagged practice.** The concepts existed, but without tooling to trigger them at the right moments, the practices never got bootstrapped. The agent doesn't have a strong sense that it should work through implementation steps one-at-a-time — which breaks TDD steering AND walkthrough accumulation.

### The Pipeline

The intended flow is:

```
IDEAS ─────────────────────────────────────────────────────────┐
  (captured at pause points, processed during triage)          │
                                                               ▼
RFCs ─────────────────────────────────────────────────────► MANUAL
  Stage 0 → 1 → 2 → 3 → 4                                  (source of truth)
       │         │                                              ▲
       │         ▼                                              │
       │    ┌─────────────────────────────────────────────┐     │
       └───►│              PHASE                          │─────┘
            │  Agreement → Tasks → Walkthrough → Feedback │
            │       │                                     │
            │       ▼                                     │
            │  TDD: write test → implement → pass         │
            │       │                                     │
            │       ▼                                     │
            │  Validation (exohooks) → PR → Merge         │
            └─────────────────────────────────────────────┘
                              │
                              ▼
                    RFC Stage Promotion
```

**The flywheel**: Phase = PR. The agent already understands git flows. Attaching validation and steering to PR submission leverages existing muscle memory instead of fighting it. "Let's commit and submit a PR" is clear — we don't need to remind about tests and lints if they're glued to the PR process.

### The Bootstrapping Problem

The issue isn't missing concepts — it's that **core entry points don't trigger the full process**:

| Entry Point       | What Should Bootstrap                                                | What Actually Happens                            |
| ----------------- | -------------------------------------------------------------------- | ------------------------------------------------ |
| `exo phase start` | Agreement → scaffolded impl plan → linked RFCs → TDD steering active | Phase starts, but TDD/walkthrough don't activate |
| Task start        | TDD reminder: "write the test first"                                 | Agent just starts coding                         |
| Task complete     | Log entry for walkthrough                                            | Agent marks complete but doesn't log             |
| Phase finish      | Walkthrough review → feedback → RFC promotion check                  | Just commits                                     |

### The Sticky Parts

1. **Manual**: Updated organically when RFCs reach Stage 3/4. This is the "codified law" the agent operates from.

2. **Validation via Git**: Tests + lints glued to commit/PR process. Phase = PR means validation is automatic, not remembered.

3. **Axioms**: Core values that are _assumed sticky_ unless explicitly changed. But we need clarity on what counts as an axiom and how they connect to workflow.

### Contextual Steering (Not AGENTS.md)

A key insight: the problem with putting everything in AGENTS.md is that it fades from the agent's memory. Instead, tools should:

- Remind about related tools and concepts based on current activity
- Create a "progressively disclosed, contextual operator's manual"
- Surface the right guidance at the right moment

This is already somewhat effective and has room to grow.

---

## Gap 1: Intuitive, Memorable Practices

**The Problem**: The intended workflow exists but isn't translated into intuitive, easy-to-remember practices for both Human and AI.

**Symptoms**:

- I forget which `exo` command does what
- The agent forgets which mode to operate in
- The "right thing to do next" isn't obvious without explicit scaffolding
- We have documentation, but reading documentation isn't a practice

**What I Want**: A small set of rituals that, if followed, _automatically_ result in the correct workflow. Something like:

- "When you start a session, always run `exo status`"
- "Before writing code, always check `exo steering`"
- "After finishing a task, always..."

The practices need to fit in working memory. Right now, there's too much.

---

## Gap 2: Visibility for Me

**The Problem**: The dashboard/UI doesn't map tightly onto what we're trying to do at any given moment.

**Symptoms**:

- I look at the panes and don't immediately know "what's happening"
- Some panes I rarely use, adding friction to my mental model
- The connection between what I see and what the agent should be doing is fuzzy
- Cruft accumulates (panes, features) that don't serve the core loop

**What I Want**: When I look at my screen, I should _see_ the workflow. The visual should tell me:

- What phase are we in?
- What's the current task?
- What's blocking us?
- What's the user's intent (if any)?

---

## Gap 3: Ideas → Structured Action

**The Problem**: No tight integration between "the user might want to provide ideas" and actual practices where I provide ideas and we implement them in order.

**Symptoms**:

- I have ideas but `exo idea` feels disconnected from what happens next
- Ideas go into `ideas.toml` and... then what?
- The triage concept exists but there's no obvious moment to do it
- Ideas don't flow cleanly into phases/tasks

**What I Want**: When I have an idea, I should be able to:

1. Capture it quickly
2. Trust it will surface at the right time (before planning next phase)
3. See it integrated into the workflow (idea → RFC → phase task)

The system should be opinionated without being paternalistic. It should keep the wheels on without requiring me to remember all the glue.

---

## Gap 4: RFC ↔ Phase Integration

**The Problem**: RFCs and phases aren't connected into a coherent pipeline, even though the conceptual design calls for this.

**Symptoms**:

- RFCs exist in their directories
- Phases exist in the plan
- But there's no visualization of "RFC pipeline feeding phases"
- No tools to connect an RFC to a phase
- No automatic RFC promotion when a phase implementing it completes
- RFCs became "yet another constraint to consider" instead of "what's driving the work"

**What I Want**: RFCs should be the **why** and phases should be the **how**. The relationship should be visible:

- Stage 0 RFCs → ideas I'm considering
- Stage 1 RFCs → work I've committed to
- Stage 2 RFCs → specs for upcoming phases
- Stage 3 RFCs → what we're building now
- Stage 4 RFCs → laws codified in the Manual

The progression should be obvious and tool-supported.

---

## Gap 5: Lost Concepts

Several concepts from earlier development have been lost or weakened:

### 5a. Axioms

**Original Intent**: Core ideas of the project that we could vet future ideas against.

**Current State**: `axioms.*.toml` files exist but aren't consulted. They're not part of the workflow.

**What I Want**: Axioms as active constraints. When evaluating an RFC or idea, the system should check: "Does this align with our axioms?"

### 5b. Personas / Modes

**Original Intent**: Represent groups of people who would use the project (personas), then generalized into "work modes" for the agent.

**Current State**: `modes.toml` defines Thinking Partner, Chief of Staff, Maker — but there's no mode switching, no runtime awareness of which mode applies.

**What I Want**: Modes should be first-class. The agent should know "I'm in Planning Mode" vs "I'm in Implementation Mode" and behave differently.

### 5c. Implementation Plan (the narrative)

**Original Intent**: A separate markdown file fleshing out what we're going to build. An important planning artifact, not just a task list.

**Current State**: Merged into `implementation-plan.toml` — gained structure, lost narrative readability.

**What I Want**: Both. Structured data for tools + human-readable narrative for review.

### 5d. Walkthroughs

**Original Intent**: A dedicated, separate file created with each phase. The "story" of what happened. The feedback system was meant to integrate with this.

**Current State**: Merged into implementation plan as task logs. Lost the dedicated artifact.

**What I Want**: Walkthroughs as a phase deliverable. When a phase is done, there should be a walkthrough I can review.

### 5e. The Manual (Source of Truth)

**Original Intent**: RFCs reified into a "Manual" that, together with axioms, are the agent's primary source of truth.

**Current State**: RFCs serve as source of truth even though they're designed to be superseded and fall out of date.

**What I Want**: The Manual as compiled reality. RFCs as historical record. Clear process to keep them in sync.

---

## Gap 6: Planning Degradation

**The Problem**: We don't have a clear way to document future phase plans.

**Symptoms**:

- `plan.toml` has phases but only bare metadata
- No way to stage an _implementation plan_ for a future phase
- When starting a new phase, we reconstruct from first principles
- "Maybe we put it in ideas? Maybe there's a relevant RFC?"
- Same deal with epochs — conceptually important but no sketch-ahead capability

**What I Want**: Planning layers:

- Epoch: Sketch the overall direction, key RFCs
- Phase: Outline tasks, link to RFCs, rough acceptance criteria
- Task: The atomic work unit

I should be able to flesh out future phases _before_ we start them.

---

## Gap 7: The UI/Experience Divide

**The Problem**: The agent doesn't experience the workflow friction that I do.

**Symptoms**:

- The agent doesn't see the dashboard, so it doesn't feel cruft as friction
- Feedback on key files isn't structured, so the agent doesn't know it's missing
- The agent treats "incomplete feedback system" the same as "incomplete test for reactivity"
- I feel lost about what's supposed to happen, but the agent can't perceive this

**What I Want**: Some way for the agent to "experience" or at least model:

- Whether the UI provides good feedback
- Whether the workflow feels smooth
- Whether I'm confused or oriented
- Whether the current state is "clean" or "cruft-laden"

This is the hardest gap — how do you give an AI a sense of UX friction?

---

## Root Cause Hypothesis

**Corrected understanding**: The tooling lagged, so the practices it was meant to enforce never got bootstrapped. The concepts exist in files, but without tooling to trigger them at the right moments, they fade.

Key dynamics:

1. Tooling was built based on our interests rather than prioritizing parity with workflows we had
2. Concepts got merged ("implementation plan + tasks + walkthrough → single TOML") but the workflow glue got lost
3. Entry points (like `exo phase start`) don't fully bootstrap the processes they should trigger
4. The agent doesn't work through implementation steps one-at-a-time, which breaks downstream effects (TDD, walkthroughs)

**The result**: Sophisticated machinery, concepts that exist but don't activate, missing bootstrapping.

---

## The Anti-One-Shot Insight

The ecosystem is implicitly optimized for **one-shot**: fully specify what's in your mind so the AI can autonomously do it. This works great for the first shot, but context rot accumulates over repeated interactions.

Exosuit is explicitly **anti-one-shot**: a repeatable process for multi-phase work with minimal context rot. The goal is to make every piece of AI work feel as good as that first magical one-shot — but across days, weeks, or months.

| One-Shot Thinking       | Exosuit Thinking                |
| ----------------------- | ------------------------------- |
| Fully specify upfront   | Discover through phases         |
| Autonomous execution    | Human gates at phase boundaries |
| Context is chat history | Context is persistent files     |
| Hope it works out       | Validation glued to git flows   |

This conversation proves we haven't fully solved it yet, but it's much better than without, and the vision needs to feel real in the code.

---

## The Strict Engineering Paradox

The ecosystem says "strict typing is too hard for agents." The reality is the opposite.

The more types capture error conditions (reliably, without massive false positives), and the better the errors explain _why_ there's an error, the more AI stays within the four corners of high-quality code.

**Exohook** is designed around this: validation optimized for agent feedback.

The paradox:

- Strict feels harder → but errors give better signal
- Linting feels pedantic → but invokes a more rigorous slice of the model
- TDD feels slow → but forces thinking before coding
- Just _thinking_ about rigorous engineering summons a better slice of the model

You're less likely to get "you told me to fix your tests so I deleted them" when the system is oriented toward rigor.

---

## The Hermeneutic Circle Approach

We can't just tunnel on one piece because the parts interlock:

- TDD steering only works if phases bootstrap properly
- Walkthroughs only accumulate if tasks are worked one-at-a-time
- RFC promotion only triggers if phases are linked to RFCs
- The flywheel only works if it's attached to git flows

So we need to iterate between:

1. **Big picture**: Ensure we're advancing the overall vision
2. **Focused implementation**: Build pieces that most advance the big picture
3. **Integration**: Connect pieces so they unlock synergistic power

This is not "find the highest priority single thing" but "move the ball forward across multiple fronts toward unlocking synergy."

---

## What This Document Is For

This is my personal working document to:

1. Capture my observations in my own words
2. Serve as a touchstone as we work through these issues
3. Be updated as we identify solutions
4. Help me articulate the problems to others

This is NOT a spec or an RFC. It's my perspective.

---

## Action Items (To Be Filled In)

- [ ] Archaeological dig: early commits, lost concepts
- [ ] RFC analysis: workflow integration patterns
- [ ] Identify: what concepts to restore vs. what to retire
- [ ] Design: tighter workflow loop
- [ ] Implement: tooling for restored concepts
- [ ] Verify: does it feel right?
