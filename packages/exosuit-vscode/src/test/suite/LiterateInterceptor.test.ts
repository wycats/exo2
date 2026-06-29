import { describe, it, beforeEach } from "./harness.js";
import * as assert from "assert";
import * as vscode from "vscode";
import { LiterateInterceptor } from "../../agent/LiterateInterceptor";

// Mock VS Code Stream
class MockChatResponseStream implements vscode.ChatResponseStream {
  public output: string = "";
  public parts: any[] = [];

  markdown(value: string | vscode.MarkdownString): void {
    const text = typeof value === "string" ? value : value.value;
    this.output += text;
    this.parts.push({ type: "markdown", value: text });
  }

  progress(value: string): void {
    this.parts.push({ type: "progress", value });
  }

  push(part: vscode.ChatResponsePart): void {
    this.parts.push(part);
  }

  reference(value: vscode.Uri | vscode.Location): void {
    this.parts.push({ type: "reference", value });
  }

  text(value: string): void {
    this.output += value;
    this.parts.push({ type: "text", value });
  }

  anchor(value: vscode.Uri | vscode.Location, title?: string): void {
    this.parts.push({ type: "anchor", value, title });
  }

  button(command: vscode.Command): void {
    this.parts.push({ type: "button", command });
  }

  filetree(tree: vscode.ChatResponseFileTree[], baseUri: vscode.Uri): void {
    this.parts.push({ type: "filetree", tree, baseUri });
  }
}

// Mock Workspace Cache
class MockWorkspaceCache {
  hasFile(_path: string): boolean {
    return false;
  }
  hasDirectory(_path: string): boolean {
    return false;
  }
  isInitialized(): boolean {
    return true;
  }
  onDidInitialize = new vscode.EventEmitter<void>().event;
  dispose() {}
}

describe("LiterateInterceptor Spec v2.1.0", () => {
  let stream: MockChatResponseStream;
  let interceptor: LiterateInterceptor;
  let cache: MockWorkspaceCache;

  beforeEach(() => {
    stream = new MockChatResponseStream();
    cache = new MockWorkspaceCache();
    interceptor = new LiterateInterceptor(
      stream as any,
      cache as any,
      async () => {}
    );
  });

  it("Text State: Passes through plain text", () => {
    interceptor.feed("Hello world");
    interceptor.close();
    assert.strictEqual(stream.output, "Hello world");
  });

  it("Reference Resolver: Linkifies known files", () => {
    cache.hasFile = (p) => p === "src/utils.ts";

    interceptor.feed("Check src/utils.ts for details.");
    interceptor.close();

    assert.strictEqual(
      stream.output,
      "Check [src/utils.ts](src/utils.ts) for details."
    );
  });

  it("Reference Resolver: Does not linkify unknown files", () => {
    cache.hasFile = (_p) => false;

    interceptor.feed("Check src/unknown.ts for details.");
    interceptor.close();

    assert.strictEqual(stream.output, "Check src/unknown.ts for details.");
  });

  it("Reference Resolver: Does not linkify inside code blocks", () => {
    cache.hasFile = (p) => p === "src/utils.ts";

    interceptor.feed("Use `src/utils.ts` in your code.");
    interceptor.close();

    assert.strictEqual(stream.output, "Use `src/utils.ts` in your code.");
  });

  it("Fence State: Ignores tags inside code blocks", () => {
    interceptor.feed(
      'Start\n```\n<exo-tool name="foo">{}</exo-tool>\n```\nEnd'
    );
    interceptor.close();
    assert.ok(
      stream.output.includes("<exo-tool"),
      "Should contain the raw tag"
    );
    assert.ok(
      !stream.parts.some((p) => p.type === "progress"),
      "Should not execute tool"
    );
  });

  it("Cell State: Parses JSON content for arguments", async () => {
    let toolArgs: any = null;
    interceptor = new LiterateInterceptor(
      stream as any,
      cache as any,
      async (_name, args) => {
        toolArgs = args;
      }
    );

    interceptor.feed('<exo-tool name="test">{"key": "value"}</exo-tool>');
    interceptor.close(); // Triggers flush

    await interceptor.waitForTools();
    assert.deepStrictEqual(toolArgs, { key: "value" });
  });

  it("Adjacency Heuristic: Batches tools separated by whitespace", async () => {
    let callCount = 0;
    interceptor = new LiterateInterceptor(
      stream as any,
      cache as any,
      async (_name) => {
        callCount++;
      }
    );

    interceptor.feed('<exo-tool name="t1">{}</exo-tool>');
    interceptor.feed("   \n   "); // Whitespace
    interceptor.feed('<exo-tool name="t2">{}</exo-tool>');

    // Wait a tick to ensure no async execution happened
    await new Promise((r) => setTimeout(r, 0));
    assert.strictEqual(callCount, 0, "Tools should be buffered");

    // Now feed text
    interceptor.feed("Text");

    // Should trigger flush
    await interceptor.waitForTools();
    assert.strictEqual(callCount, 2, "Tools should execute after text barrier");
  });

  it("Adjacency Heuristic: Flushes on close", async () => {
    let callCount = 0;
    interceptor = new LiterateInterceptor(
      stream as any,
      cache as any,
      async (_name) => {
        callCount++;
      }
    );

    interceptor.feed('<exo-tool name="t1">{}</exo-tool>');

    await new Promise((r) => setTimeout(r, 0));
    assert.strictEqual(callCount, 0, "Tools should be buffered");

    interceptor.close();

    await interceptor.waitForTools();
    assert.strictEqual(callCount, 1, "Tools should execute on close");
  });

  it("Surrogate Handling: Buffers split surrogates", () => {
    const emoji = "🧪"; // High: D83E, Low: DDEA
    const high = emoji[0];
    const low = emoji[1];

    interceptor.feed("Science " + high);
    // Should not emit the high surrogate yet
    assert.strictEqual(stream.output, "Science ");

    interceptor.feed(low + " Rules");
    interceptor.close();
    assert.strictEqual(stream.output, "Science 🧪 Rules");
  });

  it("Adjacency Heuristic: Batches tools separated by XML comments (Chain of Thought)", async () => {
    let callCount = 0;
    interceptor = new LiterateInterceptor(
      stream as any,
      cache as any,
      async (_name) => {
        callCount++;
      }
    );

    interceptor.feed('<exo-tool name="t1">{}</exo-tool>');
    interceptor.feed("\n<!-- Thinking about the next step -->\n");
    interceptor.feed('<exo-tool name="t2">{}</exo-tool>');

    // Wait a tick to ensure no async execution happened
    await new Promise((r) => setTimeout(r, 0));
    assert.strictEqual(
      callCount,
      0,
      "Tools should be buffered across comments"
    );

    // Verify comment is emitted
    assert.ok(
      stream.output.includes("Thinking about"),
      "Comment should be emitted"
    );

    // Now feed text barrier
    interceptor.feed("Execution Barrier");

    // Should trigger flush
    await interceptor.waitForTools();
    assert.strictEqual(callCount, 2, "Tools should execute after text barrier");
  });

  it("Compat: Supports <tool_code>", async () => {
    let callCount = 0;
    let toolName = "";
    interceptor = new LiterateInterceptor(
      stream as any,
      cache as any,
      async (name) => {
        callCount++;
        toolName = name;
      }
    );

    const compatTag = '<tool_code> { "name": "listDirectory" } </tool_code>';
    interceptor.feed(compatTag);
    interceptor.close();

    await interceptor.waitForTools();
    assert.strictEqual(callCount, 1, "Should execute tool_code");
    assert.strictEqual(toolName, "listDirectory", "Should parse tool name");
  });
});
