# Tooling Interface Specification

**Status:** Draft
**Version:** 0.1.0

## Overview

This document defines the specification for the Tool Presentation Interface in Exosuit. It governs how agent tools are presented to the user, including their lifecycle states (progress, success, error), visual rendering, and interpolation logic.

The goal is to decouple the _logic_ of a tool from its _presentation_, allowing for a configurable and rich user experience that can evolve independently of the underlying code.

## Schema Definition

Tool presentation definitions are stored in TOML format.

- **Preferred (config)**: `.config/exo/tool-presentation.toml`
- **Legacy (back-compat)**: `docs/agent-context/tool-presentation.toml`

`exo update` migrates the legacy file into the preferred config location when the preferred file is missing.

### Current Structure

Today, Exosuit supports a single presentation template per tool:

```toml
[[tools]]
name = "readFile"
alias = "read_file" # optional
presentation = "Reading file: {path}"
```

The `presentation` string is currently used as the tool progress message.

### Future Structure (Not Yet Implemented)

We may expand this to distinct lifecycle templates (progress/success/error), icons, and richer renderers, but the current on-disk schema does not include those fields.

## Interpolation Standard

The presentation strings support a strict variable substitution syntax.

### Syntax

- **Format**: `{key}`
- **Scope**: Keys must correspond to top-level arguments in the tool's input schema (for `progress`) or the tool's output schema (for `success`).
- **Behavior**:
  - If `key` exists in the data context, it is substituted verbatim.
  - If `key` is missing or null, the token is preserved (e.g., `{missing}`) to aid debugging.
  - Whitespace inside braces is ignored (e.g., `{ path }` is treated as `{path}`).
  - Nested property access (e.g., `{obj.prop}`) is **NOT** supported in this version.

### Example

Given a tool `readFile` with arguments `{ path: "src/main.ts" }`:

- Template: `"Reading {path}..."`
- Result: `"Reading src/main.ts..."`

## Presentation Lifecycle

Currently, Exosuit uses a single progress-style presentation string for each tool invocation.

If/when we add distinct success/error templates, the same interpolation standard applies.

## Visual Capabilities (Renderers)

Exosuit’s chat UI may render rich output for certain tools (e.g. file references and file trees), but `tool-presentation.toml` does not currently control renderer selection. Consider renderer configuration future work.

## Future Considerations

- **Conditional Templates**: Support for logic like `"{count} file{s}"` (pluralization).
- **Rich Interpolation**: Support for formatting (e.g., `{date:ISO}`).
