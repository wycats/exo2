<!-- exo:10072 ulid:01kmzxefehtwnamzvvg9sma7y5 -->


# RFC 10072: exo map Command

## Summary

Specify `exo map` as the primary steering command for “what should I do next?”.

## Motivation

- Provide a single entry point for discovery.
- Make repo/context health visible before taking actions.

## Proposed behavior

- `exo map`: show active phase tasks/steps and suggested actions.
- `exo map --next`: emit a single best action.
- `exo map --why <command>`: explain preconditions and effects.

## Open questions

- What contract should `--format json` guarantee?
- Should `exo map` ever mutate state?

## Recovery note

This RFC file was previously 0 bytes in the repo with no recoverable historical content in git. This is a placeholder skeleton to enable restoration.
