import * as vscode from "vscode";
import type { IWorkspaceCache } from "../WorkspaceCache";
import type { RTDBlock, XmlBlock } from "@exosuit/rtd";
import { StreamingParser, serializeBlock } from "@exosuit/rtd";

export class LiterateInterceptor {
  private parser = new StreamingParser();
  private lastRenderedBlockIndex = 0;
  private lastRenderedContentLength = 0;

  private textBuffer = ""; // For adjacency heuristic

  private pendingTools: Promise<void>[] = [];
  private pendingBatch: { name: string; args: any }[] = [];
  private toolOutputs: { name: string; output: string }[] = [];

  private onToolCall: (
    name: string,
    args: any,
    stream: vscode.ChatResponseStream,
  ) => Promise<string | void | undefined>;

  constructor(
    private readonly vsStream: vscode.ChatResponseStream,
    private readonly workspaceCache: IWorkspaceCache,
    toolCallback: (
      name: string,
      args: any,
      stream: vscode.ChatResponseStream,
    ) => Promise<string | void | undefined>,
  ) {
    this.onToolCall = toolCallback;
  }

  public getToolOutputs() {
    return this.toolOutputs;
  }

  public feed(chunk: string) {
    const blocks = this.parser.parse(chunk);
    this.renderBlocks(blocks);
  }

  public close() {
    const blocks = this.parser.flush();
    this.renderBlocks(blocks, true);
    this.flushTextBuffer();
    this.flushBatch();
  }

  public async waitForTools() {
    await Promise.all(this.pendingTools);
  }

  private renderBlocks(blocks: RTDBlock[], isFinal = false) {
    for (let i = this.lastRenderedBlockIndex; i < blocks.length; i++) {
      const block = blocks[i];
      const isLast = i === blocks.length - 1;
      const isBlockFinal = !isLast || isFinal || (this.parser.isIdle && isLast);

      if (i > this.lastRenderedBlockIndex) {
        // Moved to a new block. Reset content length.
        this.lastRenderedContentLength = 0;
        this.lastRenderedBlockIndex = i;
      }

      if (block.kind === "xml-block") {
        this.handleXmlBlock(block, isBlockFinal);
      } else {
        this.handleMarkdownBlock(block, isBlockFinal);
      }

      if (isBlockFinal) {
        this.lastRenderedBlockIndex = i + 1;
        this.lastRenderedContentLength = 0;
      }
    }
  }

  private handleXmlBlock(block: XmlBlock, isFinal: boolean) {
    if (!isFinal) {
      // Check if we already showed progress?
      if (this.lastRenderedContentLength === 0) {
        if (block.tagName === "tool" || block.tagName === "tool_code") {
          const name =
            block.attributes.name ||
            (block.tagName === "tool_code" ? "tool_code" : "tool");
          this.vsStream.progress(`Parsing ${name}...`);
        }
        this.lastRenderedContentLength = 1; // Mark as "progress shown"
      }
      return;
    }

    // Block is final. Execute.
    this.executeXmlBlock(block);
    this.lastRenderedContentLength = block.content.length; // Just to be safe
  }

