# Exosuit: Key Differentiators

> **Status**: Working document — refined through discussion 2026-01-23
> **Slogan**: A system for building high-quality software with AI

---

## The Core Insight

The AI ecosystem is implicitly optimized for **one-shot**: fully specify what's in your mind so the AI can autonomously do it. This works great for the first shot, but **context rot** accumulates over repeated interactions.

Exosuit is a **repeatable process** for multi-phase work with minimal context rot. It's designed to make every piece of AI work feel as good as that first magical one-shot — but across days, weeks, or months.

---

## 1. New Workflow Concepts for AI Collaboration

This isn't just "put stuff in files" (everyone does that). Exosuit defines **new concepts** — adaptations of project management, rethought for AI collaboration to take best advantage of what AI is good at and give the human user more leverage.

| Concept          | What It Is                                       | Why It Matters for AI                         |
| ---------------- | ------------------------------------------------ | --------------------------------------------- |
| **Phases**       | Unit of committed work (not tasks, not sessions) | Clear boundary for human approval             |
| **Epochs**       | Thematic milestones grouping phases              | Big-picture direction without micromanagement |
| **RFC Pipeline** | Design → implementation → law (Stage 0→4)        | Decisions accumulate into permanent record    |
| **Walkthroughs** | Execution narrative accumulated during phase     | No "what did we do?" reconstruction           |
| **TDD Steering** | Invoked by task start, not remembered            | Quality without relying on AI memory          |

These are new concepts, not just adaptations of issues/PRs/docs.

---

## 2. Bending the Curve with Coherent Projections

The command spec refactor makes CLI, VSCode UI, and LM tools all **project the same data coherently**. This bends the curve.

**The insight**: We can have concepts like "walkthroughs" without deciding which of three markdown files to get the AI to remember to update via aggressive system prompting.

```
                STRUCTURED DATA (TOML)
                        │
          ┌─────────────┼─────────────┐
          │             │             │
          ▼             ▼             ▼
       CLI Tool    VSCode UI    LM Tools

   All surfaces project the same source of truth.
   All edits go through tools that manipulate correctly.
   No drift between surfaces.
```

**What this enables**:

- Correct-by-construction edits (tools enforce schema)
- Projections generate readable views on demand
- Contextual steering surfaces guidance at the right moment
- No "which file should I update?" confusion

---

## 3. Anti-One-Shot: Repeatable High-Quality Work

The ecosystem rhetoric optimizes for one-shot. Exosuit is explicitly **anti-one-shot**:

| One-Shot Thinking       | Exosuit Thinking                           |
| ----------------------- | ------------------------------------------ |
| Fully specify upfront   | Discover through SOAR cycles within phases |
| Autonomous execution    | Human gates at phase boundaries            |
| Context is chat history | Context is persistent files                |
| Hope it works out       | Validation glued to git flows (Clean Pane) |
| "AI did it for me"      | "AI collaborated with me"                  |

**The goal**: Make every SOAR cycle feel as good as the first one-shot. Repeatable, not degrading.

---

## 4. The Strict Engineering Paradox

The ecosystem says "strict typing is too hard for agents." The reality is the opposite:

> The more your types capture error conditions (reliably, without massive false positives), and the better the errors explain _why_ there's an error, the more the AI stays within the four corners of high-quality code.

**Exohook** is designed around this: validation optimized for agent feedback.

**The paradox explained**:

- Strict feels harder → but errors give the AI better signal
- Linting feels pedantic → but it invokes a slice of the model that's more rigorous
- TDD feels slow → but it forces thinking before coding
- Just _thinking_ about rigorous engineering summons a better slice of the model

You're less likely to get "you told me to fix your tests so I deleted them" when the system is oriented toward rigor.

---

## 5. VSCode Native

Exosuit is built as a **native VSCode experience**, not a separate app or wrapper. This matters:

- **Where you already work**: No context switch to a different tool
- **Integrated UI**: Sidebars, webviews, status bar — all part of your editor
- **LM tools**: Agent tools that work within Copilot/Claude workflows
- **Familiar patterns**: Extensions, settings, keybindings — VSCode conventions

The UI is a first-class part of the system, not an afterthought. Each slice of work should bring the UI closer to the concepts.

---

## 6. Git Flows as the Carrier

