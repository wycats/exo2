<!-- exo:10077 ulid:01kmzxefe86xbx9514qfs9hr6s -->


# RFC 10077: E2E Holodeck

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**: This later reconstruction duplicate revives the previously withdrawn placeholder RFC 0078. Current end-to-end behavior is documented by the maintained test harness and research evidence rather than this placeholder.

- **Superseded by**: RFC 0078


## Summary

Define an end-to-end (“holodeck”) testing approach for Exosuit workflows.

## Motivation

- Reduce regressions across CLI/editor integration.
- Make complex workflows reproducible.
- Provide confidence for refactors.

## Proposal

- Define the scope of “E2E” for this project.
- Specify fixtures, harnesses, and what must be deterministic.
- Document how failures are diagnosed and reported.

## Open questions

- What is the minimal supported host environment?
- Which workflows are required as E2E smoke tests?

## Recovery note

This RFC file was previously 0 bytes in the repo with no recoverable historical content in git. This is a placeholder skeleton to enable restoration.