  private executeXmlBlock(block: XmlBlock) {
    const tagName = block.tagName;
    const attributes = block.attributes;
    const content = block.content;

    try {
      switch (tagName) {
        case "tool": {
          let args = {};
          try {
            if (content.trim()) {
              args = JSON.parse(content);
            }
          } catch (e) {
            this.emitText(`\n> Error parsing tool arguments JSON: ${e}\n`);
            return;
          }
          const name = attributes.name;
          if (name) {
            this.pendingBatch.push({ name, args });
            const labelMap: Record<string, string> = {
              read_file: "Reading file",
              list_files: "Scanning directory",
              edit_file: "Applying edits",
              open_file: "Opening file",
            };
            const label = labelMap[name] || name;
            this.vsStream.progress(`Buffered ${label}...`);
          } else {
            this.emitText(`\n> Error: Tool name missing in tool tag.\n`);
          }
          break;
        }
        case "tool_code": {
          try {
            const payload = JSON.parse(content);
            const name = payload.name;
            const args = payload.arguments || payload.args || {};
            if (name) {
              this.pendingBatch.push({ name, args });
              this.vsStream.progress(`Buffered ${name} (compat)...`);
            } else {
              this.emitText(`\n> Error: Tool name missing in tool_code.\n`);
            }
          } catch (e) {
            this.emitText(`\n> Error parsing tool_code JSON: ${e}\n`);
          }
          break;
        }
        case "diff": {
          this.flushTextBuffer(); // Ensure text before diff is emitted
          const md = new vscode.MarkdownString(
            `\n**Suggested Edit for [${attributes.path}](${attributes.path})**\n\`\`\`typescript\n${content}\n\`\`\`\n`,
          );
          md.supportHtml = true;
          this.vsStream.markdown(md);
          break;
        }
        case "progress": {
          this.vsStream.push(
            new vscode.ChatResponseProgressPart(content.trim()),
          );
          break;
        }
        case "cmd": {
          this.vsStream.push(
            new vscode.ChatResponseCommandButtonPart({
              title: content.trim(),
              command: attributes.id,
              arguments: attributes.args ? JSON.parse(attributes.args) : [],
            }),
          );
          break;
        }
        case "link": {
          const uri = vscode.Uri.file(attributes.path);
          this.vsStream.push(
            new vscode.ChatResponseCommandButtonPart({
              title: content.trim() || "Open File",
              command: "vscode.open",
              arguments: [uri],
            }),
          );
          break;
        }
        case "tree": {
          try {
            const items = JSON.parse(content);
            const baseUri = vscode.Uri.file(attributes.root || "/");
            this.vsStream.push(
              new vscode.ChatResponseFileTreePart(items, baseUri),
            );
          } catch (e) {
            this.emitText(`\n> Error parsing tree JSON: ${e}\n`);
          }
          break;
        }
        case "ref": {
          this.renderRef(attributes);
          break;
        }
      }
    } catch (e) {
      this.emitText(
        `\n> Error rendering ${tagName}: ${
          e instanceof Error ? e.message : String(e)
        }\n`,
      );
    }
  }

  private handleMarkdownBlock(block: RTDBlock, _isFinal: boolean) {
    const fullText = serializeBlock(block);
    const newText = fullText.slice(this.lastRenderedContentLength);

    if (newText) {
      this.emitText(newText);
      this.lastRenderedContentLength += newText.length;
    }
  }

  private emitText(text: string) {
    if (this.pendingBatch.length > 0) {
      if (!this.isIgnorable(text)) {
        this.flushBatch();
      }
    }

    this.textBuffer += text;
    // Smoothing: Only flush on safe boundaries or length
    if (/[\s\.\,\;\!\?\n>]$/.test(text) || this.textBuffer.length > 500) {
      this.flushTextBuffer();
    }
  }

  private isIgnorable(text: string): boolean {
    const ignorableRegex = /^(\s*(<!--[\s\S]*?-->)?\s*)*$/;
    return ignorableRegex.test(text);
  }

  private flushBatch() {
    if (this.pendingBatch.length === 0) {
      return;
    }

    const batch = [...this.pendingBatch];
    this.pendingBatch = [];

    const promises = batch.map(async (tool) => {
      try {
        const output = await this.onToolCall(
          tool.name,
          tool.args,
          this.vsStream,
        );
        if (typeof output === "string") {
          this.toolOutputs.push({ name: tool.name, output });
        }
      } catch (e) {
        this.toolOutputs.push({ name: tool.name, output: `Error: ${e}` });
      }
    });

    const batchPromise = Promise.all(promises).then(() => {});
    this.pendingTools.push(batchPromise);

    batchPromise.finally(() => {
      const index = this.pendingTools.indexOf(batchPromise);
      if (index > -1) {
        this.pendingTools.splice(index, 1);
      }
    });
  }

  private flushTextBuffer() {
    if (this.textBuffer.length > 0) {
      const linkedText = this.linkify(this.textBuffer);
      const md = new vscode.MarkdownString(linkedText);
      md.supportHtml = true;
      this.vsStream.markdown(md);
      this.textBuffer = "";
    }
  }

  private linkify(text: string): string {
    const tokenRegex =
      /(`[^`]*`)|(\[[^\]]*\]\([^\)]*\))|([a-zA-Z0-9_\-\./\\]+)/g;

    return text.replace(tokenRegex, (match, code, link, candidate) => {
      if (code) {
        return code;
      }
      if (link) {
        return link;
      }
      if (candidate) {
        if (
          this.workspaceCache.hasFile(candidate) ||
          this.workspaceCache.hasDirectory(candidate)
        ) {
          return `[${candidate}](${candidate})`;
        }
        return candidate;
      }
      return match;
    });
  }

  private renderRef(attrs: Record<string, string>) {
    if (!attrs.path) {
      return;
    }
    const uri = vscode.Uri.file(attrs.path);
    this.vsStream.push(new vscode.ChatResponseReferencePart(uri));
  }
}
