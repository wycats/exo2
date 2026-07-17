<!-- exo:127 ulid:01kg5kp2hbfdtes1ny3qvjjxne -->

# RFC 127: Rigorous Rust Infrastructure

- **Supersedes**: RFC 10109



# RFC 0127: Rigorous Rust Infrastructure

## Summary

Establish a robust, production-grade infrastructure for the `exosuit` Rust codebase (currently `tools/rfc-status` and future tools), mirroring the high standards set in the `locald` project.

## Motivation

- **Reliability**: As we move core agent logic into Rust (RFC 0009), the tooling becomes critical path. It must be reliable.
- **Developer Experience**: A well-configured environment with fast feedback loops (linting, testing) improves velocity.
- **Consistency**: Adopting a proven setup (from `locald`) reduces decision fatigue and ensures we use best practices.

## Detailed Design

### 1. Cargo Workspace Configuration

We will adopt a flat workspace structure with centralized configuration in the root `Cargo.toml`.

- **Resolver**: Set `resolver = "3"` to support Rust 2024 and unify feature resolution.
- **Edition**: Set `[workspace.package] edition = "2024"` so all members inherit it.
- **Lints**: Define all linting rules in `[workspace.lints]` to ensure uniformity across crates.

### 2. Linting & Formatting (Strict)

We will enforce code quality via tooling, not just human review.

- **Clippy Configuration**:
  - **Deny**: `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `dbg_macro`.
  - **Philosophy**: "If it crashes, it's a bug." Force error handling at the code level.
- **Pre-commit Hooks**: Use `lefthook` to run checks before commit.
  - `fmt`: `cargo fmt`
  - `clippy`: `cargo clippy --workspace -- -D warnings`

### 3. Testing Philosophy: "Real World, No Magic"

We explicitly avoid mocks and snapshot testing in favor of real integration tests.

- **No Mocks**: Do not mock the filesystem or OS. Use `assert_cmd` to spawn the real binary and interact with it.
- **No Snapshots**: Do not use `insta`. Assert on specific, meaningful properties of the output to avoid brittle tests.
- **Structure**:
  - **Unit Tests**: Minimal, only for pure logic.
  - **Integration Tests**: Located in `tests/`, spinning up temporary directories and running real commands.

### 4. CI/CD (GitHub Actions)

The CI pipeline is the final gatekeeper.

- **Checks**: `cargo fmt --all -- --check` and `cargo clippy --workspace -- -D warnings`.
- **Coverage**: Use `cargo-llvm-cov` to generate accurate code coverage reports (`lcov.info`) and upload to Codecov.
- **WASM Compatibility**: Ensure `wasm32-wasip1` compilation is checked.

## Unresolved Questions

- None. The `locald` baseline provides a complete specification.

