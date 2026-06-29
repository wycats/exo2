<!-- exo:10118 ulid:01kmzxey237gbztsjefe9khywb -->


# RFC 10118: CLI AST Tool Schema

## Summary

Define a schema/AST for CLI commands so tools can reliably parse, validate, and display invocations.

## Motivation

- Avoid shell-string parsing ambiguity.
- Enable safe automation and tooling.

## Proposal

- AST representation for commands/args.
- Validation rules and diagnostics.
- Backward compatibility and migration strategy.

## Open questions

- How do we represent redirections/pipes (if at all)?
- What guarantees exist across versions?

## Recovery note

This RFC file was previously 0 bytes in the repo with no recoverable historical content in git. This is a placeholder skeleton to enable restoration.
