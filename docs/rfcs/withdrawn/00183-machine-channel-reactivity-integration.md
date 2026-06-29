<!-- exo:183 ulid:01kmzxbczb25cbstpr9y09zjq2 -->

# RFC 183: Machine Channel Reactivity Integration


# **Withdrawn**: Consolidated into RFC 00188 (Derived Roots & Reactive Caches)

# RFC 00183: Machine Channel Reactivity Integration

## Summary

Integrate machine channel responses with the VS Code extension's reactivity system so that CLI-derived data participates in automatic invalidation and refresh cycles.

## Motivation

### Current State

The extension has two data-fetching patterns that don't interoperate:

1. **Reactivity System** (`ReactivityService`): Tracks file changes via WASM-based dependency graph. When files change, registered "roots" are invalidated and subscribers refresh.

2. **Machine Channel** (`exoMachineChannel`): Calls CLI commands via persistent subprocess, returns structured JSON. No caching, no invalidation awareness.

### The Problem

When the extension fetches data via machine channel (e.g., `exo rfc show` for RFC metadata), that data:

- Is fetched fresh on every call (no caching)
- Doesn't invalidate when underlying files change
- Doesn't participate in the reactivity graph

**Example**: The Phase Details tree fetches RFC metadata via machine channel. If an RFC's title changes, the tree won't update until something else triggers a refresh.

### Current Workarounds

Ad-hoc solutions exist:

- Subscribe to broad file patterns and refresh everything
- Re-fetch on every render (wasteful)
- Ignore staleness (incorrect)

None of these are architecturally sound.

## Design Sketch

### Core Insight

Machine channel responses are **derived data** from files on disk. The CLI already knows which files it reads to produce a response. If the CLI reported its file dependencies, the extension could:

1. Cache the response
2. Register those dependencies with the reactivity system
3. Invalidate the cache when dependencies change

### Proposed Protocol Extension

Add an optional `dependencies` field to `MachineChannelResponseEnvelope`:

```typescript
interface MachineChannelResponseEnvelope {
  // ... existing fields ...

  /** Files read to produce this response (for reactivity integration) */
  dependencies?: {
    /** Absolute paths of files read */
    files: string[];
    /** Directory listings consulted */
    directories?: string[];
  };
}
```

### Extension-Side Integration

```typescript
// Pseudocode for reactive machine channel wrapper
async function reactiveMachineChannel(
  cwd: string,
  request: MachineChannelRequestEnvelope,
): Promise<MachineChannelResponseEnvelope> {
  const cacheKey = computeCacheKey(request);

  // Check cache
  const cached = responseCache.get(cacheKey);
  if (cached && !cached.invalidated) {
    return cached.response;
  }

  // Fetch fresh
  const response = await exoMachineChannel(cwd, request);

  // Register dependencies with reactivity system
  if (response.dependencies) {
    for (const file of response.dependencies.files) {
      reactivityService.trackDependency(cacheKey, file);
    }
  }

  // Cache response
  responseCache.set(cacheKey, { response, invalidated: false });

  return response;
}
```

### CLI-Side Changes

Commands that read files would report their dependencies:

```rust
// In exo rfc show
fn execute(&self, ctx: &Context) -> Result<Response> {
    let rfc_path = self.find_rfc_file()?;
    let content = fs::read_to_string(&rfc_path)?;
    let metadata = parse_frontmatter(&content)?;

    Ok(Response {
        result: metadata,
        dependencies: Some(Dependencies {
            files: vec![rfc_path],
            directories: None,
        }),
    })
}
```

## Open Questions

1. **Granularity**: Should dependencies be file-level or finer (e.g., specific TOML keys)?
2. **Cache invalidation strategy**: Eager (invalidate immediately) vs lazy (check on next access)?
3. **Memory pressure**: How to bound cache size for long-running sessions?
4. **Opt-in vs opt-out**: Should all commands report dependencies, or only those that opt in?

## Related Work

- RFC 0071: Observation scopes and `DirListing` primitive. Defines the observation scope API and directory listing semantics this RFC should use for dependency reporting and invalidation.
- RFC 0097: Machine Channel v2 (persistent server mode)
- `ReactivityService`: WASM-based file dependency tracking
- `RootMaterializerRegistry`: Current pattern for registering file-backed roots

## Future Possibilities

- Extend to non-file dependencies (git state, environment variables)
- Use for LM tool response caching
- Enable "stale-while-revalidate" patterns for better perceived performance

