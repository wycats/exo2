<script lang="ts">
  import {
    parseMarkdown,
    HTMLRenderer,
    StreamingParser,
    RSLParser,
    type RSLStyle,
  } from "@exosuit/rtd";

  let input = $state(
    `# RTD Playground

This playground demonstrates the **robustness** and **security** features of the Exosuit RTD Parser.

## 1. LLM Robustness

### Unclosed Comments
The parser automatically closes comments if it encounters a strong block starter (like a Header), or at EOF.

<!-- This comment is unclosed, but the next header will force it to close.

### Ghost Citations
LLMs often output citations like this: 【13†source】. We normalize them to a proper citation token.

### Math Normalization
We handle LaTeX delimiters:
- Block: \\[ x^2 + y^2 = z^2 \\]
- Inline: \\( E = mc^2 \\)

## 2. Security (No HTML)

We strictly reject HTML tags. They are rendered as plain text:

<div>
  <script>alert('XSS')<\/script>
</div>

## 3. Link Security

We sanitize unsafe links:

- [Safe Link](https://example.com)
- [Unsafe Link](javascript:alert(1)) (Rendered as text)
- [Relative Link](./doc.md)

## 4. Nested Fences

We handle nested code blocks gracefully:

\`\`\`\`\`\`markdown
# Nested
\`\`\`\`\`\`javascript
console.log("Hello");
\`\`\`\`\`\`
\`\`\`\`\`\`

## 5. Lists

- Unordered
- List
  1. Nested
  2. Ordered

## 6. Admonitions

::: note
This is a note.
:::

::: warning
Be careful!
:::

## 7. Generic Directives

::: custom-container
Content inside a generic container.
:::

## 8. New Features (Phase 37+)

### Tables (GFM)

| Feature | Status | Notes |
| :--- | :---: | ---: |
| Tables | ✅ | GFM-style |
| Align | ✅ | Left/Center/Right |
| Math | ✅ | In cells too! $E=mc^2$ |

### Strikethrough

This is ~~wrong~~ correct.

### Task Lists

- [x] Completed task
- [ ] Pending task
- [ ] **Bold** task

### Icons & Commands

- Icon: $(gear) (rendered as span with codicon class)
- Command: [Run Test](command:test.run)
`
  );

  let rslInput = $state(`region {
  layout: stack lg;
  surface: transparent;
}

collection {
  layout: stack md;
}

item {
  surface: surface-1;
  border: subtle md;
  layout: stack md;
  spacing: all md;
}

field {
  layout: row sm;
  text: size sm;
}

badge {
  surface: surface-2;
  border: radius sm;
  spacing: y xs, x sm;
  text: size xs, weight bold;
}
`);

  let rssInput = $state(`::: region
# Active Feedback

::: collection
::: item
**Status**: ::: badge
Done
:::

**Summary**: We are adopting [Svelte](link:svelte) to fix the *reactivity* issues.

**Impact**: High
:::

::: item
**Status**: ::: badge
In Progress
:::

**Summary**: Refactoring the **Parser** to support streaming.
:::
:::
:::
`);

  let ast = $state<any[]>([]);
  let renderer = new HTMLRenderer({
    allowedSchemes: ["http", "https", "mailto", "command"],
  });
  let html = $derived(renderer.render(ast));

  let activeTab = $state("preview"); // 'preview' | 'ast' | 'rsl'
  let isStreaming = $state(false);

  // RSL Parsing Logic
  let rslStyles = $derived.by(() => {
    const map = new Map<string, RSLStyle>();
    const parser = new RSLParser();

    // Simple regex to split "selector { body }"
    const ruleRegex = /([a-zA-Z0-9-_]+)\s*\{([^}]+)\}/g;
    let match;

    while ((match = ruleRegex.exec(rslInput)) !== null) {
      const selector = match[1].trim();
      const body = match[2].trim();
      try {
        const style = parser.parse(body);
        map.set(selector, style);
      } catch (e) {
        console.error(`Failed to parse RSL for ${selector}`, e);
      }
    }
    return map;
  });

  // Custom Renderer for RSL
  let rslRenderer = $derived(
    new HTMLRenderer({
      resolveStyle: (variant) => rslStyles.get(variant),
    })
  );

  let rslHtml = $derived.by(() => {
    try {
      const parsedAst = parseMarkdown(rssInput);
      return rslRenderer.render(parsedAst);
    } catch (e) {
      return `<div style="color: red">Error parsing Content: ${(e as Error).message}</div>`;
    }
  });

  // Initial parse & updates when not streaming
  $effect(() => {
    if (!isStreaming && activeTab !== "rsl") {
      ast = parseMarkdown(input);
    }
  });

  async function simulateStreaming() {
    if (isStreaming) return;
    isStreaming = true;
    activeTab = "preview"; // Force preview tab

    const fullText = input;
    input = ""; // Clear input to show typing effect
    ast = []; // Clear AST

    const parser = new StreamingParser();
    const chunkSize = 2; // Small chunks to see the effect
    let cursor = 0;

    while (cursor < fullText.length) {
      const chunk = fullText.slice(cursor, cursor + chunkSize);
      input += chunk;

      // Parse the chunk
      // Note: parser.parse returns the internal blocks array reference
      // We spread it to trigger reactivity
      ast = [...parser.parse(chunk)];

      cursor += chunkSize;
      await new Promise((r) => setTimeout(r, 30)); // 30ms delay
    }

    ast = parser.flush();
    isStreaming = false;
  }
