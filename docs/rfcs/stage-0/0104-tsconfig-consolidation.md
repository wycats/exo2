<!-- exo:104 ulid:01kg5m2xs1a5md0artk7m660se -->

# RFC 104: TSConfig Consolidation

# RFC 0104: TSConfig Consolidation

- **Stage**: 0 (Idea)
- **Champion**: TBD
- **Created**: 2025-07-21
- **Related**: Build System, Developer Experience

## Summary

Consolidate the current 9 tsconfig files down to 2-3 well-structured configurations using TypeScript 5.5+'s `${configDir}` template variable.

## Motivation

The workspace currently has 9 tsconfig files:

1. `tsconfig.base.json` - Base configuration
2. `packages/exosuit-core/tsconfig.json`
3. `packages/exosuit-rtd/tsconfig.json`
4. `packages/exosuit-vscode/tsconfig.json`
5. `packages/exosuit-vscode/tsconfig.node.json`
6. `packages/exosuit-vscode/tsconfig.webview.json`
7. `packages/exosuit-vscode/test/tsconfig.json`
8. `packages/exosuit-vscode/webview/tsconfig.json`
9. `packages/exosuit-vscode/src/generated/protocol/tsconfig.json`

This proliferation creates several problems:

1. **Maintenance burden**: Changes to compiler settings must be replicated across files
2. **Inconsistency**: Different packages may drift in their configurations
3. **Configuration errors**: The webview config extends the Node-focused base instead of `@tsconfig/svelte`, causing Svelte 5 type errors
4. **Cognitive load**: Developers must understand which config applies where

## Proposal

### Target Structure: 3 Configs

```
tsconfig.base.json        # Shared settings (strict, ES2022, paths)
tsconfig.node.json        # Node.js targets (extension host, scripts)
tsconfig.svelte.json      # Svelte/browser targets (webview)
```

Package configs become thin wrappers:

```json
// packages/exosuit-vscode/tsconfig.json
{
  "extends": "../../tsconfig.node.json",
  "compilerOptions": {
    "outDir": "${configDir}/dist"
  },
  "include": ["${configDir}/src/**/*"]
}
```

### Key Design Decisions

#### 1. Use `${configDir}` (TS 5.5+)

The `${configDir}` template variable resolves to the directory containing the tsconfig file. This makes base configs truly portable:

```json
// tsconfig.node.json
{
  "extends": "./tsconfig.base.json",
  "compilerOptions": {
    "module": "Node16",
    "moduleResolution": "Node16",
    "outDir": "${configDir}/dist",
    "rootDir": "${configDir}/src"
  }
}
```

#### 2. Separate Svelte Config

Svelte requires specific settings that conflict with Node.js configs:

```json
// tsconfig.svelte.json
{
  "extends": ["./tsconfig.base.json", "@tsconfig/svelte/tsconfig.json"],
  "compilerOptions": {
    "module": "ESNext",
    "moduleResolution": "bundler",
    "verbatimModuleSyntax": true
  }
}
```

#### 3. Corsa Compatibility

TypeScript's native preview (`@typescript/native-preview`, aka Corsa/TS7) promises 10x faster type checking. Current research shows:

- ✅ Project references supported
- ❌ No TypeScript API yet (breaks svelte-check, eslint-typescript)
- ⏳ API support coming in future releases

Our consolidated structure should be forward-compatible with Corsa when API support lands.

### Migration Plan

| Current Config                                                 | Target       | Notes                              |
| -------------------------------------------------------------- | ------------ | ---------------------------------- |
| `tsconfig.base.json`                                           | Keep         | Add `${configDir}` patterns        |
| `packages/exosuit-core/tsconfig.json`                          | Thin wrapper | Extends `tsconfig.node.json`       |
| `packages/exosuit-rtd/tsconfig.json`                           | Thin wrapper | Extends `tsconfig.node.json`       |
| `packages/exosuit-vscode/tsconfig.json`                        | Thin wrapper | Extends `tsconfig.node.json`       |
| `packages/exosuit-vscode/tsconfig.node.json`                   | **Delete**   | Merge into main tsconfig.node.json |
| `packages/exosuit-vscode/tsconfig.webview.json`                | **Fix**      | Extend `tsconfig.svelte.json`      |
| `packages/exosuit-vscode/test/tsconfig.json`                   | Thin wrapper | Extends `tsconfig.node.json`       |
| `packages/exosuit-vscode/webview/tsconfig.json`                | Thin wrapper | Extends `tsconfig.svelte.json`     |
| `packages/exosuit-vscode/src/generated/protocol/tsconfig.json` | Evaluate     | May be deletable                   |

### Resulting File Count

- Before: 9 files
- After: 3 base configs + ~6 thin wrappers = effectively 3 maintained configs

## Benefits

1. **Single source of truth**: Compiler settings defined once
2. **Correct defaults**: Svelte configs properly extend `@tsconfig/svelte`
3. **Portable paths**: `${configDir}` eliminates brittle relative paths
4. **Future-proof**: Compatible with Corsa when API support lands
5. **Fewer errors**: Eliminates config mismatches that cause false positives

## Risks

1. **Breaking changes**: Incorrect migration could break builds
2. **Tool compatibility**: Some tools may not support `${configDir}` yet
3. **Debugging complexity**: Inherited settings can be harder to trace

## Open Questions

1. Should generated protocol code have its own config or share with extension?
2. How do we handle test configs for package tests and extension-host tests?
3. Should we wait for Corsa API support before restructuring?

## Next Steps

1. Audit all 9 configs to understand actual differences
2. Create `tsconfig.svelte.json` extending `@tsconfig/svelte`
3. Migrate webview configs to fix immediate Svelte 5 errors
4. Consolidate remaining configs incrementally
