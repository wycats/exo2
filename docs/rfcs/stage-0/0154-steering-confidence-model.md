<!-- exo:154 ulid:01kg5kp2jnh9pypq7b02pvsnhd -->

# RFC 154: Steering Confidence Model


# RFC 0154: Steering Confidence Model

## Summary

Document the current steering confidence computation mechanics and propose improvements to make confidence semantically meaningful, context-aware, and actionable.

## Motivation

Steering suggestions are surfaced with confidence values to guide action selection. The current system uses confidence as a **ranking metric**—higher is better, but the values themselves lack semantic meaning.

A first-principles analysis revealed several gaps:

1. **No task-completion awareness**: "Finish phase" doesn't get boosted when all tasks are complete
2. **No user-approval gating**: Transitions aren't penalized when sign-off is missing
3. **No verification gating**: Check pass/fail doesn't directly affect transition confidence
4. **Ambiguous rankings**: In Transitioning mode, "Finish phase" (0.48) barely beats "Add step" (0.45)

This RFC documents the current system and proposes a redesign where confidence represents **decision quality**, not just relative ranking.

## Detailed Design

### Terminology

- **Suggested action**: A `SuggestedAction` containing `label`, `command`, `rationale`, `intent`, and optional `confidence`.
- **Base confidence**: The initial `confidence` assigned when a suggestion is created.
- **Adjusted confidence**: The base confidence scaled by a `ProgressMode`-specific multiplier and clamped to $[0,1]$.
- **Work intent**: The categorical purpose of an action (`WorkIntent`).
- **Progress mode**: A workflow state (`ProgressMode`) used to bias intents.

### User Experience (UX)

Confidence values are shown alongside suggested actions. They are not directly configurable by the user. The effect is to rank or prioritize actions appropriate to the current workflow state.

### Architecture

Steering suggestions are constructed in the steering module, and the world-derived progress mode is applied as a confidence multiplier before returning the final steering block.

### Implementation Details

#### Work intents

`WorkIntent` categories:

- Orient
- Plan
- Execute
- Record
- Verify
- Ship

#### Progress modes and activation rules

`ProgressMode` variants:

- RoadmapRevision
- BetweenEpochs
- BetweenPhases
- Planning
- Executing
- Verifying
- Transitioning

Activation rules (derived from world state):

- Verifying: any step is red.
- BetweenEpochs: no active phase and either no epochs, all epochs complete, or current epoch complete.
- BetweenPhases: no active phase, active epoch exists and is not complete (default between-state).
- Planning: active phase exists and no tasks/steps defined.
- Executing: active phase and any pending or in-progress tasks/steps.
- Transitioning: active phase and all tasks/steps completed (or none exist).
- RoadmapRevision: present in the enum but not currently derived in progress mode logic.

#### Base confidence values

Base confidence is set when actions are created. The current sources and values are:

**Upgrade required steering**
- Run upgrade migrations: 1.0 (blocking).

**No active phase steering**

BetweenEpochs:
- Review roadmap: 0.85
- Start next epoch's first phase (if available): 0.75
- Review completed epoch (if any unreviewed): 0.8

BetweenPhases:
- Start next phase (if available): 0.85
- Review phase options: 0.7
- Check status (repair action): 0.5

Fallback (unexpected mode for this path):
- Start a phase (if available): 0.8
- Show plan: 0.7

**Phase steering**
- Confirm tests now pass (if any red): 0.8
- Start TDD cycle for next step (if pending step): 0.85
- Complete a task (if pending tasks): 0.6
- Add an implementation step: 0.5
- Finish the phase: 0.4
- Re-orient (repair action): 0.4

**World repairs (git dirty)**
- Check phase status: 0.95
- Inspect working tree: 0.9
- Review diff: 0.8
- Recommend commit + open PR: conditional
  - 0.85 if phase suggests active work and changes touch non-generated files
  - 0.55 if only generated-ish files
  - 0.7 otherwise

**Missing snapshots**
- Initialize implementation plan snapshot: 0.8
- Restore missing snapshot: 0.8

**Plan health repairs**
- Review plan health: 0.8 if Critical, 0.6 if Degraded
- Suppressed in between-states and transitioning

**Epoch review suggestions**
- Review completed epoch: 0.7 (advisory)

#### Progress mode multiplier table

`ProgressMode::confidence_multiplier_for_intent(intent)` returns:

| ProgressMode     | Orient | Plan | Execute | Record | Verify | Ship  | Other |
|------------------|--------|------|---------|--------|--------|-------|-------|
| RoadmapRevision  | 1.15   | 1.10 | 0.80    | 0.80   | 0.80   | 0.80  | 0.80  |
| BetweenEpochs    | 1.15   | 1.05 | 0.85    | 1.00   | 0.85   | 0.85  | 0.85  |
| BetweenPhases    | 1.10   | 1.10 | 0.85    | 0.85   | 0.85   | 0.85  | 0.85  |
| Planning         | 1.05   | 1.20 | 0.85    | 0.85   | 0.85   | 0.85  | 0.85  |
| Executing        | 0.95   | 0.95 | 1.15    | 1.10   | 0.95   | 0.95  | 0.95  |
| Verifying        | 0.90   | 0.90 | 1.05    | 0.90   | 1.20   | 0.90  | 0.90  |
| Transitioning    | 0.90   | 0.90 | 0.90    | 1.10   | 0.90   | 1.20  | 0.90  |

#### Adjusted confidence computation

For each action with a base confidence:

1. Determine the current `ProgressMode`.
2. Compute the multiplier for `action.intent`.
3. Set:

