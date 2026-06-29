# RTD Style Language (RSL) Specification

## 1. Introduction

The RTD Style Language (RSL) is a domain-specific styling language designed for Rich Text Documents (RTD). It provides a high-level, semantic abstraction over CSS, focusing on _intent_ rather than implementation details.

RSL is designed to be:

1.  **Semantic**: Styles are defined using meaningful tokens (e.g., `stack`, `surface-1`, `xl`) rather than raw values.
2.  **Constrained**: It exposes a closed set of "Super Properties" that encapsulate common UI patterns.
3.  **Compilable**: It compiles to standard CSS (Baseline 2025), leveraging CSS Variables, Logical Properties, and Container Queries.
4.  **Configurable**: The mapping from RSL tokens to CSS is defined in a "Token Registry," allowing for easy theming and evolution without changing the compiler.

## 2. Syntax

RSL uses a CSS-like syntax with a restricted property set.

```css
/* Selector */
.variant-name {
  /* Super Property: value-tokens... */
  layout: md stack;
  surface: surface-1;
}
```

### 2.1 Selectors

RSL supports two types of selectors:

1.  **Variant Selector** (`.name`): Matches an `RTDContainer` block with `variant="name"`.
2.  **Element Selector** (`name`): Matches a core RTD block type (e.g., `heading`, `paragraph`, `list`, `blockquote`).

### 2.2 Super Properties

RSL defines exactly 5 properties. Each property accepts a **Dictionary** of slot assignments.

**Syntax:** `Property: Slot Token, Slot Token;`

- **Comma-separation** denotes distinct slot assignments.
- **Space-separation** delimits the Slot Name from the Token Value.

| Property  | Description                | Slots             | Example                   |
| :-------- | :------------------------- | :---------------- | :------------------------ |
| `layout`  | Arrangement of children    | `mode`, `gap`     | `mode stack, gap md`      |
| `surface` | Background & color context | `base`            | `base surface-1`          |
| `spacing` | Box model padding          | `all`, `x`, `y`   | `x sm, y lg`              |
| `border`  | Boundary styling           | `style`, `radius` | `style subtle, radius md` |
| `text`    | Typography                 | `size`, `weight`  | `size xl, weight bold`    |

## 3. The Token Registry

The behavior of RSL is defined by the **Token Registry**. This registry maps `(Property, Slot, Token)` triples to a **Token Definition**.

### 3.1 Token Definition Schema

A Token Definition consists of:

- **`css`** (Required): A partial CSS object (property-value pairs) to be applied when this token is present.

### 3.2 Resolution Algorithm

Given a declaration `Property: Slot1 Token1, Slot2 Token2 ...`:

1.  Initialize an empty `ResultCSS` object.
2.  Parse the declaration into a list of `(Slot, Token)` pairs.
3.  For each `(Slot, Token)` pair:
    a. Look up the `TokenDefinition` in the Registry for the given `(Property, Slot, Token)`.
    b. If the definition is not found, emit a warning and skip.
    c. Merge `definition.css` into `ResultCSS` using the **Merge Policy** (see Section 3.3).
4.  Return `ResultCSS`.

### 3.3 Merge Policy

To handle conflicts when merging CSS objects, the compiler enforces the following rules:

1.  **Atomic Constraint**: Tokens SHOULD emit atomic (longhand) properties (e.g., `border-width` instead of `border`) to avoid shorthand collisions.
2.  **Variable Composition**: Tokens SHOULD use CSS variables for partial updates (e.g., setting `--shadow-color` instead of redefining `box-shadow`).
3.  **Stackable Properties**: A specific set of properties (`transform`, `filter`, `backdrop-filter`, `transition`) are defined as **Stackable**. When merging these, the new value is **appended** (space-separated) to the existing value instead of replacing it.
4.  **Default Replacement**: For all other properties, the new value **replaces** the existing value (Last Write Wins).

## 4. Default Registry Specification

The following defines the standard set of tokens for the default theme.

### 4.1 `layout`

