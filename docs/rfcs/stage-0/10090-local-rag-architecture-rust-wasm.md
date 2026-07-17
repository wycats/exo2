<!-- exo:10090 ulid:01kmzxeff9t51w85zyny5xs2q4 -->


# RFC 10090: Local RAG Architecture (Rust/Wasm)

- **Superseded by**: RFC 0169


**Status**: Stage 0 (Draft)
**Context**: Future Work
**Created**: 2025-12-05

## 1. Motivation

Exosuit relies heavily on structured context (Axioms, Decisions, Plans) stored in TOML files. As the project grows, retrieving relevant context for the Agent becomes a challenge.

Current limitations:
1.  **Keyword Search**: Too brittle. Misses semantic matches.
2.  **Full Context Window**: Too expensive and noisy. Floods the LLM with irrelevant data.
3.  **Cloud Embeddings**: Privacy concerns, latency, and cost. Requires an API key.

We need a **Local, Private, and Fast** retrieval system that understands the specific structure of Exosuit data.

## 2. Architecture

The proposed solution is a **Local RAG (Retrieval-Augmented Generation)** stack running entirely within the VS Code Extension host, powered by Rust and WebAssembly.

### 2.1 The Compute Engine (Rust -> Wasm)

- **Language**: Rust.
- **Target**: `wasm32-unknown-unknown` (or `wasm32-wasi` if running in a worker with WASI polyfills).
- **Runtime**: The Wasm module runs in a dedicated **Web Worker** to keep the VS Code UI thread unblocked.

### 2.2 Embeddings (ONNX Runtime)

Instead of calling an external API (OpenAI), we generate embeddings locally.

- **Engine**: **ONNX Runtime Web**.
- **Model**: Quantized embedding models (e.g., `all-MiniLM-L6-v2` or `bge-small-en-v1.5`).
- **Size**: ~20-40MB. Downloaded on first use or bundled.
- **Performance**: Sub-millisecond inference for small chunks.

### 2.3 Indexing Strategy

Because our domain is "well-defined" (Exosuit TOML), we can outperform generic chunking.

- **Structured Chunking**: We do NOT chunk by arbitrary token counts. We chunk by **Semantic Entity**.
    - *Example*: Each `[[axiom]]` entry is a single chunk.
    - *Example*: Each `[[decision]]` entry is a single chunk.
- **Metadata Extraction**: We extract structured fields (`status`, `tags`, `type`) during indexing to enable hybrid search.

### 2.4 Vector Search

For the scale of a typical workspace (thousands of entities, not millions), we do not need a complex graph index (HNSW).

- **Index Structure**: A simple **Flat Index** (`Vec<f32>`) stored in memory (Wasm linear memory).
- **Algorithm**: Brute-force Cosine Similarity.
- **Persistence**: The index can be serialized to a binary format or stored in **SQLite Wasm** (via the Extension's global storage).

### 2.5 Hybrid Search

The query engine combines vector similarity with structured filtering.

- **Query**: "Show me active decisions about parsing."
- **Logic**:
    1.  **Filter**: `type == "decision" AND status == "active"` (Exact Match).
    2.  **Search**: Cosine similarity for "parsing" against the filtered subset.

## 3. Implementation Plan (Sketch)

1.  **`packages/exosuit-rag`**: A new Rust crate.
2.  **Dependencies**: `ort` (ONNX Runtime bindings), `tokenizers` (HuggingFace), `serde`.
3.  **Build Pipeline**: `wasm-pack` to generate the Wasm binary and JS glue.
4.  **Extension Integration**: A `RagService` in the VS Code extension that manages the Worker and handles query messages.

## 4. Alternatives Considered

- **VS Code Copilot `#codebase`**: Closed API. Cannot be accessed programmatically.
- **Cloud Vector DB (Pinecone)**: Requires network, API keys, and data exfiltration.
- **Local Python Server**: Too heavy. Requires user to install Python environment. Wasm is zero-install.
