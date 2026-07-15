<!-- exo:51 ulid:01kg5kp2dfzt9pbhz55y0ka8dp -->

# RFC 51: Coherence Bootstrap

- **Supersedes**: RFC 10068



# RFC 0051: Coherence Bootstrap

## Summary

This RFC proposes restructuring the project's "Coherence" prompts into a modular **Coherence Matrix**. This system decomposes "coherence" into four orthogonal axes—Time, Truth, Logic, and Intent—and provides specialized prompts to diagnose and repair fractures in each dimension.

Crucially, this RFC also establishes a **Global Bootstrap** pattern, allowing these prompts to be installed and used across multiple projects via the `exo` CLI.

## Motivation

The current `rebuild-coherence` prompt is a monolithic tool that attempts to fix everything at once. This leads to:
1.  **Cognitive Overload**: The agent tries to fix docs, code, and plan simultaneously, often doing a mediocre job at all three.
2.  **Missed Fractures**: Subtle issues like "Axiomatic Hypocrisy" (code that works but violates principles) are ignored in favor of fixing obvious syntax errors.
3.  **Lack of Portability**: The prompts are hardcoded to this repository's structure.

## The Coherence Matrix

We define "Coherence" as the alignment of four vectors:

| Axis | Type | Definition | Failure Mode | Target Artifacts | Grimoire Mapping |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Time** | **Temporal Coherence** | Synchronization of State | **Lag / Hallucination** | `plan.toml` vs `manual/` vs `src` | `var`, `sci` |
| **Truth** | **Axiomatic Coherence** | Adherence to Physics | **Hypocrisy** | `axioms.*.toml` vs `src` | `ax`, `native`, `novel` |
| **Logic** | **Internal Coherence** | Logical Consistency | **Contradiction** | `plan.toml` vs `docs/rfcs/*` | `inv`, `fuzz` |
| **Intent** | **Alignment Coherence** | Fidelity to Goal | **Drift** | `AGENTS.md` vs Output | `razor`, `prop` |

## Implementation Plan

### 1. Prompt Architecture
We will create a `src/prompts/coherence/` directory containing:

*   **`coherence.prompt.md` (The Dispatcher)**: Analyzes the user's request/anxiety and dispatches the correct sub-routine.
*   **`check-temporal.prompt.md`**: Synchronizes Plan, Manual, and Code.
*   **`check-axiomatic.prompt.md`**: Refactors code to align with `axioms.*.toml`.
*   **`check-internal.prompt.md`**: Verifies logical consistency and links.
*   **`check-intent.prompt.md`**: Aligns trajectory with `AGENTS.md`.

### 2. Global Bootstrap Strategy
To satisfy the requirement for "Global Installation":

1.  **Packaging**: These prompts will be treated as a "Standard Library" for Exosuit agents.
2.  **Installation**: We will add a mechanism to `exo` (e.g., `exo init --with-prompts`) or a standalone script to install these prompts into a global user directory (e.g., `~/.config/Code/User/prompts/exo/`).
3.  **Usage**: The `cast.prompt.md` or user settings will reference these global paths, allowing any project to inherit the "Coherence Matrix" capability.

## Note to Exo Agent

> **Proposal to the Agent System:**
>
> We are upgrading the `rebuild-coherence` capability. The old monolithic prompt is deprecated.
>
> **New Protocol:**
> When asked to "fix coherence" or "check status", do not just read the plan. You must now:
> 1.  **Diagnose**: Is this a Time, Truth, Logic, or Intent problem?
> 2.  **Dispatch**: Load the specific `check-*.prompt.md`.
> 3.  **Verify**: Ensure the fix addresses the specific fracture without causing regression in other axes.
>
> This change requires you to be more *diagnostic* before being *operative*.

## Grimoire Integration

This plan is fully aligned with the **Cognitive Grimoire**:
*   It operationalizes **System Architecture** spells (`ax`, `var`) into the **Axiomatic** and **Temporal** checks.
*   It operationalizes **Problem Solving** spells (`sci`, `inv`) into the **Internal** check.
*   It operationalizes **Meta-Cognition** spells (`razor`, `prop`) into the **Intent** check.
*   The **Dispatcher** acts as an automated **Auto-Caster** (`auto`).

## Constraints & Questions

*   **Constraint**: Must work with existing `exo` CLI.
*   **Constraint**: Must be portable (no hardcoded paths to `exosuit` repo in the prompts themselves).
*   **Question**: Should we bundle `cast.prompt.md` itself into this global package?
