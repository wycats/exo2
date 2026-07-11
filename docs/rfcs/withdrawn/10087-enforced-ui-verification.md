<!-- exo:10087 ulid:01kmzxeffc30cshswevk02cr8g -->


# RFC 10087: Enforced UI Verification

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**: Withdrawn by RFC 10180 storage disposition: this proposal depends on retired file-backed phase context or direct editing/protection of legacy docs/agent-context current artifacts.

## Summary

Add a mandatory `[verification]` section to the phase execution artifact
(`docs/agent-context/current/implementation-plan.toml`) and enforce its presence
via the `exo` CLI before a phase can be finished.

## Motivation

Agents (and humans) often skip the "Verification" step or do it superficially.
We need to make verification a "hard gate" in the process.

As per Axiom 19, no UI component is done without a test. The tooling should
enforce this.

## Design

### 1. Schema Change (`implementation-plan.toml`)

The phase execution artifact will require a new top-level table:

```toml
[verification]
automated = ["Run scripts/verify-phase.sh"]
manual = ["Describe any manual verification steps"]
```

### 2. Tooling Change (`exo`)

The `exo phase finish` command will be updated to:

1.  Parse `docs/agent-context/current/implementation-plan.toml`.
2.  Check for the `[verification]` table.
3.  If missing, abort with an error.
4.  If both `automated` and `manual` are empty, abort with an error.

### 3. Workflow

1.  Agent works on phase.
2.  Agent runs `exo phase finish`.
3.  `exo` complains: "Verification missing. Please update implementation-plan.toml."
4.  Agent adds test(s), updates `implementation-plan.toml`.
5.  `exo phase finish` succeeds.
