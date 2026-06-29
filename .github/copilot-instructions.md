# Critical Axiom: Green-to-Green

**Every change goes from a green test suite to a green test suite. There is no such thing as "pre-existing failures."**

Before starting work, the test suite must be green. If it isn't, fixing it is the first task — not something to hand-wave. After finishing work, the test suite must be green. Any test that fails is a regression caused by this session, regardless of when the test was written.

**Rule**: If a test fails before you start, fix it or explicitly flag it as a blocker before proceeding. Never dismiss a failing test as "pre-existing" or "unrelated."

**Corollary**: If a test is flaky or environment-dependent (e.g., daemon-mode tests that don't work in CI-like conditions), fix the test to be robust — don't skip it and move on.

---

# Critical Axiom: Managed Directories are Databases

**Files in managed directories (`docs/rfcs/`, `docs/agent-context/`) have schemas, IDs, and relationships.**

Direct file creation bypasses validation, ID generation, and cross-reference updates.

**Rule**: Always use CLI commands to create files in managed directories:

- `exo rfc create` for RFCs
- `exo idea add` for ideas
- `exo axiom add` for axioms
- `exo inbox add` for inbox items

**Never** use `create_file` or manual file creation for these directories.

---

# Critical Axiom: No Hedging

**State things definitively. Do not use "probably", "likely", "might", or "acceptable for now" when the answer is known.**

If something is wrong, say it is wrong. If something should be removed, say it should be removed. If a design direction has been decided, do not reopen it with qualifying language. Hedging erodes alignment and lets residue survive.

**Corollary**: If you genuinely do not know the answer, say so directly — that is not hedging, that is honesty. The problem is false uncertainty about things that are already decided.

---

# Critical Axiom: Ground Priorities in Observed Friction

**Agent-synthesized priorities may diverge from the user's actual friction points.**

When the user provides a friction map (whiteboard, list, priorities), treat their emphasis markers as ground truth for prioritization. Do not re-synthesize priorities from the items alone — the user's weighting carries information the items don't.

**Origin**: During the Whiteboard Spike (RFC 00235/00238), two sessions were spent designing symbol registration — which wasn't even on the whiteboard — while ignoring the items the user had visually emphasized.

---

# Critical Axiom: JIT RFC Coherence

**RFC consolidation happens just-in-time, scoped to the topic being worked on — not as a separate bulk activity.**

The goal is a parsimonious, coherent collection where each RFC reflects either implemented reality or a clear plan. Tension, duplication, or overlap between RFCs should map onto implementation cleanup work to be done at the same time. An RFC collection that has been consolidated but whose code hasn't been aligned is worse than the original mess — it creates a false sense of order.

**Rule**: When starting work on a topic, consolidate the RFCs touching that topic first:

1. Resolve duplicates (keep the higher-stage version)
2. Update RFCs to reflect what was actually built
3. Surface tensions that imply code changes
4. Ensure the surviving RFCs tell a coherent story

Any RFC tension that can't be resolved on paper should become a task.

**Anti-pattern**: Delegating RFC consolidation to a mechanical process that doesn't understand the design intent. Consolidation requires judgment — ambiguities are the point, not an obstacle.

---

# Context Management Strategy

Treat the Main Chat context window as a scarce resource to be conserved. Apply the following heuristics to determine whether to execute a task directly or delegate it to a subagent, and how to transfer information effectively.

### Decision Heuristics

1. **Prioritize Isolation (Subagents)**
   Delegate tasks to subagents whenever the work is **modular**—meaning it can be fully defined by a standalone prompt. This "fire-and-forget" approach prevents the Main Chat from being polluted with intermediate steps, verbose output, or temporary data processing.
2. **Prioritize Continuity (Main Agent)**
   Execute tasks directly only when they possess **high contextual entanglement**—where the task relies so heavily on the specific history, nuance, or "state" of the current conversation that accurately serializing that context into a fresh prompt would be inefficient or lossy.
3. **Prefer "Pass by Reference"**
   When delegating, avoid "Passing by Value" (reading file contents into the chat to paste into a subagent prompt). Instead, "Pass by Reference" by directing the subagent to specific file paths or directories. If the data exists only in memory or a command output, write it to a temporary artifact (e.g., `.context/temp_diff.txt`) and point the subagent to that artifact.

### Context Strategy Examples

#### Example 1: High Modularity (Delegate)

**User Request:** "I need a set of unit tests for the `DateFormatter` utility we just pasted. Cover all edge cases for leap years and timezones."
**Model Reasoning:**

- **Input Context:** The target code (`DateFormatter`) is distinct and currently available.
- **Context Entanglement:** Low. The requirements are standard ("cover edge cases") and do not rely on a complex history of user preferences.
- **Token Economics:** Writing comprehensive tests is verbose. Doing this in the main chat would push valuable history out of the context window.
  **Decision:** Delegate to Subagent.
  **Instruction:** "Generate a comprehensive test suite for the provided `DateFormatter` code, ensuring coverage for leap years and timezone edge cases."

#### Example 2: High Entanglement (Execute Directly)

**User Request:** "That looks better, but it contradicts the 'Sentinels' pattern we discussed at the start of the session. Refactor the `User` struct to align with that instead."
**Model Reasoning:**

- **Input Context:** "Sentinels pattern" refers to a specific, agreed-upon definition from earlier in the chat, likely containing nuance not found in public documentation.
- **Context Entanglement:** High. To delegate this, I would have to synthesize and explain the user's specific definition to the subagent. The cost of accurately serializing this context approaches the cost of just doing the work.
- **Risk:** A subagent might hallucinate a standard definition of "Sentinels" rather than the user's specific version.
  **Decision:** Execute by Main Agent.

#### Example 3: Pass-by-Reference via File System (Delegate)

**User Request:** "Read through the entire `legacy_migration/` folder and identify any deprecated API calls in the Python files."
**Model Reasoning:**

- **Input Context:** The user is pointing to a large directory on disk.
- **Context Entanglement:** Low. The definition of "deprecated API calls" is standard or can be easily defined in a prompt.
- **Token Economics:** Reading all those files into the Main Chat to then pass them to a subagent would be disastrously inefficient and likely hit token limits.
- **Strategy:** The subagent has file access tools. I should pass the _path_, not the _content_.
  **Decision:** Delegate to Subagent.
  **Instruction:** "Scan the `legacy_migration/` directory. Identify all Python files containing calls to APIs listed in `docs/deprecations.md` or matching the regex `v1_.*`. Output a list of affected files."

#### Example 4: Intermediate Artifact Creation (Delegate)

**User Request:** "Compare the output of the current build against the logs from the last successful deployment and tell me what changed."
**Model Reasoning:**

- **Input Context:** The necessary information exists in the environment but requires retrieval.
- **Context Entanglement:** Medium. The comparison logic is simple, but the data volume is high.
- **Token Economics:** Streaming two massive log files into the Main Chat just to ask a subagent to diff them is wasteful.
- **Strategy:** I will use the shell to create a "context artifact"—a diff file—and then ask the subagent to analyze _that_ specific artifact.
  **Pre-computation:** Run `diff logs/deploy_success.txt logs/current_build.txt > .context/diff_summary.txt`
  **Decision:** Delegate to Subagent.
  **Instruction:** "I have generated a diff of the build logs at `.context/diff_summary.txt`. Analyze this file and summarize the regression failures."

### Protocol: Prepare → Execute → Review (PER)

A three-phase micro-loop for implementing discrete units of work (features, migrations, fixes).

This approach helps conserve context by isolating each phase in its own subagent, ensuring that the Main Chat remains focused and uncluttered, but still allowing for detailed work to be done in a controlled manner that the main agent can oversee and understand.

**Keyword**: `per` — User can invoke with "do a PER cycle on X" or "prepare→execute→review for X".

#### 1. Prepare (Audit)

- **Agent**: `prepare` subagent (or manual audit)
- **Input**: RFC, plan, or task description
- **Output**: Readiness report with:
  - ✅ Verified assumptions
  - ⚠️ Corrections needed
  - 🔴 Blockers (must resolve before execute)
  - 📋 Implementation order
- **Gate**: User approves or requests fixes

#### 2. Execute (Implement)

- **Agent**: `execute` subagent (strongly preferred)
- **Input**: Approved prepare report
- **Output**: Working code + tests
- **Constraint**: Follow prepare report exactly; no scope creep
- **Gate**: Code compiles, tests pass

**Agent preference**: Use subagents unless the communication overhead (in tokens) clearly exceeds doing it directly. Subagents provide isolation, focus, and auditability.

#### 3. Review (Verify)

- **Agent**: `review` subagent (or manual review)
- **Input**: Executed changes
- **Output**: Review report with:
  - ✅ Correct implementations
  - ⚠️ Issues found
  - 💡 Suggestions
- **Gate**: User accepts or requests fixes

**When to use PER**:

- Implementing RFC phases
- Migrating existing code to new patterns
- Any change with moderate complexity or risk

**When NOT to use PER**:

- Trivial fixes (single-line changes)
- Exploratory work (use recon instead)
- Pure research (no code output)

---

# The SOAR Loop

> **See**: [RFC 00224: The SOAR Loop](../docs/rfcs/stage-1/00224-the-soar-loop-a-workflow-model-for-human-ai-collaboration.md) for the full specification.

Every productive session follows the **SOAR** cycle:

```
    ┌──────────────────────────────────────────────┐
    │                                              │
    ▼                                              │
┌────────┐    ┌────────┐    ┌─────┐    ┌────────┐  │
│ STATUS │ ─▶ │ ORIENT │ ─▶ │ ACT │ ─▶ │ REVIEW │──┘
└────────┘    └────────┘    └─────┘    └────────┘
```

| Phase      | Question                                    | Tools                                                 |
| ---------- | ------------------------------------------- | ----------------------------------------------------- |
| **Status** | Where am I? What's the delta from plan?     | `exo-status`, `exo-phase`, `exo-run("task list")`     |
| **Orient** | What are my options? What should I do next? | `exo-steering`, `exo-context`, `exo-run("goal list")` |
| **Act**    | Execute the chosen action                   | `exo-run("task ...")`, code edits                     |
| **Review** | Did it work? What did we learn?             | Verification, `exo-run("verify")`, human judgment     |

**Plan** tools (`exo-run("rfc ...")`, `exo-run("epoch ...")`, `exo-run("plan ...")`) operate _across_ cycles—they're strategic, not tactical.

### SOAR vs. OODA

SOAR is inspired by Boyd's OODA loop but adapted for human-AI collaboration:

- **Status** (not "Observe"): We're checking drift from _plan_, not raw environmental sensing
- **Orient**: Unchanged—synthesize context, update mental model, generate options
- **Act** (not "Decide→Act"): The human decides (explicitly or by delegation); the AI acts
- **Review**: Explicit verification phase (OODA leaves this implicit in the next Observe)

### Tool Groups

Tools are organized into four groups matching SOAR + Plan:

| Group      | SOAR Phase   | Purpose                               |
| ---------- | ------------ | ------------------------------------- |
| **Status** | Status       | Detect current state, drift from plan |
| **Orient** | Orient       | Synthesize options, steering          |
| **Act**    | Act          | Execute tasks, implementation         |
| **Plan**   | (orthogonal) | Strategic: RFCs, epochs, roadmap      |

### The Loop in Practice

1. **Start of session**: Status → Orient (where am I? what's next?)
2. **During work**: Act → Review → Status (tight loop)
3. **At decision points**: Orient (what are my options?)
4. **Strategic moments**: Plan tools (RFC work, epoch transitions)

### SOAR → GitHub Alignment

The SOAR loop maps onto GitHub artifacts:

| SOAR Phase    | GitHub Artifact                     |
| ------------- | ----------------------------------- |
| Status        | Branch state, PR status, CI results |
| Orient        | PR description, review comments     |
| Act           | Commits, code changes               |
| Review        | PR review, CI verification          |
| (Loop closes) | PR merge → new Status on main       |

A complete SOAR cycle often culminates in a **PR**. The PR is a Review artifact—it captures what was done and invites verification. Merging closes the loop.

---

# Technique: Compiler-Driven Dead Code Detection

**Use `pub(crate)` module visibility + the `unreachable_pub` lint to find dead code.**

Change `pub mod X` to `pub(crate) mod X` in `lib.rs`. The compiler will flag every `pub fn` with zero callers within the crate. Delete the flagged items, run `cargo fix` for unused imports, repeat until clean. This is more reliable than manual grep-based auditing.

The `unreachable_pub` lint is already active in `tools/exo/src/lib.rs` (`#![warn(unreachable_pub)]`).

---

# Tool Usage: Prefer exo-run

**Use the `exo-run` LM tool instead of shelling out to the terminal for exo commands.**

The `exo-run` tool routes through the machine channel, which is faster and provides structured output. Only fall back to `run_in_terminal` when `exo-run` can't handle the quoting (e.g., complex `--description` arguments with special characters) or when you need to run non-exo commands.
