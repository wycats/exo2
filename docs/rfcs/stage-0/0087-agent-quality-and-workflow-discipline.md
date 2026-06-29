<!-- exo:87 ulid:01kg5kp2f9sdp63v074d90ax5z -->

# RFC 87: Agent Quality and Workflow Discipline


# RFC 0087: Agent Quality and Workflow Discipline

## Summary

This RFC proposes a comprehensive system for fostering high-quality agent behavior through **steering** rather than enforcement. It combines Just-In-Time (JIT) context injection, mission-oriented prompts, loop detection/prevention, and automated lightweight checks to create an "Organic Quality Loop" that makes good practices the path of least resistance.

## Consolidated From

This RFC consolidates three related proposals:

- **RFC 0052: The Agent Quality Loop** - Steering philosophy and JIT context
- **RFC 0025: Mashing Protection** - Loop detection and prevention
- **RFC: Lightweight Checks** - Automated verification during workflow

## Motivation

Current agent workflows suffer from multiple failure modes:

1. **Lack of Standards Awareness**: Agents implement features without checking existing standards, leading to drift.
2. **Performative Compliance**: Agents are forced to fill out checklists without understanding the underlying goals.
3. **Unproductive Loops**: Agents get stuck repeating the same failed operation, burning tokens and frustrating users.
4. **Late Feedback**: Errors are caught only at commit time (via pre-commit hooks), breaking flow.
5. **Cognitive Overload**: Agents must "remember" to run checks, competing with primary task attention.

We need a unified approach that addresses all of these through **The Paved Road**: an environment that makes high-quality work natural and effortless.

## The Steering Philosophy

We define three core axioms for Agent Steering:

### 1. The Principle of Paved Roads

**"The Tool is a Guide, not a Gatekeeper."**
We do not force the Agent to walk the path of quality; we pave that path so smoothly that walking off it feels like unnecessary effort.

- _Anti-Pattern_: Blocking the agent for missing a tag.
- _Pattern_: Suggesting relevant tags based on the current epoch.

### 2. The Principle of Contextual Resonance

**"Every Output is a Prompt."**
The CLI's stdout is the Agent's stdin. We use every command output to subtly prime the Agent's latent knowledge and persona.

- _Anti-Pattern_: `Task started.` (Zero context).
- _Pattern_: `[Mission] Task started. Remember: In this Epoch, we value "Glitch Freedom".`

### 3. The Principle of Professional Dignity

**"Optimize for Pride, not Compliance."**
We treat the Agent as a Senior Engineer. We ask for "Narratives of Victory," not "Proof of Compliance."

- _Anti-Pattern_: "Upload a screenshot to prove you did it."
- _Pattern_: "Show me what you built. If I were reviewing this PR, what would make me say 'Wow'?"

## Detailed Design

### Part 1: JIT Context & Mission Briefings

#### 1.1 The Law Library (`exo rfc`)

Enable the agent to "Consult the Oracle" effortlessly.

- **Commands**:
  - `exo rfc list --tag <tag>`: Find relevant standards.
  - `exo rfc search <query>`: Semantic/Keyword search.
  - `exo rfc show <id>`: Read the law.
- **Steering**: When an agent struggles or asks about "standards," the system prompts it to use `exo rfc`.

#### 1.2 The Mission Briefing (`exo task start`)

Transform `exo task start` into a context-injection event.

- **Logic**:
  1. Identify the Task's Epoch and Tags.
  2. Retrieve relevant "Quality Axioms" (e.g., UI → Glitch Freedom, Core → Idempotency).
  3. Output a "Mission Briefing" block that primes the agent with these values.

#### 1.3 The Narrative Walkthrough (`exo walkthrough`)

Refactor `exo walkthrough` to encourage storytelling.

- **Prompting**: Instead of generic "Description", the CLI asks specific, pride-oriented questions based on the entry type.
  - _Feature_: "What is the 'North Star' user experience of this feature?"
  - _Refactor_: "How did you leave the code better than you found it?"
  - _Fix_: "How did you prove the bug is dead?"

### Part 2: Loop Detection & Prevention (Mashing Protection)

#### 2.1 The "Mashing" Problem

Agents sometimes get stuck in loops (e.g., trying to fix a lint error, failing, trying again with the same fix). This burns tokens and frustrates users.

#### 2.2 Detection Mechanisms

- **Loop Detection**: Analyze the chat history for repeated tool calls with identical or near-identical arguments.
- **Error Counting**: If a tool fails X times in a row, stop and ask the user for help.
- **Pattern Recognition**: Detect semantic equivalence (e.g., "check code" → "verify syntax" → "lint file" as the same operation).

