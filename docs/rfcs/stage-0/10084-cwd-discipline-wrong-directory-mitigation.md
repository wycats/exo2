<!-- exo:10084 ulid:01kmzxeffvrq5z532ycq580567 -->


# RFC 10084: CWD Discipline & Wrong Directory Mitigation

## Summary

Investigate mechanisms to prevent the agent from executing commands in the wrong directory, a common source of errors ("whoops, wrong directory").

## Motivation

Agents often lose track of their Current Working Directory (CWD) or assume a specific CWD, leading to failed commands (e.g., running `npm install` in the root of a monorepo instead of a package).

## Detailed Design

_Ideas:_

- **Explicit CWD Requirement**: Require every `run_in_terminal` call to explicitly state the intended CWD?
- **Smart CWD Context**: The agent should always "know" where it is.
- **Directory Guardrails**: A tool wrapper that checks if `package.json` exists before running `npm`.
- **Prompt Engineering**: Stronger system prompt instructions about CWD.

## Unresolved Questions

- How to enforce this without making the agent too verbose?
- Can we automate the "cd" part?
