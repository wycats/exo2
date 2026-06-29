<!-- exo:10018 ulid:01kg5kp2dtvhrwd27vsvbqpkvg -->

# RFC 10018: Verified Text Surgery



# RFC 10018: Verified Text Surgery

## Summary

Define “verified text surgery”: constrained, verifiable edits to text files with clear pre/post conditions.

## Motivation

- Avoid accidental data loss in tool-mediated edits.
- Make edits auditable and deterministic.

## Proposal

- Define permitted edit primitives.
- Define verification rules (round-trip parse, invariants, etc.).
- Define failure modes and recovery behavior.

## Open questions

- What is the minimum useful set of primitives?
- How should verification be exposed in the CLI/editor?

## Recovery note

This RFC file was previously 0 bytes in the repo with no recoverable historical content in git. This is a placeholder skeleton to enable restoration.