#### 2.3 Intervention Strategies

- **Backoff Strategy**: Force the agent to "stop and think" (read documentation, check RFCs) after a failure.
- **Context Shift**: Suggest alternative approaches (e.g., "This file edit keeps failing. Try checking the schema in docs/").
- **User Interrupt**: Provide a UI control for the user to break the loop ("Stop Mashing" button).

#### 2.4 Heuristics

- **Threshold**: 3 identical failures → soft warning, 5 → hard stop.
- **Cooldown**: After intervention, agent must perform at least 2 different operations before retrying the failed operation.

### Part 3: Lightweight Automated Checks

#### 3.1 The Late Feedback Problem

Errors caught at commit time (via pre-commit hooks) cause a "fix-commit" loop that breaks flow and increases cognitive load.

#### 3.2 The `exo check` Command

A unified command that runs relevant lightweight checks for the current context:

- **Rust**: `cargo check`, `cargo fmt --check`, `cargo clippy`
- **TypeScript**: `tsc --noEmit`, `eslint`, `prettier --check`
- **Auto-Detection**: Examines recently modified files to determine which checks to run.
- **Smart Reporting**: Reports _only_ new errors or high-priority issues.

#### 3.3 Automated "Micro-Checks" in Tooling

When the agent uses file editing tools, optionally trigger fast checks and return results with the tool output:

- **Pros**: Zero cognitive load. Immediate feedback.
- **Cons**: May slow down tool execution.
- **Configuration**: Make this opt-in via `exosuit.toml`.

#### 3.4 "Steering" Injection

The system prompt or tool output includes JIT reminders:

- **Example**: "You just edited a Rust file. Run `exo check` to verify."
- **Frequency**: Only after edits, not on every action.

### Part 4: Integration & Synergies

#### 4.1 Unified Error Handling

When a check fails, the system provides context-aware suggestions:

- "Cargo check failed. This might relate to the 'Type Safety' axiom for this epoch. Run `exo rfc show 0012` for guidance."

#### 4.2 Loop Prevention + Checks

If the agent repeatedly fails a check, the mashing protection kicks in:

- "You've run `cargo check` 3 times with the same error. Consider reading the error message more carefully or consulting `exo rfc search 'lifetimes'`."

#### 4.3 Mission Briefings + Checks

Mission briefings can include check recommendations:

- "[Tip] For UI work, run `exo check` frequently to catch visual regressions early."

## User Experience

**Starting a Task:**

```text
> exo task start "Implement Sidebar"
[System] Task "Implement Sidebar" started.
[Context] Epoch: "User Experience" | Tags: [UI, VSCode]
[Mission] In this Epoch, we value "Glitch Freedom". Ensure no stale state exists after updates.
[Tip] Run `exo rfc list --tag ui` to review UI standards.
[Tip] Run `exo check` after making changes to catch issues early.
```

**After Editing a File:**

```text
> (agent edits src/lib.rs)
[Auto-Check] Running cargo check...
[Auto-Check] ✓ No errors
```

**Loop Detection:**

```text
> (agent runs `cargo check` for the 4th time with the same error)
[System] ⚠️ You've run this check 4 times with the same error.
[Suggestion] Try reading the full error message or run `exo rfc search 'lifetimes'`.
[Action] I'm pausing automatic retries. Type 'continue' when you're ready to proceed.
```

## Open Questions

1. **Threshold Tuning**: What are the right thresholds for loop detection? (3 failures? 5?)
2. **Check Performance**: What is the latency cost of running checks on every edit?
3. **Distinguishing Loops from Refinement**: How do we tell the difference between "mashing" and legitimate iterative refinement?
4. **Context Window Budget**: How much token budget should we allocate to mission briefings and check output?

## Drawbacks

- **Complexity**: Requires chat history analysis, file watching, and coordination between multiple systems.
- **Token Cost**: Mission briefings and check output add to the context window.
- **False Positives**: Loop detection might flag legitimate retry patterns.

## Alternatives

- **Prompt-Only**: Just tell the agent "please run checks" in the system prompt. (Proven unreliable).
- **Strict Gates**: Block commits until all checks pass. (Rejected: causes friction and performative compliance).
- **Manual Checks Only**: Rely on the user to run checks. (Current state: causes late feedback).

## Implementation Phases

1. **Phase A**: Implement `exo check` command with auto-detection.
2. **Phase B**: Add mission briefings to `exo task start`.
3. **Phase C**: Implement basic loop detection (error counting).
4. **Phase D**: Add steering injection after file edits.
5. **Phase E**: Integrate loop detection with context-aware suggestions.
6. **Phase F**: Add opt-in micro-checks in editing tools.

