<!-- exo:129 ulid:01kg5kp2he8ff3z61273qfewze -->

# RFC 129: Configurable TDD Runners


# RFC 0129: Configurable TDD Runners

---
title: Configurable TDD Runners
feature: TDD
---


# RFC 0052: Configurable TDD Runners

## Summary

Make TDD/test runners configurable so `exo tdd` can support multiple ecosystems.

## Motivation

- Different repos use different test commands.
- TDD workflows should be consistent even when tooling differs.

## Proposal

- Configuration format and precedence.
- How runners are discovered and invoked.
- How failures are parsed and presented.

## Open questions

- Do we support multiple runners per repo?
- How do we standardize “red/green” detection?

## Recovery note

This RFC file was previously 0 bytes in the repo with no recoverable historical content in git. This is a placeholder skeleton to enable restoration.