Most AI tools treat git as an afterthought. Exosuit makes **Phase = PR**.

**Why this matters**:

- Developers already have git muscle memory
- Validation (tests, lints) attaches to PRs automatically
- Phase completion = merged PR = permanent record
- Links to other high-quality processes governed by git flows

This leans into git flows rather than treating them as annoyances.

---

## 7. Sticky Artifacts That Build Coherently

Long AI projects typically **degrade** in coherence. Exosuit is designed to **accumulate** coherence:

```
Session 1    Session 2    Session 3    Session 4
    │            │            │            │
    ▼            ▼            ▼            ▼
┌────────────────────────────────────────────────────────┐
│  AXIOMS: Stable core values, rarely changed            │
│  ──────────────────────────────────────────────────    │
│  Constrain all decisions, checked at key moments       │
└────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────┐
│  MANUAL: Grows as RFCs reach Stage 4                   │
│  ──────────────────────────────────────────────────    │
│  Compiled law — the agent's source of truth            │
└────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────┐
│  CODEBASE: Evolves through phases                      │
│  ──────────────────────────────────────────────────    │
│  Reflects the design documented in Manual              │
└────────────────────────────────────────────────────────┘

The three grow together, staying coherent.
```

RFCs are already better than ad-hoc notes — you could read them sequentially to build understanding. But the full system (RFCs → Manual → Axiom checks) is what prevents drift.

---

## 8. Contextual Steering as Thread

The operating manual isn't a static document that fades from context. It's **progressively disclosed** through steering:

```
SOAR Loop (RFC 00224)
    │
    ├── STATUS: "Where am I? What's the delta from plan?"
    │       └── Tools: exo-status, exo-phase, exo-list-tasks
    │
    ├── ORIENT: "What are my options? What should I do next?"
    │       └── Tools: exo-steering, exo-context, exo-goal-list
    │
    ├── ACT: "Execute the chosen action."
    │       └── Tools: exo-task-*, exo-tdd-*, exo-impl-*
    │
    └── REVIEW: "Did it work? What did we learn?"
            └── Tools: exo-diagnostics (RFC 00225), human judgment
```

**The vision**: A phase is describable as a **thread** — SOAR cycles within phases, phases within epochs. Not ad-hoc, but a designed flow.

---

---

## 9. The SOAR Loop (RFC 00224)

> **Added 2026-02-03**: This formalizes the steering vision into a concrete workflow model.

SOAR (Status → Orient → Act → Review) is our adaptation of Boyd's OODA loop for human-AI collaboration:

```
    ┌──────────────────────────────────────────────┐
    │                                              │
    ▼                                              │
┌────────┐    ┌────────┐    ┌─────┐    ┌────────┐  │
│ STATUS │ ─▶ │ ORIENT │ ─▶ │ ACT │ ─▶ │ REVIEW │──┘
└────────┘    └────────┘    └─────┘    └────────┘
```

| Phase      | Question                                    | Tools                         |
| ---------- | ------------------------------------------- | ----------------------------- |
| **Status** | Where am I? What's the delta from plan?     | `exo-status`, `exo-phase`     |
| **Orient** | What are my options? What should I do next? | `exo-steering`, `exo-context` |
| **Act**    | Execute the chosen action                   | `exo-task-*`, `exo-tdd-*`     |
| **Review** | Did it work? What did we learn?             | `exo-diagnostics` (RFC 00225) |

**Why SOAR, not OODA?**

- **Status** (not Observe): We check drift from _plan_, not raw environmental sensing
- **Review** (explicit): OODA leaves verification implicit; we make it a first-class phase
- **Human decides, AI acts**: The human retains decision authority at Orient→Act boundary

---

## 10. Clean Pane = Clear Mind (RFC 00225)

> **Added 2026-02-03**: A new axiom for the Review phase.

VS Code's Problems pane contains valuable validation information already configured by the user. RFC 00225 proposes integrating this into SOAR:

- **Status**: Diagnostic summary in `exo-status`
- **Orient**: Steering adjusts confidence based on error count
- **Review**: Phase transitions blocked on errors (configurable)

**The axiom**: A zero-noise Problems pane is a prerequisite for effective steering. Noise is opt-in, not opt-out.

**Differentiator**: No competitor (Cursor, Claude Code, Kiro) integrates diagnostics into workflow steering.

---

