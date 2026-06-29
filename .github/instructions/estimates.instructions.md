---
applyTo: "**/docs/agent-context/**/*.toml, **/docs/rfcs/**/*.md, **/implementation-plan.toml"
---

# Effort & Sequencing

## Estimating Effort

Use **relative sizing** to communicate effort:

- **S** (Small): Trivial change, single location
- **M** (Medium): Moderate complexity, few surfaces
- **L** (Large): Significant work, multiple components
- **XL** (Extra Large): Major undertaking, cross-cutting

## Planning Multiple Tasks

When a plan has more than a few tasks, analyze **dependencies**:

- What blocks what?
- What can run in parallel?
- Where is the critical path?

These two dimensions—_size_ and _sequence_—are sufficient for actionable planning.

## Why This Framing

Temporal estimates (hours/days/weeks) tend to drift under revision and consume attention better spent on structural relationships. Relative sizing stays stable across edits; dependency analysis reveals the critical path.
