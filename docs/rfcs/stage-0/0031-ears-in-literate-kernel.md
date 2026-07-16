<!-- exo:31 ulid:01kg5m2xm58jktbye7esf503na -->

# RFC 31: EARS in Literate Kernel

- **Supersedes**: RFC 10059



# RFC 0031: EARS in Literate Kernel

## Meta

- **Status**: Stage 0 (Draft)
- **Created**: 2025-12-09
- **Authors**: GitHub Copilot
- **Epoch**: Future

## Summary

This RFC proposes incorporating the **EARS (Easy Approach to Requirements Syntax)** notation into the Literate Kernel specification language. This would provide a rigorous, standardized way to define system requirements within the literate documentation.

## Motivation

The Literate Kernel aims to make documentation executable and verifiable. However, requirements are often written in free-form natural language, which can be ambiguous and hard to parse programmatically.

By adopting EARS, we can:

1.  **Reduce Ambiguity**: Enforce a standard structure for all requirements.
2.  **Enable Parsing**: Allow the Literate Kernel to extract requirements and potentially verify them against tests or traces.
3.  **Improve Clarity**: Make specs easier for both humans and agents to understand.

## Proposal

### 1. The EARS Syntax

We will adopt the standard EARS patterns:

- **Ubiquitous**: `The <system> shall <response>`
- **Event-driven**: `When <trigger>, the <system> shall <response>`
- **State-driven**: `While <state>, the <system> shall <response>`
- **Unwanted Behavior**: `If <trigger>, then the <system> shall <response>`
- **Optional Feature**: `Where <feature> is included, the <system> shall <response>`

### 2. Integration with Literate Kernel

We propose a new block type or annotation in the Literate Kernel markdown to denote EARS requirements.

#### Option A: Fenced Code Blocks

```ears
When the user clicks "Save", the System shall persist the changes to disk.
```

#### Option B: Inline Annotations

> [!REQ]
> When the user clicks "Save", the System shall persist the changes to disk.

### 3. Verification

The Literate Kernel parser will extract these statements. Future phases could link these requirements to:

- **Tests**: "This test verifies REQ-123".
- **Traces**: "This execution trace demonstrates REQ-123".

## Drawbacks

- **Rigidity**: May feel too formal for some "literate" contexts.
- **Learning Curve**: Contributors need to learn the EARS patterns.

## Alternatives

- **Gherkin (Given/When/Then)**: More suited for tests than system requirements.
- **Free-form**: Status quo, but lacks rigor.

