# Blog Post Outline: Introducing Exosuit

> **Status**: Early Draft
> **Purpose**: Outline for release blog post
> **Note to self**: Keep authentic. Avoid "THIS REFRAMES EVERYTHING" energy.

---

## Working Title Options

- "Exosuit: A Workspace-First Approach to AI Collaboration"
- "Your Project as the Source of Truth"
- "Beyond Chat: Structured Human-AI Collaboration"
- "The Exosuit Manifesto" (too grandiose?)

---

## The Hook (To Be Refined)

What's the one thing that makes exosuit different?

**Candidates**:

1. **Workspace as Source of Truth**: Your project files are the source of truth, not the AI's memory
2. **Phased Execution**: Breaking work into plan → implement → verify → transition
3. **Structured Context**: TOML/Markdown files that both humans and AI can read
4. **Portable by Design**: Your project works even without the extension

**Draft Hook**:

> Most AI coding assistants treat your project as input to a conversation. Exosuit treats conversation as input to your project.

---

## The Problem (What People Experience)

- AI is stateless — every session starts fresh
- Long projects lose coherence
- "What were we doing again?"
- AI makes changes that contradict earlier decisions
- No audit trail of decisions
- The human becomes the fragile memory layer

**Concrete pain points**:

- Starting a new chat session and having to re-explain everything
- AI implementing something you discussed but then forgetting the constraints
- Multi-day projects where the AI "forgets" the architecture
- No record of why decisions were made

---

## The Insight (What We Realized)

_[This section needs fleshing out — what was the "aha moment"?]_

Something about:

- Projects already have structure (files, folders, docs)
- That structure could _be_ the AI's memory
- If decisions are recorded in the project, they survive chat sessions
- The workspace should drive the AI, not vice versa

**The Inversion**:
Traditional: AI has context → reads project → makes changes
Exosuit: Project has context → AI reads project → makes changes → updates project context

---

## How Exosuit Works (The Core Concepts)

### 1. Context is Files

```
docs/agent-context/
├── plan.toml          # The big picture
├── ideas.toml         # Captured thoughts
├── inbox.toml         # User intent
└── current/
    └── implementation-plan.toml  # The current work
```

Every session, the AI reads these. Every session, the AI updates these.

### 2. Phased Execution

Work happens in phases:

```
PLAN → IMPLEMENT → VERIFY → TRANSITION
```

- No code before plan is approved
- No transition before verification passes
- Commit at every phase boundary

Phases live inside Epochs (major milestones).

### 3. RFCs as Laws

Decisions are recorded as RFCs (Request for Comments):

- Stage 0: Idea
- Stage 1: Proposal
- Stage 2: Detailed Spec
- Stage 3: Implemented
- Stage 4: Stable

The Manual (`docs/manual/`) is the compiled source of truth.

### 4. The exo CLI

Tools that enforce the workflow:

```bash
exo status        # Where are we?
exo steering      # What should we do next?
exo phase start   # Begin a new phase
exo task complete # Mark work done
```

### 5. Tooling Independence

The workspace is portable. The extension helps, but the project structure works without it.

---

## What Makes This Different (Differentiators)

_[This section needs research — what are others doing?]_

**Categories to address**:

1. vs. Cursor / GitHub Copilot Chat (stateless assistants)
2. vs. Cody / Continue (IDE-integrated, but still chat-centric)
3. vs. Aider / Claude Dev / etc. (CLI-based, still session-focused)
4. vs. Mentat / Devin (autonomous agents)
5. vs. Custom prompt engineering (manual context management)

**Our unique position** (hypothesis):

- Not trying to replace the developer
- Not trying to be fully autonomous
- Structured collaboration with clear phases
- The project structure IS the product (portable)
- Human always in the loop at phase boundaries

---

## The Aspirational Loop (What It Should Feel Like)

_[This needs to match reality or be clearly marked as "vision"]_

**Morning Session**:

```
$ exo status
Phase 4: CI Integration
Tasks: 5 complete, 0 in progress, 0 pending
Status: ReadyToFinish

> Looks like we finished yesterday. Let's transition.

$ exo phase finish
✓ All tasks complete
✓ Verification passed
✓ Committed: "Phase 4: CI Integration"
```

**New Phase**:

```
$ exo phase start phase-5
Starting Phase 5: Dashboard Enhancements

Implementation plan created. Ready for planning.
```

**The Agent knows**:

- What we're building (reads plan.toml)
- What decisions we've made (reads RFCs)
- What the user wants (reads inbox.toml)
- What we're doing right now (reads implementation-plan.toml)

**The Human sees**:

- Clear phase status
- Current task
- Next step suggestions
- No cruft

---

## Honest Assessment (What's Not Perfect)

_[Include this to maintain credibility]_

- Still evolving
- Some tooling gaps
- Requires discipline to maintain
- Not magic — it's structured collaboration
- Learning curve for the workflow concepts

---

## Who This Is For

- Developers working on multi-day AI-assisted projects
- Teams wanting audit trails of AI decisions
- Anyone who's frustrated by AI "forgetting" context
- Builders who want their project to be self-contained

---

## Call to Action

_[What do we want people to do?]_

- Try it: [link]
- Read the manual: [link]
- Contribute: [link]
- Give feedback: [link]

---

## Research Needed

1. **Competitive Landscape**: What are Cursor, Cody, Aider, Mentat, etc. actually doing? What are their pain points?
2. **User Pain Points**: What do people actually complain about with AI assistants?
3. **The Differentiator**: What's the one thing we do that nobody else does?
4. **Case Studies**: Can we show before/after for a real project?

---

## Notes to Self

- Keep it grounded, not hype-y
- Show, don't tell (code examples, screenshots)
- Acknowledge this is a particular philosophy, not the One True Way
- The power is in the structure, not in the AI
- "Exosuit" = exoskeleton for your project