## What This Adds Up To

**"Issues are about to get rebooted."**

One lens on exosuit: it's rebooting how we think about project management for AI collaboration. Not adapting old concepts (issues, PRs, docs) but defining new workflow primitives that fit the actual collaboration pattern.

The project structure IS the product — but the point isn't "inversion." The point is:

- New workflow concepts designed for AI collaboration
- Coherent projection across all surfaces
- Repeatable high-quality work, not one-shot magic
- Strict engineering as enabler, not obstacle
- VSCode native experience
- Git flows as the carrier
- Artifacts that accumulate coherence
- Steering as designed thread, not ad-hoc reminders
- **The SOAR Loop as formalized workflow model** _(added 2026-02-03)_
- **Problems Pane integration for Review phase** _(added 2026-02-03)_

**Slogan**: A system for building high-quality software with AI.

---

## 11. The Value Curve: Ceremony Scales with Complexity

> **Objection**: "This looks like too much ceremony for my project."

The ceremony is **opt-in and progressive**. Exosuit provides value at every level of investment:

### Immediate Value (Zero Ceremony)

| Feature        | What You Get                  | Ceremony Required                       |
| -------------- | ----------------------------- | --------------------------------------- |
| **SOAR Loop**  | Structured thinking model     | None (it's a mental model)              |
| **Clean Pane** | Problems pane as quality gate | None (RFC 00225, uses existing VS Code) |
| **Steering**   | "What should I do next?"      | `exo-steering` tool call                |
| **Phases**     | Human-gated work units        | `exo phase start/finish`                |

### Growing Value (Light Ceremony)

| Feature    | What You Get        | Ceremony Required      |
| ---------- | ------------------- | ---------------------- |
| **Tasks**  | Tracked work items  | `exo task add "..."`   |
| **Ideas**  | Captured insights   | `exo idea add "..."`   |
| **Epochs** | Thematic milestones | `exo plan` CLI tooling |

### Full Value (Scaled Ceremony)

| Feature    | What You Get              | Ceremony Required              |
| ---------- | ------------------------- | ------------------------------ |
| **RFCs**   | Permanent decision record | Write markdown, promote stages |
| **Manual** | Compiled law              | Update when RFCs stabilize     |
| **Axioms** | Enforced principles       | Define and check               |

### The Key Insight

**You don't need RFCs to use Exosuit.** You need RFCs when:

- Decisions need to survive session boundaries
- Multiple people (or future-you) need to understand _why_
- The cost of reversing a decision is high

For a weekend project? SOAR + Phases + Clean Pane is plenty.
For a production system? The full RFC pipeline pays dividends.

**The structure provides value even when ceremony is minimal** — the concepts (phases, steering, clean pane) guide thinking even without the artifacts (RFCs, manual, axioms).

---

## Appendix: Exosuit vs. Beads

> **Updated 2026-02-03**: Beads has evolved significantly since October 2025. This comparison reflects the current state.

Steve Yegge's [Beads](https://github.com/steveyegge/beads) is a "coding agent memory system" that solves inter-session amnesia by giving agents a structured issue database with dependency chains. It has evolved from a simple issue tracker into a more sophisticated system with gates, workflow templates ("molecules"), and editor integrations.

### What Beads Gets Right

Yegge correctly identifies **the dementia problem**: agents lose context across sessions, create proliferating markdown plans, and can't maintain coherent long-horizon work. His solution — a Git-backed JSONL database with dependency links, queryable via `bd` CLI — gives agents external memory they can query (`bd ready`) to know what to work on next.

Key Beads insights that resonate:

- Markdown plans are "write-only memory" — agents create them but can't query them
- Dependencies need to be first-class data, not prose
- Agents need a place to file discovered work that won't be lost
- Session persistence without re-prompting is crucial

**Recent Beads evolution** (since late 2025):

- **Gates** (including human gates) — approval points in workflows
- **Molecules/Formulas** — workflow templates for structured processes
- **MCP server** — Copilot integration and editor support
- **KV store** — persistent agent state beyond issues
- **Dolt backend** — optional database backend for advanced use cases

### Where Exosuit Differs

| Dimension             | Beads                                         | Exosuit                                                    |
| --------------------- | --------------------------------------------- | ---------------------------------------------------------- |
| **Scope**             | Agent memory + workflow orchestration         | Full workflow system (planning → execution → accumulation) |
| **Unit of work**      | Issues (fine-grained, agent-sized)            | Phases (human-gated, multi-task)                           |
| **Human involvement** | Optional gates (can be fully autonomous)      | Designed-in gates — phases require human approval          |
| **Tactical loop**     | Queue-driven (`bd ready` → work → `bd close`) | SOAR Loop (Status → Orient → Act → Review)                 |
| **Planning**          | Flat issue graph with dependencies            | Nested hierarchy: Epochs → Phases → Tasks, RFC pipeline    |
| **Output**            | Issues get closed                             | Artifacts accumulate (walkthroughs, RFCs → Manual)         |
| **Quality mechanism** | Gates + agent judgment                        | TDD steering, exohook validation, Clean Pane axiom         |
| **Permanence**        | Issues (ephemeral work items)                 | RFCs become law, Manual is permanent record                |
| **IDE integration**   | MCP server + editor setup commands            | Native VS Code extension (sidebars, webviews, LM tools)    |

### The SOAR Difference

Beads organizes work around **ready queues** — agents ask "what's ready?" and work on it. This is effective but reactive.

Exosuit organizes work around the **SOAR Loop** — a formalized tactical cycle:

```
STATUS: Where am I? What's the delta from plan?
ORIENT: What are my options? What should I do next?
ACT:    Execute the chosen action
REVIEW: Did it work? What did we learn?
```

For complex tasks within Act, Exosuit uses the **PER Protocol** (Prepare → Execute → Review):

- **Prepare**: Audit the plan against reality before starting
- **Execute**: Implement according to the audited plan
- **Review**: Verify the work meets expectations

Beads has gates and molecules for workflow orchestration, but no explicit SOAR or PER-style loop model documented — it focuses on _what_ work exists rather than _how_ to think about work.

### Complementary or Competing?

They're different layers of the same problem space:

**Beads** = External memory + workflow orchestration for agent swarms
**Exosuit** = Full workflow system with designed loops, quality mechanisms, and artifact accumulation

Beads is optimized for **autonomous agent swarms** doing fine-grained work. Exosuit is optimized for **human-AI collaboration** where the human remains in the loop at phase boundaries.

You could imagine using Beads _within_ an Exosuit phase — an agent might file `bd` issues as it discovers work during task execution. But the key insight is different:

- **Beads**: Help agents remember **what** to work on
- **Exosuit**: Give agents a **how** (SOAR loop) and a **why** (axioms, RFCs)

### The Key Distinctions

**1. Remember vs. Build**

Beads solves **agent continuity** — making sure agents don't forget discovered work or lose track of multi-session projects.

Exosuit solves **project coherence** — making sure long-running projects don't degrade in quality, that designs become documented law, that phases produce lasting artifacts, and that human oversight stays integrated.

Put differently:

- Beads helps agents **remember**
- Exosuit helps agents **build something that lasts**

**2. Editor-Agnostic vs. VS Code-Native**

Beads is deliberately **editor-agnostic** — it's a CLI/MCP system that any editor can query. This makes sense for autonomous agent swarms that work independently of any particular UI.

Exosuit is deliberately **VS Code-native** — the extension provides dashboards, sidebars, and reactive UI that surface context without the agent needing to ask. The human sees what's happening in real-time.

| Beads                          | Exosuit                           |
| ------------------------------ | --------------------------------- |
| Agent invokes CLI tools        | UI surfaces context automatically |
| No visual feedback during work | Dashboard, sidebars, status bar   |
| Context via JSON queries       | Context via reactive UI state     |
| Editor is a client             | Editor is the **cockpit**         |

Put differently: Beads optimizes for agents that work independently. Exosuit optimizes for humans and agents working together in a shared cockpit.

### Why Ship Now?

Beads is getting attention because it solves a real pain point (agent memory). It's a good tactical solution that has evolved into a capable orchestration system.

But Exosuit asks a different question: not "how do we help agents remember?" but "how do we design workflows where humans and AI collaborate to build high-quality software?"

The risk of waiting: the conversation shifts to "how do we make agents more autonomous?" instead of "how do we keep humans meaningfully in the loop while leveraging AI capabilities?"

Exosuit's answer is: new workflow concepts (SOAR, PER, phases, epochs), not just better memory.
