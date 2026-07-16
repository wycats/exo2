<!-- exo:10083 ulid:01kmzxefe9dpbehzaq4bayvkbt -->


# RFC 10083: Exposing RFCs as Copilot Resources

- **Status**: Withdrawn
- **Stage**: 0
- **Reason**: Later reconstruction duplicate of RFC 0162; RFC 0162 remains the canonical proposal.

- **Superseded by**: RFC 0162


## Summary

This RFC proposes a mechanism to expose Exosuit RFCs and other documentation artifacts as first-class "Tagged Resources" in GitHub Copilot (e.g., `#rfc:0030`, `#manual:axioms`). This allows users and agents to explicitly reference specific context without dumping entire files into the chat, and enables "Rich Linkage" where the agent can traverse relationships between documents.

## Motivation

- **Context Flooding**: Currently, we often have to read full files or rely on vague semantic search.
- **Precision**: We want to say "Check compliance with #rfc:0030" and have the agent know exactly what that means.
- **Rich Relationships**: RFCs are not islands. They relate to each other (Enforces, Refines, Supersedes). We need a way to expose these relationships to the AI so it can "follow the thread" (e.g., "This Process RFC is enforced by that Tooling RFC").

## Detailed Design

### 1. The Resource Provider

We will implement a VS Code `LanguageModelResourceProvider` (or equivalent Copilot API) that indexes `docs/rfcs/`, `docs/manual/`, and `docs/specs/`.

- **URI Scheme**: `exo://rfc/0030`, `exo://manual/axioms`, `exo://spec/rsl`
- **Display**: `#rfc:0030 - High-Level Workflow`, `#spec:rsl - RTD Style Language`

**Relationship to RFC 0013 (Agent Context Links)**:
This RFC (Copilot Resources) focuses on _exposing_ resources to the AI in the chat interface (via `#` references). RFC 0013 focuses on _linking_ resources within the documents themselves (via `[Link](exo:...)`).

- **Convergence**: Both should use the same underlying URI scheme (`exo:...`) and resolution logic.
- **Synergy**: The `LanguageModelResourceProvider` can use the resolution logic defined in RFC 0013 to traverse the graph.

### 2. Rich Metadata Schema

To support the "Rich Linkage" requested, we need to upgrade the RFC frontmatter schema.

**Current:**

```yaml
title: My RFC
stage: 1
```

**Proposed:**

```yaml
title: My RFC
stage: 1
relations:
  - id: 0012
    type: implements
    description: "Tactical implementation of the Grand Unification strategy."
  - id: 0035
    type: enforced-by
    description: "The process defined here is enforced by the tooling in RFC 0035."
```

### 3. Rust Implementation & Schema

We will define this schema in `exosuit-core` (or the `exo` tool crate) using Serde.

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct RfcHeader {
    pub title: String,
    pub stage: u8,
    pub feature: Option<String>,
    #[serde(default)]
    pub relations: Vec<RfcRelation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RfcRelation {
    pub id: String, // e.g., "0012" or "rfc-process-refinement"
    pub r#type: RelationType,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum RelationType {
    Implements,   // This RFC implements the vision of another
    Refines,      // This RFC updates/clarifies another
    Supersedes,   // This RFC replaces another
    Enforces,     // This RFC provides tooling for another
    EnforcedBy,   // This RFC is governed by another
    Related,      // General relationship
}
```

### 4. Tooling & Coherence

The `exo` CLI will enforce the integrity of this graph.

- **Validation**: `exo check coherence` will verify:
  - **Existence**: Referenced RFC IDs must exist.
  - **Bidirectionality (Soft)**: If A `enforces` B, B should ideally acknowledge A (e.g., `enforced-by`). The tool can warn if the back-link is missing.
  - **Stage Logic**: You cannot `implement` a Withdrawn RFC.
- **Graph Traversal**: `exo rfc graph <id>` will output a DOT or JSON representation of the RFC's neighborhood, which can be fed into the Agent's context.

### 5. The "Context Graph"

When an agent references `#rfc:0030`, the resource provider should not just return the text of RFC 0030. It should optionally return a "Context Graph" summary:

> "RFC 0030 (Stage 1). Implements RFC 0012. Enforced by RFC 0035. Status: Active."

### 6. Manual Integration (Closing the Loop)

To fully realize "Context is King," we must link the **Record of Reality** (`docs/manual/`) back to the **Record of Decision** (`docs/rfcs/`).

- **Schema Extension**: Manual pages will support the same `relations` block.
- **Relation Type**: `derived-from`.
- **Example**: `docs/manual/architecture/axioms.md`

```yaml
title: Design Axioms
relations:
  - id: 0001
    type: derived-from
    description: "Original definition of the Axioms."
  - id: 0030
    type: derived-from
    description: "Added Axiom 10 (Steering-First Tooling)."
```

- **Benefit**: When an agent reads the Manual, it can traverse back to the RFCs to understand the _why_ behind the _what_. This is crucial for "Chesterton's Fence" analysis—understanding why a rule exists before changing it.

## User Experience

- **User**: Types `#rfc` in chat.
- **UI**: Shows list of RFCs with titles and stages.
- **Selection**: User selects `#rfc:0030`.
- **Agent**: Receives the content + metadata of RFC 0030.

## Tooling Implications

- `exo check coherence` needs to validate the `relations` block (e.g., if A says `enforces: B`, B should probably say `enforced-by: A` or at least exist).
