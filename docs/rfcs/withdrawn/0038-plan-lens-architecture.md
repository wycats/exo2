<!-- exo:38 ulid:01kg5kp2ctkc5r49sz5tqeyfz5 -->

# RFC 38: Plan Lens Architecture



# RFC 0038: Plan Lens Architecture

## Summary

Describe the architecture for a “plan lens”: a view/projection over plan state optimized for specific workflows.

## Motivation

- Plans need multiple coherent views (human review, machine status, UI, etc.).
- Projections reduce duplication and drift.

## Proposal

- Define what a “lens” is (inputs, outputs, invariants).
- Identify canonical sources of truth vs derived data.
- Define how lenses are computed and validated.

## Open questions

- Where are lenses implemented (CLI, library, editor)?
- How are lenses versioned and tested?

## Recovery note

This RFC file was previously 0 bytes in the repo with no recoverable historical content in git. This is a placeholder skeleton to enable restoration.