</script>

<main style="display: flex; height: 100vh; flex-direction: column;">
  <header
    style="padding: 1rem; background: #f3f4f6; border-bottom: 1px solid #e5e7eb; display: flex; justify-content: space-between; align-items: center;"
  >
    <h1 style="margin: 0; font-size: 1.25rem; font-weight: bold;">
      RTD Playground
    </h1>
    <div style="display: flex; gap: 1rem;">
      <div
        style="display: flex; background: #e5e7eb; border-radius: 4px; padding: 2px;"
      >
        <button
          style="padding: 0.5rem 1rem; border: none; background: {activeTab !==
          'rsl'
            ? '#fff'
            : 'transparent'}; border-radius: 2px; cursor: pointer;"
          onclick={() => (activeTab = "preview")}
        >
          Markdown
        </button>
        <button
          style="padding: 0.5rem 1rem; border: none; background: {activeTab ===
          'rsl'
            ? '#fff'
            : 'transparent'}; border-radius: 2px; cursor: pointer;"
          onclick={() => (activeTab = "rsl")}
        >
          RSL Studio
        </button>
      </div>

      <button
        onclick={simulateStreaming}
        disabled={isStreaming || activeTab === "rsl"}
        style="padding: 0.5rem 1rem; background: #2563eb; color: white; border: none; border-radius: 4px; cursor: pointer; opacity: {isStreaming ||
        activeTab === 'rsl'
          ? 0.5
          : 1};"
      >
        {isStreaming ? "Streaming..." : "Play Streaming"}
      </button>
    </div>
  </header>

  {#if activeTab === "rsl"}
    <div style="flex: 1; display: flex; overflow: hidden;">
      <!-- Left Pane: Editors -->
      <div
        style="width: 40%; height: 100%; border-right: 1px solid #e5e7eb; display: flex; flex-direction: column;"
      >
        <!-- RSL Editor -->
        <div
          style="flex: 1; display: flex; flex-direction: column; border-bottom: 1px solid #e5e7eb;"
        >
          <div
            style="padding: 0.5rem; background: #f9fafb; border-bottom: 1px solid #e5e7eb; font-family: monospace; font-size: 0.875rem; font-weight: bold;"
          >
            RSL (Style)
          </div>
          <textarea
            style="flex: 1; padding: 1rem; font-family: monospace; resize: none; border: none; outline: none;"
            bind:value={rslInput}
          ></textarea>
        </div>
        <!-- RSS Editor -->
        <div style="flex: 1; display: flex; flex-direction: column;">
          <div
            style="padding: 0.5rem; background: #f9fafb; border-bottom: 1px solid #e5e7eb; font-family: monospace; font-size: 0.875rem; font-weight: bold;"
          >
            Content (RTD)
          </div>
          <textarea
            style="flex: 1; padding: 1rem; font-family: monospace; resize: none; border: none; outline: none;"
            bind:value={rssInput}
          ></textarea>
        </div>
      </div>

      <!-- Right Pane: Preview -->
      <div
        style="flex: 1; display: flex; flex-direction: column; background: #fff;"
      >
        <div
          style="padding: 0.5rem; background: #f9fafb; border-bottom: 1px solid #e5e7eb; font-family: monospace; font-size: 0.875rem; font-weight: bold;"
        >
          Preview
        </div>
        <div style="flex: 1; overflow: auto; padding: 2rem;">
          <div class="prose">
            {@html rslHtml}
          </div>
        </div>
      </div>
    </div>
  {:else}
    <div style="flex: 1; display: flex; overflow: hidden;">
      <!-- Left Pane: Input -->
      <div
        style="width: 50%; height: 100%; border-right: 1px solid #e5e7eb; display: flex; flex-direction: column;"
      >
        <div
          style="padding: 0.5rem; background: #f9fafb; border-bottom: 1px solid #e5e7eb; font-family: monospace; font-size: 0.875rem;"
        >
          Markdown Input
        </div>
        <textarea
          style="flex: 1; padding: 1rem; font-family: monospace; resize: none; border: none; outline: none;"
          bind:value={input}
          disabled={isStreaming}
        ></textarea>
      </div>

      <!-- Right Pane: Output -->
      <div
        style="width: 50%; height: 100%; display: flex; flex-direction: column;"
      >
        <div
          style="display: flex; background: #f9fafb; border-bottom: 1px solid #e5e7eb;"
        >
          <button
            style="padding: 0.5rem 1rem; border: none; background: {activeTab ===
            'preview'
              ? '#fff'
              : 'transparent'}; border-right: 1px solid #e5e7eb; cursor: pointer; font-weight: {activeTab ===
            'preview'
              ? 'bold'
              : 'normal'};"
            onclick={() => (activeTab = "preview")}
          >
            Preview
          </button>
          <button
            style="padding: 0.5rem 1rem; border: none; background: {activeTab ===
            'ast'
              ? '#fff'
              : 'transparent'}; border-right: 1px solid #e5e7eb; cursor: pointer; font-weight: {activeTab ===
            'ast'
              ? 'bold'
              : 'normal'};"
            onclick={() => (activeTab = "ast")}
          >
            AST
          </button>
        </div>

        <div style="flex: 1; overflow: auto; padding: 1rem;">
          {#if activeTab === "preview"}
            <div class="prose">
              {@html html}
            </div>
          {:else}
            <div
              style="font-family: monospace; font-size: 0.75rem; white-space: pre;"
            >
              {JSON.stringify(ast, null, 2)}
            </div>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    font-family:
      system-ui,
      -apple-system,
      sans-serif;
  }

  /* Basic typography for preview */
  .prose :global(h1) {
    font-size: 2em;
    font-weight: bold;
    margin-bottom: 0.5em;
  }
  .prose :global(h2) {
    font-size: 1.5em;
    font-weight: bold;
    margin-bottom: 0.5em;
  }
  .prose :global(p) {
    margin-bottom: 1em;
  }
  .prose :global(code) {
    background: #eee;
    padding: 0.2em 0.4em;
    border-radius: 3px;
  }
  .prose :global(pre) {
    background: #f4f4f4;
    padding: 1em;
    overflow-x: auto;
  }
  .prose :global(blockquote) {
    border-left: 4px solid #ddd;
    margin: 0;
    padding-left: 1em;
    color: #666;
  }

  .prose :global(.math-block) {
    background: #f4f4f4;
    padding: 1em;
    overflow-x: auto;
    font-family: monospace;
    margin-bottom: 1em;
  }
  .prose :global(.math-inline) {
    background: #f4f4f4;
    padding: 0.2em 0.4em;
    border-radius: 3px;
    font-family: monospace;
  }
  .prose :global(.citation) {
    color: #666;
    font-size: 0.8em;
    vertical-align: super;
  }

  /* Alert styles */
  .prose :global(.rtd-variant-note) {
    border-left: 4px solid #0969da;
    background: #ddf4ff;
    padding: 1em;
    margin-bottom: 1em;
  }
  .prose :global(.rtd-variant-tip) {
    border-left: 4px solid #1a7f37;
    background: #dafbe1;
    padding: 1em;
    margin-bottom: 1em;
  }
  .prose :global(.rtd-variant-important) {
    border-left: 4px solid #8250df;
    background: #f6f8fa;
    padding: 1em;
    margin-bottom: 1em;
  }
  .prose :global(.rtd-variant-warning) {
    border-left: 4px solid #9a6700;
    background: #fff8c5;
    padding: 1em;
    margin-bottom: 1em;
  }
  .prose :global(.rtd-variant-caution) {
    border-left: 4px solid #d1242f;
    background: #ffebe9;
    padding: 1em;
    margin-bottom: 1em;
  }

  /* Custom container styles */
  .prose :global(.rtd-variant-custom-container) {
    border: 2px dashed #666;
    background: #f0f0f0;
    padding: 1em;
    margin-bottom: 1em;
    border-radius: 8px;
  }
</style>
