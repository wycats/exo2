# Vision: The Exosuit Development Kit (EDK)

**"Extensions for the Rest of Us."**

## The Problem: The "Extension Cliff"

Today, building a VS Code extension is a tale of two cities:

1.  **The "Hello World" City**: You run `yo code`. You get a simple command that shows a notification. It's easy. It's fun.
2.  **The "Real App" City**: You want a Webview. You want React/Svelte. You want state management. You want to test it.
    - Suddenly, you are battling Webpack/Vite configs to bundle two different environments (Node.js vs Browser).
    - You are inventing your own RPC protocol to pass messages.
    - You are struggling to sync state between the extension host and the UI.
    - You are writing flaky integration tests because the VS Code test harness is arcane.

Most developers fall off the cliff between City 1 and City 2. They stick to simple snippets or give up.

## The Solution: The Exosuit Development Kit (EDK)

The EDK is a **batteries-included framework** for building "App-Like" VS Code extensions. It takes the sophisticated architecture we built for Exosuit—Reactivity, Webviews, Testing—and packages it into a cohesive SDK.

### The World with EDK

Imagine you have an idea: _"I want a VS Code extension that visualizes my SQL database schema."_

#### 1. The Setup (Zero Config)

You run:

```bash
npm init @exosuit/extension my-sql-viz
```

You don't get a bare folder. You get:

- **A Monorepo Structure**: Ready for scale.
- **The Build Chain**: `vite` for UI, `esbuild` for Host. Pre-wired.
- **The Test Harness**: Playwright configured to launch VS Code and click your buttons.

#### 2. The Development (Reactive by Default)

You don't write `webview.postMessage({ type: 'update' })`. You write:

```typescript
// Extension Host
const schema = new Signal(db.getSchema());
const activeTable = new Signal(null);

// The "Bridge" automatically syncs these signals to the Webview
return new ExosuitWebview(context, { schema, activeTable });
```

In your Svelte/React component:

```svelte
<!-- Webview -->
<script>
  import { useSignal } from '@exosuit/edk';
  const schema = useSignal('schema');
</script>

<h1>{schema.name}</h1>
```

**The Magic**: When the database changes on disk, the `schema` signal updates. The UI re-renders instantly. No message passing code. No state drift.

#### 3. The UI (Native Feel)

You don't hunt for CSS classes to match VS Code's theme. You use EDK components:

```svelte
<ExoPage>
  <ExoHeader title="Database Schema" />
  <ExoList items={tables} on:select={openTable} />
</ExoPage>
```

It looks exactly like VS Code. It respects the user's theme. It handles accessibility.

#### 4. The Testing (Confidence)

You don't manually click around. You write a spec:

```typescript
test("clicking a table opens the details", async ({ page }) => {
  await page.click("text=UsersTable");
  await expect(page.locator("h1")).toHaveText("UsersTable Schema");
});
```

The EDK handles the nightmare of launching VS Code, waiting for activation, and connecting the debugger.

## The Impact

**For the Experienced Developer**:

- **Eliminate Boilerplate**: You stop maintaining build scripts and start writing features.
- **Robustness**: You get "Glitch-Free" state management for free via the Reactivity Engine.
- **Standardization**: All your extensions share a common architecture, making maintenance easy.

**For the Newcomer**:

- **Empowerment**: You can build "Real Apps" inside VS Code without needing to be an expert in RPC, Bundling, or VS Code Internals.
- **Focus**: You focus on _your_ domain (SQL, AI, Notes), not the plumbing.

## The "Exosuit Way"

The EDK isn't just code; it's a philosophy.

- **State First**: Define your state; the UI follows.
- **Context is King**: The extension knows where it is and what it's doing.
- **Quality is Default**: Testing isn't an afterthought; it's the default path.

We are democratizing the power of the "Exosuit" architecture. We are building the **Rails for VS Code Extensions**.
