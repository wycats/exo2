# RTD Roadmap & Vision

**Status**: Living Document
**Context**: [RTD Architecture](../architecture.md)

## 1. Vision Statement

**"Rich Text Document (RTD) is the Universal Content Layer for Exosuit."**

In the Exosuit ecosystem, content is never just "text". It is structured, interactive, and streaming. Whether it's an LLM generating a response, a user editing a configuration file, or a log stream from a tool, the underlying data structure is always RTD.

By standardizing on RTD, we achieve:

1.  **Unified Rendering**: A single renderer (React/Svelte) works for Chat, Editors, and Logs.
2.  **Streaming by Default**: Every component assumes data arrives incrementally.
3.  **Semantic Integrity**: We stop parsing regexes on raw strings and start operating on typed Nodes.

## 2. The Path to Stabilization

The goal is to move the RTD specs from **Draft** to **Candidate Recommendation (CR)**.

### Phase 1: Foundation (Current)

- [x] **Consolidation**: Merge fragmented specs into `docs/specs/rtd/`.
- [x] **Object Model**: Define the `RTDNode` TypeScript interfaces (`model.md`).
- [ ] **Package Scaffold**: Create `packages/exosuit-rtd` workspace package.
- [ ] **Type Migration**: Move `RTDNode` types from `exosuit-vscode` to `exosuit-rtd`.

### Phase 2: The Reference Implementation

- [ ] **Formalize Grammar**: Convert `syntax.md` into a rigorous State Machine specification.
- [ ] **Streaming Parser**: Implement the "Tail Buffering" logic (`streaming.md`) in `exosuit-rtd`.
  - _Goal_: A parser that supports **Incremental Updates** (not just block-level yielding) for sub-second latency.
- [ ] **Security by Construction**: Implement a parser that treats HTML as text by default, avoiding the need for post-hoc sanitization.
- [ ] **Test Suite**: Create a "Spec Conformance" test suite.
  - _Task_: **Harvest LLM Corpus**: Collect real-world "messy" outputs from existing logs to seed the test cases.

### Phase 3: Integration & Adoption

- [ ] **Kernel Adoption**: Update `Literate Kernel` to use `exosuit-rtd` for parsing.
- [ ] **Editor Adoption**: Update `Rich Context Editors` to use `exosuit-rtd` for rendering.
- [ ] **RTML Serializer**: Implement the `RTD -> RTML` serializer (`rtml.md`).

### Phase 4: Hardening (Security & Perf)

- [ ] **Security Audit**: Verify that the parser correctly sanitizes HTML and prevents injection.
- [ ] **Performance Benchmarks**: Measure the overhead of the streaming parser on large outputs.
- [ ] **Fuzzing**: Run a fuzzer against the parser to find crashes or infinite loops.

## 3. Aspirational Goals (Future)

- **"Universal Editor"**: A single `<RtdEditor />` component that can edit _any_ RTD document, supporting both "Source Mode" (Markdown) and "WYSIWYG Mode" (Block editing).
- **"Generative UI"**: The ability for the LLM to invent new UI components on the fly by composing standard RTD blocks (e.g., "Create a dashboard using a Grid of Cards").
- **"Collaborative RTD"**: CRDT-based real-time collaboration on RTD documents.
