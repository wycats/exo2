<!-- exo:166 ulid:01kg5m2yh4gwedbxbm6aw9a0je -->

# RFC 166: Enforced UI Verification



# RFC 0166: Enforced UI Verification

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