| Slot   | Token     | CSS Mapping                                                                                   |
| :----- | :-------- | :-------------------------------------------------------------------------------------------- |
| `mode` | `stack`   | `display: flex; flex-direction: column;`                                                      |
| `mode` | `row`     | `display: flex; flex-direction: row; align-items: center;`                                    |
| `mode` | `grid`    | `display: grid; grid-template-columns: repeat(auto-fit, minmax(var(--rtd-min, 200px), 1fr));` |
| `mode` | `flow`    | `display: block;`                                                                             |
| `gap`  | `[space]` | `gap: var(--rtd-space-[space]);`                                                              |

_Note: `[space]` represents a scale (xs, sm, md, lg, xl)._

### 4.2 `surface`

| Slot   | Token         | CSS Mapping                                                                             |
| :----- | :------------ | :-------------------------------------------------------------------------------------- |
| `base` | `surface-[n]` | `background-color: var(--rtd-color-surface-[n]); color: var(--rtd-color-text-primary);` |
| `base` | `accent`      | `background-color: var(--rtd-color-accent); color: var(--rtd-color-text-inverse);`      |
| `base` | `transparent` | `background-color: transparent;`                                                        |

### 4.3 `spacing`

| Slot  | Token     | CSS Mapping                                 |
| :---- | :-------- | :------------------------------------------ |
| `all` | `[space]` | `padding: var(--rtd-space-[space]);`        |
| `x`   | `[space]` | `padding-inline: var(--rtd-space-[space]);` |
| `y`   | `[space]` | `padding-block: var(--rtd-space-[space]);`  |

### 4.4 `border`

| Slot     | Token     | CSS Mapping                                         |
| :------- | :-------- | :-------------------------------------------------- |
| `style`  | `subtle`  | `border: 1px solid var(--rtd-color-border-subtle);` |
| `style`  | `bold`    | `border: 2px solid var(--rtd-color-border-bold);`   |
| `radius` | `[space]` | `border-radius: var(--rtd-radius-[space]);`         |

### 4.5 `text`

| Slot     | Token    | CSS Mapping                               |
| :------- | :------- | :---------------------------------------- |
| `size`   | `[size]` | `font-size: var(--rtd-font-size-[size]);` |
| `weight` | `bold`   | `font-weight: 700;`                       |
| `style`  | `italic` | `font-style: italic;`                     |
| `family` | `mono`   | `font-family: var(--rtd-font-mono);`      |

## 5. CSS Compilation Targets

The compiler MUST generate CSS that adheres to the following constraints:

1.  **Logical Properties**: Use `margin-block`, `padding-inline`, etc., instead of physical directions.
2.  **CSS Variables**: All values MUST be CSS variables (e.g., `var(--rtd-space-md)`). Hardcoded pixels are forbidden in the registry.
3.  **Container Queries**: Every element with a `layout` property MUST automatically receive `container-type: inline-size`.

## 6. Example

**Input (RSL):**

```css
.feature-card {
  layout: mode stack, gap md;
  surface: base surface-1;
  spacing: all lg;
  border: style subtle, radius md;
}
```

**Resolution Trace:**

1.  `layout`:
    - `mode stack` -> `{ display: flex, flex-direction: column }`
    - `gap md` -> `{ gap: var(--rtd-space-md) }`
    - Result: `{ display: flex, flex-direction: column, gap: var(--rtd-space-md) }`
2.  `surface`:
    - `base surface-1` -> `{ background-color: var(--rtd-color-surface-1), ... }`
3.  `spacing`:
    - `all lg` -> `{ padding: var(--rtd-space-lg) }`
4.  `border`:
    - `style subtle` -> `{ border: 1px solid ... }`
    - `radius md` -> `{ border-radius: var(--rtd-radius-md) }`

**Output (CSS):**

```css
.rtd-variant-feature-card {
  display: flex;
  flex-direction: column;
  gap: var(--rtd-space-md);
  background-color: var(--rtd-color-surface-1);
  color: var(--rtd-color-text-primary);
  padding: var(--rtd-space-lg);
  border: 1px solid var(--rtd-color-border-subtle);
  border-radius: var(--rtd-radius-md);
  container-type: inline-size;
}
```
