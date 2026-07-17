<!-- exo:168 ulid:01kg5kp2kaqfv54k3n76snxszd -->

# RFC 168: Lint and Format Lane

- **Superseded by**: RFC 0081


- **Status**: Withdrawn
- **Stage**: 0
- **Reason**:

# RFC lint: Lint and Format Lane

## Summary

Define a dedicated “lane” in the workflow for linting and formatting.

## Motivation

- Reduce noisy diffs.
- Keep review focused on behavior, not formatting.

## Proposal

- When/how formatting is run.
- What is enforced in CI vs locally.
- How auto-fix is communicated to the user.

## Open questions

- Which tools are canonical (and where are they configured)?
- How do we handle multi-language repos?

## Recovery note

This RFC file was previously 0 bytes in the repo with no recoverable historical content in git. This is a placeholder skeleton to enable restoration.