$$
\text{adjusted} = \mathrm{clamp}(\text{base} \times \text{multiplier}, 0.0, 1.0)
$$

Mode adjustments are applied to both `next_actions` and `repair_actions`.

## Implementation Plan (Stage 2)

- [ ] N/A (Stage 0)

## Context Updates (Stage 3)

- [ ] N/A (Stage 0)

## Drawbacks

- Hard-coded confidence values require code changes to evolve.
- Multipliers may obscure raw intent if base values are poorly tuned.

## Alternatives

- Learn multipliers dynamically from usage telemetry.
- Expose confidence scaling in configuration.

## Unresolved Questions

- Should `RoadmapRevision` be derivable from world state?
- Should confidence be decayed for repeated suggestions?

## Proposed Improvements

### Reframing Confidence as Decision Quality

The current system treats confidence as a **ranking metric**. We propose treating it as **decision quality**—the system should know when it's sure vs. uncertain, and behave differently in each case.

#### Design Goals

1. **Aim for a single high-confidence result**: The system should converge on one clear recommendation when evidence supports it
2. **Define semantic thresholds**: Confidence bands with meaning, not arbitrary numbers
3. **Surface uncertainty**: Multiple similar-confidence answers indicate genuine ambiguity worth showing to the user
4. **Progressive refinement**: Allow requesting higher-fidelity information, but only when the cost is justified

#### Confidence Thresholds

| Band | Range | Meaning | Agent Behavior |
|------|-------|---------|----------------|
| **Certain** | 0.90–1.00 | Strong evidence, single clear action | Auto-suggest or auto-execute |
| **Confident** | 0.75–0.89 | Good evidence, likely correct | Recommend with rationale |
| **Likely** | 0.60–0.74 | Reasonable guess, some uncertainty | Suggest with caveats |
| **Uncertain** | 0.40–0.59 | Multiple viable options | Present choices to user |
| **Low** | 0.20–0.39 | Weak signal, needs more context | Gather more information first |
| **Unknown** | 0.00–0.19 | Insufficient evidence | Ask user for guidance |

#### Handling Multiple Results

When the top N actions have similar confidence (within 0.10 of each other), this indicates **genuine ambiguity**:

- **Gap ≥ 0.15**: Clear winner, surface top action
- **Gap 0.05–0.14**: Slight preference, surface top 2 with rationale
- **Gap < 0.05**: Ambiguous, present as choices

#### Progressive Refinement

Agents may request higher-fidelity information to resolve uncertainty:

**When to request more context:**
- Top action is in "Uncertain" band (0.40–0.59)
- Multiple actions within 0.10 of each other
- Context freshness is stale (no recent status read)

**When NOT to request:**
- Top action is "Confident" or higher (≥0.75)
- Clear gap (≥0.15) between top action and next
- Already have fresh context

**Cost model**: Each refinement request has a token/latency cost. The system should only refine when:

$$
\text{expected\_value\_of\_refinement} > \text{cost\_of\_refinement}
$$

Where expected value is proportional to the confidence gap that refinement might create.

### Context-Aware Adjustments

Based on the first-principles analysis, add these evidence-based adjustments:

#### Task Completion Signal

When all tasks/steps are complete:
- Transition actions (Ship intent): **+0.20 additive**
- Execution actions (Execute intent): **−0.15 additive**

When tasks remain incomplete:
- Transition actions: **−0.20 additive**

#### Verification Signal

When checks are passing:
- Transition actions: **×1.20 multiplier**

When checks are failing:
- Transition actions: **×0.40 multiplier**
- Verification actions: **×1.30 multiplier**

#### User Approval Gate

When user sign-off is required but missing:
- Transition actions: **×0.50 multiplier**

When user has explicitly approved:
- Transition actions: **+0.15 additive**

### Adjusted Confidence Formula

The improved formula would be:

$$
\text{adjusted} = \mathrm{clamp}\left((\text{base} + \text{additives}) \times \prod \text{multipliers}, 0.0, 1.0\right)
$$

Where:
- **base**: Static base confidence for the action type
- **additives**: Context-aware bonuses/penalties (task completion, approval)
- **multipliers**: ProgressMode multiplier × verification multiplier × approval multiplier

### Example Scenarios

**Scenario A: All tasks complete, checks passing, in Transitioning mode**
- "Finish phase": base 0.4 + 0.20 (tasks complete) = 0.60 × 1.20 (Ship) × 1.20 (checks pass) = **0.86** (Confident)
- "Add step": base 0.5 − 0.15 (tasks complete) = 0.35 × 0.90 (Execute suppressed) = **0.32** (Low)
- **Result**: Clear winner, auto-suggest "Finish phase"

**Scenario B: Tasks remaining, in Executing mode**
- "Complete task": base 0.6 × 1.15 (Execute) = **0.69** (Likely)
- "Finish phase": base 0.4 − 0.20 (tasks remain) = 0.20 × 0.90 (Ship suppressed) = **0.18** (Unknown)
- **Result**: Clear winner, suggest "Complete task"

**Scenario C: Tasks complete, checks failing**
- "Finish phase": base 0.4 + 0.20 = 0.60 × 1.20 (Ship) × 0.40 (checks fail) = **0.29** (Low)
- "Confirm tests pass": base 0.8 × 1.20 (Verify) × 1.30 (checks fail boost) = **1.00** (Certain)
- **Result**: Clear winner, prioritize verification

## Future Possibilities

- Introduce a confidence policy layer decoupled from steering construction.
- Add per-project overrides for base confidence tables.
- Telemetry-driven tuning of base values and thresholds.
- Expose confidence bands in UI with semantic labels ("Confident", "Uncertain", etc.).

