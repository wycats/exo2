<!-- exo:10107 ulid:01kmzxey196k5r5wb3e66entgz -->


# RFC 10107: RTD Surface Mapping

- **Status**: Withdrawn
- **Stage**: 1
- **Reason**: Withdrawn by RFC 10180 storage disposition: this proposal depends on retired file-backed phase context or direct editing/protection of legacy docs/agent-context current artifacts.

````markdown
# RTD Surface Mapping Specification

**Status**: Draft
**Phase**: 22.5
**Context**: [Feedback System](./feedback-system.md), [RSS Screen Proposals](./rss-screen-proposals.md)

## 1. Overview

This document defines the **Rendering Pipeline** for the Exosuit "Rich Context" Editors. It specifies how we transform the abstract "Surface Syntax" (LDL, RSL, RTML) into concrete HTML DOM nodes and CSS within a VS Code Webview.

The goal is to decouple the **Intent** (what the UI _is_) from the **Implementation** (how the UI _looks_), allowing us to evolve the design system without rewriting the logic.

## 2. The Rendering Pipeline

The rendering process follows a strict unidirectional flow:

1.  **Source**: The raw `.toml` file (e.g., `docs/agent-context/plan.toml` or `docs/agent-context/current/implementation-plan.toml`).
2.  **Hydration**: The TOML is parsed into **Domain Objects** (e.g., `Decision`, `Axiom`).
3.  **Projection**: Domain Objects are projected into an **LDL Tree** (Layout Description Language).
4.  **Styling**: The LDL nodes are decorated with **RSL Tokens** (RTD Style Language).
5.  **Content Rendering**: Text fields within the objects are parsed as **RTML** (RTD Markup Language).
6.  **Paint**: The tree is rendered to the **DOM** (Svelte Components).

## 3. Layer 1: Layout (LDL -> DOM)

The **Layout Description Language (LDL)** defines the structural hierarchy. It maps semantic containers to semantic HTML.

| LDL Component  | Semantic Intent                              | HTML Mapping                          | Accessibility Roles            |
| :------------- | :------------------------------------------- | :------------------------------------ | :----------------------------- |
| `<surface>`    | The root container of the editor.            | `<main>`                              | `role="main"`                  |
| `<region>`     | A major structural division (e.g., Sidebar). | `<section>`                           | `aria-label="..."`             |
| `<collection>` | A list of similar items.                     | `<div class="collection">`            | `role="feed"` or `role="list"` |
| `<item>`       | A discrete unit of data (Card).              | `<article>`                           | `role="article"`               |
| `<field>`      | A labeled data point.                        | `<div class="field">`                 | N/A                            |
| `<overlay>`    | A transient layer (popover/dialog).          | `<dialog>` or `<div class="popover">` | `role="dialog"`                |

### Example Mapping

**LDL:**

```xml
<region title="Active Feedback">
  <collection>
    <item id="fb-1">...</item>
  </collection>
</region>
```
````

**DOM:**

```html
<section aria-label="Active Feedback">
  <header>Active Feedback</header>
  <div class="collection" role="feed">
    <article id="fb-1" role="article">...</article>
  </div>
</section>
```

## 4. Layer 2: Styling (RSL -> CSS)

The **RTD Style Language (RSL)** is a token-based abstraction over CSS. It prevents "magic values" and enforces consistency.

### 4.1 The Layout Token (`layout`)

Controls the Flexbox/Grid behavior of a container.

| Token          | CSS Output                                                                           |
| :------------- | :----------------------------------------------------------------------------------- |
| `stack {size}` | `display: flex; flex-direction: column; gap: var(--space-{size});`                   |
| `row {size}`   | `display: flex; flex-direction: row; gap: var(--space-{size}); align-items: center;` |
| `grid {cols}`  | `display: grid; grid-template-columns: {cols};`                                      |

### 4.2 The Surface Token (`surface`)

Controls background, text color, and elevation.

| Token         | CSS Output                                                     |
| :------------ | :------------------------------------------------------------- |
| `surface-1`   | `background: var(--bg-primary); color: var(--text-primary);`   |
| `surface-2`   | `background: var(--bg-secondary); color: var(--text-primary);` |
| `transparent` | `background: transparent;`                                     |
| `accent`      | `background: var(--bg-accent); color: var(--text-inverse);`    |

### 4.3 The Border Token (`border`)

Controls borders and radii.

| Token             | CSS Output                                                                       |
| :---------------- | :------------------------------------------------------------------------------- |
| `subtle {radius}` | `border: 1px solid var(--border-subtle); border-radius: var(--radius-{radius});` |
| `bold {radius}`   | `border: 2px solid var(--border-bold); border-radius: var(--radius-{radius});`   |

### 4.4 The Text Token (`text`)

Controls typography.

| Token     | CSS Output                                                  |
| :-------- | :---------------------------------------------------------- |
| `xl bold` | `font-size: var(--font-xl); font-weight: 700;`              |
| `sm mono` | `font-size: var(--font-sm); font-family: var(--font-mono);` |

## 5. Layer 3: Content (RTML -> HTML)

The **RTD Markup Language (RTML)** is the format used for the _content_ inside the cards (e.g., the `summary` of a decision, or the `body` of a feedback message).

Unlike standard Markdown, RTML is **Interactive**. It supports "Active Elements" that trigger VS Code commands.

### 5.1 The RTML Blob

When a TOML field is marked as `type = "rtd"`, it is processed by the RTML Renderer.

**Input (TOML String):**

```toml
summary = "We are adopting [Svelte](link:svelte) to fix the *reactivity* issues."
```

**Output (DOM):**

```html
<div class="rtd-content">
  <p>
    We are adopting <a href="..." data-command="open-docs">Svelte</a> to fix the
    <em>reactivity</em> issues.
  </p>
</div>
```

### 5.2 Active Elements

RTML supports custom tags that map to Svelte components within the content flow.

| RTML Element    | DOM Mapping                | Behavior                                         |
| :-------------- | :------------------------- | :----------------------------------------------- |
| `<rtd-command>` | `<button class="rtd-cmd">` | Triggers a VS Code command via `postMessage`.    |
| `<rtd-link>`    | `<a class="rtd-link">`     | Opens an external URL or internal file.          |
| `<rtd-status>`  | `<span class="badge">`     | Displays a status pill (e.g., `[Status: Done]`). |

## 6. Implementation Strategy (Svelte)

We will implement a generic `<RtdView>` component that accepts an **LDL Node** and recursively renders it.

```svelte
<script>
  export let node; // LDL Node
</script>

{#if node.type === 'region'}
  <section use:rsl={node.style}>
    <RtdView node={node.children} />
  </section>
{:else if node.type === 'item'}
  <article use:rsl={node.style}>
    <RtdView node={node.children} />
  </article>
{:else if node.type === 'content'}
  <!-- The RTML Renderer -->
  <RtdContent content={node.value} />
{/if}
```

The `use:rsl` action will compile the RSL string into CSS variables and classes at runtime.
