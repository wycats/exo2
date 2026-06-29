import type { RTDBlock, RTDInline } from "../dom/types.ts";
import { BlockState, InlineState, type ParserState } from "./state-machine.js";
import { splitTail } from "./buffer.js";

export class StreamingParser {
  private state: ParserState;
  private blocks: RTDBlock[] = [];
  private currentBlock: any = null;
  private listStack: { block: any; indent: number }[] = [];

  constructor() {
    this.state = {
      blockState: BlockState.ROOT,
      inlineState: InlineState.TEXT,
      buffer: [],
      tailBuffer: "",
      stack: [],
    };
  }

  public get isIdle(): boolean {
    return this.state.blockState === BlockState.ROOT;
  }

  public parse(chunk: string): RTDBlock[] {
    // Prepend tail buffer from previous chunk
    const input = this.state.tailBuffer + chunk;
    const [safePart, newTail] = splitTail(input);
    this.state.tailBuffer = newTail;

    if (safePart.length > 0) {
      // LLM Normalization
      const normalized = this.normalize(safePart);
      const lines = normalized.split(/\r?\n/);

      for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        if (
          i === lines.length - 1 &&
          line === "" &&
          normalized.endsWith("\n")
        ) {
          continue;
        }
        this.processLine(line);
      }
    }

    // Speculative Parsing for Tail (Eager Paragraph)
    if (this.state.tailBuffer.length > 0) {
      if (this.state.blockState === BlockState.PARAGRAPH) {
        // We are in a paragraph. Eagerly render the tail.
        const text = [...this.state.buffer, this.state.tailBuffer].join("\n");
        this.currentBlock.children = this.parseInlines(text);
      } else if (this.state.blockState === BlockState.ROOT) {
        // Check if tail looks like a block starter
        if (!this.isPotentialBlockStarter(this.state.tailBuffer)) {
          // It's likely a paragraph. Start it eagerly.
          this.state.blockState = BlockState.PARAGRAPH;
          this.currentBlock = {
            kind: "paragraph",
            children: [],
          };
          this.blocks.push(this.currentBlock);
          this.currentBlock.children = this.parseInlines(this.state.tailBuffer);
          // Note: We do NOT push to state.buffer yet, because processLine will do that
          // when the line is complete.
        } else {
          // It IS a potential block starter.
          // Check for complete XML block in tail
          const trimmed = this.state.tailBuffer.trimStart();
          if (trimmed.startsWith("<exo-") || trimmed.startsWith("<tool_code")) {
            const match = trimmed.match(/^<(exo-[a-zA-Z0-9-]+|tool_code)/);
            if (match) {
              const fullTagName = match[1];
              const tagName = fullTagName.startsWith("exo-")
                ? fullTagName.slice(4)
                : fullTagName;
              const closeTag =
                tagName === "tool_code" ? "</tool_code>" : `</exo-${tagName}>`;

              const closeIdx = this.state.tailBuffer.indexOf(closeTag);
              if (closeIdx !== -1) {
                // We have a complete block!
                this.processLine(this.state.tailBuffer);
                this.state.tailBuffer = "";
              }
            }
          }
        }
      }
    }

    return this.blocks;
  }

  private isPotentialBlockStarter(text: string): boolean {
    // Check for prefixes that could start a block
    if (text.startsWith("#")) return true;
    if (text.startsWith(">")) return true;
    if (text.startsWith("`")) return true; // Code block
    if (text.startsWith("<")) return true; // XML or HTML comment
    if (text.startsWith("-") || text.startsWith("*")) return true; // List
    if (/^\d/.test(text)) return true; // Ordered list
    if (text.startsWith(" ")) return true; // Indented code
    if (text.startsWith("\t")) return true;
    if (text.startsWith("$$")) return true; // Block Math
    if (text.startsWith("\\[")) return true; // Block Math (LLM)
    return false;
  }

  private normalize(text: string): string {
    return text
      .replace(/\\\[/g, "$$$$") // Block Math (needs $$$$ to produce $$)
      .replace(/\\\]/g, "$$$$")
      .replace(/\\\(/g, "$") // Inline Math
      .replace(/\\\)/g, "$")
      .replace(/【(.*?)】/g, (_match, p1) => `\u0000CITATION${p1}\u0000`); // Ghost Citations marker
  }

  public flush(): RTDBlock[] {
    if (this.state.tailBuffer.length > 0) {
      this.processLine(this.normalize(this.state.tailBuffer));
      this.state.tailBuffer = "";
    }
    this.finalizeBlock();
    return this.blocks;
  }

  private processLine(line: string) {
    switch (this.state.blockState) {
      case BlockState.ROOT:
        this.handleRoot(line);
        break;
      case BlockState.CODE_BLOCK_BODY:
        this.handleCodeBlock(line);
        break;
      case BlockState.CODE_BLOCK_INDENTED:
        this.handleCodeBlockIndented(line);
        break;
      case BlockState.PARAGRAPH:
        this.handleParagraph(line);
        break;
      case BlockState.BLOCKQUOTE:
        this.handleBlockquote(line);
        break;
      case BlockState.COMMENT:
        this.handleComment(line);
        break;
      case BlockState.XML_BLOCK:
        this.handleXmlBlock(line);
        break;
      case BlockState.CONTAINER:
        this.handleContainer(line);
        break;
      case BlockState.TABLE:
        this.handleTable(line);
        break;
    }
  }

  private handleRoot(line: string) {
    // List Item (- or * or 1.)
    // Check first to handle nesting vs code block ambiguity
    const listMatch = line.match(/^(\s*)([-*]|\d+\.)\s+(.*)/);
    const indent = listMatch ? listMatch[1].length : 0;

    if (listMatch && (indent < 4 || this.listStack.length > 0)) {
      const marker = listMatch![2];
      const content = listMatch![3];
      const ordered = /^\d+\./.test(marker);

      // Find the correct parent list based on indentation
      while (
        this.listStack.length > 0 &&
        this.listStack[this.listStack.length - 1].indent >= indent
      ) {
        if (this.listStack[this.listStack.length - 1].indent === indent) {
          break;
        }
        this.listStack.pop();
      }

      let currentListCtx = this.listStack[this.listStack.length - 1];
      let listBlock: any;

      if (currentListCtx && currentListCtx.indent === indent) {
        if (currentListCtx.block.ordered !== ordered) {
          this.listStack.pop();
          currentListCtx = undefined as any;
        } else {
          listBlock = currentListCtx.block;
        }
      }

      if (!listBlock) {
        listBlock = {
          kind: "list",
          ordered,
          items: [],
        };

        if (currentListCtx) {
          const parentList = currentListCtx.block;
          const lastItem = parentList.items[parentList.items.length - 1];
          if (lastItem) {
            lastItem.children.push(listBlock);
          } else {
            this.blocks.push(listBlock);
          }
        } else {
          this.blocks.push(listBlock);
        }
        this.listStack.push({ block: listBlock, indent });
      }

      (listBlock as any).items.push({
        children: [
          {
            kind: "paragraph",
            children: this.parseInlines(content),
          },
        ],
      });
      return;
    }

    // Not a list item. Clear stack.
    if (line.trim() !== "") {
      this.listStack = [];
    }

    // Indented Code Block (4 spaces or 1 tab)
    if (line.match(/^( {4}|\t)/)) {
      this.state.blockState = BlockState.CODE_BLOCK_INDENTED;
      this.currentBlock = {
        kind: "code-block",
        value: line.replace(/^( {4}|\t)/, ""),
      };
      this.blocks.push(this.currentBlock); // Eager emit
      return;
    }

    // Block Math ($$ ... $$)
    const mathMatch = line.match(/^\$\$(.*)\$\$$/);
    if (mathMatch) {
      this.blocks.push({
        kind: "math-block",
        value: mathMatch[1],
      });
      return;
    }

    // HTML Comment Block (<!-- ...)
    if (line.trimStart().startsWith("<!--")) {
      this.state.blockState = BlockState.COMMENT;
      this.currentBlock = {
        kind: "comment",
        value: "",
        nestingLevel: 0,
      };
      this.blocks.push(this.currentBlock);
      this.handleComment(line);
      return;
    }

    // XML Block (<exo-...)
    const xmlMatch = line
      .trimStart()
      .match(/^<(exo-[a-zA-Z0-9-]+|tool_code)([\s\S]*)/);
    if (xmlMatch) {
      const fullTagName = xmlMatch[1]; // exo-tool or tool_code
      const tagName = fullTagName.startsWith("exo-")
        ? fullTagName.slice(4)
        : fullTagName;

      // Check if `>` is in this line
      const closeIdx = line.indexOf(">");
      if (closeIdx !== -1) {
        // Opening tag ends here.
        const attrStr = line.slice(
          line.indexOf(fullTagName) + fullTagName.length,
          closeIdx,
        );
        const attributes = this.parseAttributes(attrStr);

        this.currentBlock = {
          kind: "xml-block",
          tagName,
          attributes,
          content: "",
        };
        this.blocks.push(this.currentBlock);
        this.state.blockState = BlockState.XML_BLOCK;

        // Process rest of line as content
        const rest = line.slice(closeIdx + 1);
        if (rest) {
          // Check if closing tag is ALSO on this line
          const closeTag =
            tagName === "tool_code" ? "</tool_code>" : `</exo-${tagName}>`;
          const restCloseIdx = rest.indexOf(closeTag);

          if (restCloseIdx !== -1) {
            this.currentBlock.content = rest.slice(0, restCloseIdx);
            this.currentBlock = null;
            this.state.blockState = BlockState.ROOT;
            // Recurse for rest?
            const afterClose = rest.slice(restCloseIdx + closeTag.length);
            if (afterClose.trim()) {
              this.handleRoot(afterClose);
            }
            return;
          }

          this.currentBlock.content = rest;
        }
        return;
      } else {
        // Multiline opening tag? Not supported by this simple logic yet.
        // Treat as text?
      }
    }

    // Container Block (::: variant)
    const containerMatch = line.match(/^(:{3,})\s*([a-zA-Z0-9-]+)/);
    if (containerMatch) {
      this.state.blockState = BlockState.CONTAINER;
      this.currentBlock = {
        kind: "container",
        variant: containerMatch[2],
        fenceLength: containerMatch[1].length,
        children: [],
      };
      this.blocks.push(this.currentBlock);
      return;
    }

    const codeBlockMatch = line.match(/^(`{3,})(.*)/);
    if (codeBlockMatch) {
      this.state.blockState = BlockState.CODE_BLOCK_BODY;
      const lang = codeBlockMatch[2].trim();
      this.currentBlock = {
        kind: "code-block",
        language: lang || undefined,
        value: "",
        fenceLength: codeBlockMatch[1].length,
        nestingLevel: 0,
      };
      this.blocks.push(this.currentBlock); // Eager emit
      return;
    }

    if (line.startsWith("#")) {
      const match = line.match(/^(#{1,6})\s+(.*)/);
      if (match) {
        this.blocks.push({
          kind: "heading",
          level: match[1].length as any,
          children: this.parseInlines(match[2]),
        });
        return;
      }
    }

    if (line.startsWith(">")) {
      this.state.blockState = BlockState.BLOCKQUOTE;
      this.state.buffer.push(line.slice(1).trim());
      return;
    }

    if (line.trim() === "") {
      return;
    }

    this.state.blockState = BlockState.PARAGRAPH;

    // Eager create
    this.currentBlock = { kind: "paragraph", children: [] };
    this.blocks.push(this.currentBlock);

    this.state.buffer.push(line);
    this.currentBlock.children = this.parseInlines(
      this.state.buffer.join("\n"),
    );
  }

  private handleCodeBlock(line: string) {
    const match = line.match(/^(`{3,})(.*)/);
    if (match) {
      const fenceLength = match[1].length;
      const infoString = match[2].trim();

      // Check for nested start fence
      if (infoString !== "") {
        this.currentBlock.nestingLevel =
          (this.currentBlock.nestingLevel || 0) + 1;
      }
      // Heuristic: If fence is generic (```) but previous line ended with ':', treat as start
      else if (this.currentBlock.value.trimEnd().endsWith(":")) {
        this.currentBlock.nestingLevel =
          (this.currentBlock.nestingLevel || 0) + 1;
      }
      // Check for closing fence
      else if (fenceLength >= (this.currentBlock.fenceLength || 3)) {
        if ((this.currentBlock.nestingLevel || 0) > 0) {
          this.currentBlock.nestingLevel--;
        } else {
          // Real closing fence
          const { fenceLength: _fenceLength, nestingLevel: _nestingLevel } =
            this.currentBlock;
          delete this.currentBlock.fenceLength;
          delete this.currentBlock.nestingLevel;

          this.currentBlock = null;
          this.state.blockState = BlockState.ROOT;
          return;
        }
      }
    }

    this.currentBlock.value += (this.currentBlock.value ? "\n" : "") + line;
  }

  private handleCodeBlockIndented(line: string) {
    if (line.match(/^( {4}|\t)/)) {
      this.currentBlock.value += "\n" + line.replace(/^( {4}|\t)/, "");
    } else if (line.trim() === "") {
      this.currentBlock.value += "\n";
    } else {
      // Block is already in this.blocks
      this.currentBlock = null;
      this.state.blockState = BlockState.ROOT;
      this.handleRoot(line);
    }
  }

  private handleParagraph(line: string) {
    if (line.trim() === "") {
      this.finalizeBlock();
      this.state.blockState = BlockState.ROOT;
      return;
    }

    // Check for block interruptions
    if (
      line.startsWith("```") ||
      line.startsWith("#") ||
      line.startsWith(">") ||
      /^(\s*)([-*]|\d+\.)\s+/.test(line) // List marker
    ) {
      this.finalizeBlock();
      this.state.blockState = BlockState.ROOT;
      this.handleRoot(line);
      return;
    }

    // Check for Table Delimiter Row
    // Matches | --- | --- | or --- | ---
    if (line.match(/^\|?(\s*:?-+:?\s*\|)+\s*:?-+:?\s*\|?$/)) {
      // Potential table delimiter.
      // Check if we have a preceding line in the buffer that can be a header.
      if (this.state.buffer.length > 0) {
        const headerLine = this.state.buffer[this.state.buffer.length - 1];
        // Header line must have pipes
        if (headerLine.includes("|")) {
          // We have a table!
          // 1. Finalize any preceding paragraph content (if buffer > 1)
          const preceding = this.state.buffer.slice(0, -1);
          if (preceding.length > 0) {
            // Update current paragraph to only include preceding lines
            this.currentBlock.children = this.parseInlines(
              preceding.join("\n"),
            );
            this.currentBlock = null;
          } else {
            // No preceding lines, so the current paragraph block was just for the header
            this.blocks.pop();
            this.currentBlock = null;
          }

          // 2. Start Table Block
          const alignments = this.parseTableAlignments(line);
          const headerCells = this.parseTableCells(headerLine);

          this.currentBlock = {
            kind: "table",
            header: { cells: headerCells },
            rows: [],
            alignments,
          };
          this.blocks.push(this.currentBlock);
          this.state.blockState = BlockState.TABLE;
          this.state.buffer = []; // Clear buffer
          return;
        }
      }
    }

    this.state.buffer.push(line);

    // Eagerly update paragraph
    if (!this.currentBlock || this.currentBlock.kind !== "paragraph") {
      this.currentBlock = {
        kind: "paragraph",
        children: [],
      };
      this.blocks.push(this.currentBlock);
    }
    const text = this.state.buffer.join("\n");
    this.currentBlock.children = this.parseInlines(text);
  }

  private handleTable(line: string) {
    if (!line.trim().includes("|")) {
      // End of table
      this.finalizeBlock();
      this.state.blockState = BlockState.ROOT;
      this.handleRoot(line);
      return;
    }

    const cells = this.parseTableCells(line);
    this.currentBlock.rows.push({ cells });
  }

  private parseTableAlignments(
    line: string,
  ): ("left" | "center" | "right" | null)[] {
    const parts = line.split("|");
    // Remove first/last if empty (leading/trailing pipe)
    if (parts[0].trim() === "") parts.shift();
    if (parts[parts.length - 1].trim() === "") parts.pop();

    return parts.map((part) => {
      const trimmed = part.trim();
      if (trimmed.startsWith(":") && trimmed.endsWith(":")) return "center";
      if (trimmed.endsWith(":")) return "right";
      if (trimmed.startsWith(":")) return "left";
      return null;
    });
  }

  private parseTableCells(line: string): any[] {
    const parts = line.split("|");
    if (parts[0].trim() === "") parts.shift();
    if (parts[parts.length - 1].trim() === "") parts.pop();

    return parts.map((part) => ({
      children: this.parseInlines(part.trim()),
    }));
  }

  private handleBlockquote(line: string) {
    if (line.trim() === "") {
      this.finalizeBlock();
      this.state.blockState = BlockState.ROOT;
      return;
    }

    if (line.startsWith(">")) {
      this.state.buffer.push(line.slice(1).trim());
    } else {
      this.finalizeBlock();
      this.state.blockState = BlockState.ROOT;
      this.handleRoot(line);
    }
  }

  private handleComment(line: string) {
    // Heuristic Recovery: If line starts with a strong block starter, assume comment was unclosed.
    // Strong starters: # (Header), ``` (Code), ::: (Container)
    // Note: We do NOT include > (Blockquote) or - (List) as they are common in comments.
    // We do NOT include <!-- because that is handled by nesting logic.
    const trimmed = line.trimStart();
    if (
      /^#{1,6}\s/.test(trimmed) || // Header (must have space)
      trimmed.startsWith("```") ||
      trimmed.startsWith(":::")
    ) {
      this.currentBlock = null;
      this.state.blockState = BlockState.ROOT;
      this.handleRoot(line);
      return;
    }

    let cursor = 0;

    // We need to scan the line for <!-- and --> to update nesting
    while (cursor < line.length) {
      if (line.startsWith("<!--", cursor)) {
        this.currentBlock.nestingLevel++;
        cursor += 4;
      } else if (line.startsWith("-->", cursor)) {
        if (this.currentBlock.nestingLevel > 0) {
          this.currentBlock.nestingLevel--;
          cursor += 3;

          if (this.currentBlock.nestingLevel === 0) {
            // Closed!
            this.currentBlock.value +=
              (this.currentBlock.value ? "\n" : "") + line.slice(0, cursor);
            this.currentBlock = null;
            this.state.blockState = BlockState.ROOT;
            return;
          }
        } else {
          cursor++;
        }
      } else {
        cursor++;
      }
    }

    this.currentBlock.value += (this.currentBlock.value ? "\n" : "") + line;

    // Sanity Limit
    const lineCount = this.currentBlock.value.split("\n").length;
    if (lineCount > 20) {
      // Abort!
      // Convert to paragraph
      this.blocks.pop();
      const text = this.currentBlock.value;
      this.blocks.push({
        kind: "paragraph",
        children: this.parseInlines(text),
      });
      this.currentBlock = null;
      this.state.blockState = BlockState.ROOT;
    }
  }

  private handleXmlBlock(line: string) {
    const tagName = this.currentBlock.tagName;
    const closeTag =
      tagName === "tool_code" ? "</tool_code>" : `</exo-${tagName}>`;

    const closeIdx = line.indexOf(closeTag);

    if (closeIdx !== -1) {
      this.currentBlock.content +=
        (this.currentBlock.content ? "\n" : "") + line.slice(0, closeIdx);
      this.currentBlock = null;
      this.state.blockState = BlockState.ROOT;

      // Process rest of line
      const rest = line.slice(closeIdx + closeTag.length);
      if (rest.trim()) {
        this.handleRoot(rest);
      }
    } else {
      this.currentBlock.content +=
        (this.currentBlock.content ? "\n" : "") + line;
    }
  }

  private handleContainer(line: string) {
    const match = line.trim().match(/^(:{3,})$/);
    if (match && match[1].length >= (this.currentBlock.fenceLength || 3)) {
      // End of container
      const text = this.state.buffer.join("\n");
      // Recursive parse
      const innerParser = new StreamingParser();
      innerParser.parse(text);
      const children = innerParser.flush();

      // Clean up internal props
      delete this.currentBlock.fenceLength;

      this.currentBlock.children = children;
      this.currentBlock = null;
      this.state.blockState = BlockState.ROOT;
      this.state.buffer = [];
    } else {
      this.state.buffer.push(line);
    }
  }

  private finalizeBlock() {
    if (this.state.blockState === BlockState.COMMENT) {
      // Unclosed comment at EOF
      if (this.currentBlock) {
        this.blocks.pop();
        const text = this.currentBlock.value;
        this.blocks.push({
          kind: "paragraph",
          children: this.parseInlines(text),
        });
        this.currentBlock = null;
      }
      this.state.blockState = BlockState.ROOT;
    } else if (this.state.blockState === BlockState.XML_BLOCK) {
      if (this.currentBlock) {
        this.currentBlock = null;
      }
      this.state.blockState = BlockState.ROOT;
    } else if (this.state.blockState === BlockState.CONTAINER) {
      if (this.currentBlock) {
        const text = this.state.buffer.join("\n");
        const innerParser = new StreamingParser();
        innerParser.parse(text);
        this.currentBlock.children = innerParser.flush();
        delete this.currentBlock.fenceLength;
        this.currentBlock = null;
      }
      this.state.blockState = BlockState.ROOT;
      this.state.buffer = [];
    } else if (this.state.blockState === BlockState.TABLE) {
      this.currentBlock = null;
      this.state.blockState = BlockState.ROOT;
    } else if (this.state.blockState === BlockState.PARAGRAPH) {
      if (this.state.buffer.length > 0) {
        const text = this.state.buffer.join("\n");

        // Field Refinement (**Key**: Value)
        const fieldMatch = text.match(/^\*\*([^*]+)\*\*:\s+(.*)/);
        if (fieldMatch && this.state.buffer.length === 1) {
          // Refine to Container
          this.currentBlock.kind = "container";
          this.currentBlock.variant = "field";
          this.currentBlock.children = [
            {
              kind: "paragraph",
              children: [
                {
                  kind: "strong",
                  children: [{ kind: "text", value: fieldMatch[1] }],
                },
              ],
            },
            { kind: "paragraph", children: this.parseInlines(fieldMatch[2]) },
          ];
        } else {
          // Already a paragraph, just ensure children are up to date
          this.currentBlock.children = this.parseInlines(text);
        }
      }

      // Filter out empty paragraphs
      if (this.currentBlock && this.currentBlock.kind === "paragraph") {
        const hasContent = this.currentBlock.children.some((child: any) => {
          if (child.kind === "text") return child.value.trim().length > 0;
          return true; // Other inlines (images, etc) count as content
        });

        if (!hasContent) {
          const idx = this.blocks.indexOf(this.currentBlock);
          if (idx !== -1) {
            this.blocks.splice(idx, 1);
          }
        }
      }

      this.currentBlock = null;
    } else if (this.state.blockState === BlockState.BLOCKQUOTE) {
      if (this.state.buffer.length > 0) {
        const text = this.state.buffer.join("\n");
        const alertMatch = text.match(
          /^\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]\s*([\s\S]*)/i,
        );
        if (alertMatch) {
          this.blocks.push({
            kind: "alert",
            variant: alertMatch[1].toLowerCase() as any,
            children: [
              {
                kind: "paragraph",
                children: this.parseInlines(alertMatch[2].trim()),
              },
            ],
          });
        } else {
          this.blocks.push({
            kind: "blockquote",
            children: [
              { kind: "paragraph", children: this.parseInlines(text) },
            ],
          });
        }
      }
    } else if (this.state.blockState === BlockState.CODE_BLOCK_INDENTED) {
      if (this.currentBlock) {
        // Already in blocks
        this.currentBlock = null;
      }
    }
    this.state.buffer = [];
  }

  private parseInlines(text: string): RTDInline[] {
    const inlines: RTDInline[] = [];
    let cursor = 0;

    // Surrogate Pair Handling
    // If the text ends with a high surrogate, we must NOT process it yet.
    // It will be processed when the low surrogate arrives in the next chunk.
    let effectiveLength = text.length;
    if (effectiveLength > 0) {
      const lastChar = text.charCodeAt(effectiveLength - 1);
      if (lastChar >= 0xd800 && lastChar <= 0xdbff) {
        effectiveLength--;
      }
    }

    while (cursor < effectiveLength) {
      // Ghost Citation
      if (text.startsWith("\u0000CITATION", cursor)) {
        const end = text.indexOf("\u0000", cursor + 9);
        if (end !== -1 && end < effectiveLength) {
          inlines.push({
            kind: "citation",
            value: text.slice(cursor + 9, end),
          });
          cursor = end + 1;
          continue;
        }
      }

      // Math Display ($$)
      if (text.startsWith("$$", cursor)) {
        const end = text.indexOf("$$", cursor + 2);
        if (end !== -1 && end < effectiveLength) {
          inlines.push({
            kind: "math-inline",
            value: text.slice(cursor + 2, end),
          });
          cursor = end + 2;
          continue;
        }
      }

      // Math Inline ($)
      if (text.startsWith("$", cursor)) {
        const end = text.indexOf("$", cursor + 1);
        if (end !== -1 && end < effectiveLength) {
          inlines.push({
            kind: "math-inline",
            value: text.slice(cursor + 1, end),
          });
          cursor = end + 1;
          continue;
        }
      }

      // Inline Code (`)
      if (text.startsWith("`", cursor)) {
        let fence = 0;
        while (
          cursor + fence < effectiveLength &&
          text[cursor + fence] === "`"
        ) {
          fence++;
        }

        const end = text.indexOf("`".repeat(fence), cursor + fence);
        if (end !== -1 && end < effectiveLength) {
          const content = text.slice(cursor + fence, end);
          inlines.push({
            kind: "code-span",
            value: content,
          });
          cursor = end + fence;
          continue;
        }
      }

      // Bold (**)
      if (text.startsWith("**", cursor)) {
        const end = text.indexOf("**", cursor + 2);
        if (end !== -1 && end < effectiveLength) {
          inlines.push({
            kind: "strong",
            children: this.parseInlines(text.slice(cursor + 2, end)),
          });
          cursor = end + 2;
          continue;
        }
      }
      // Bold (__)
      if (text.startsWith("__", cursor)) {
        const end = text.indexOf("__", cursor + 2);
        if (end !== -1 && end < effectiveLength) {
          inlines.push({
            kind: "strong",
            children: this.parseInlines(text.slice(cursor + 2, end)),
          });
          cursor = end + 2;
          continue;
        }
      }

      // Strikethrough (~~)
      if (text.startsWith("~~", cursor)) {
        const end = text.indexOf("~~", cursor + 2);
        if (end !== -1 && end < effectiveLength) {
          inlines.push({
            kind: "strikethrough",
            children: this.parseInlines(text.slice(cursor + 2, end)),
          });
          cursor = end + 2;
          continue;
        }
      }

      // Italic (*)
      if (text.startsWith("*", cursor)) {
        const end = text.indexOf("*", cursor + 1);
        if (end !== -1 && end < effectiveLength) {
          inlines.push({
            kind: "emphasis",
            children: this.parseInlines(text.slice(cursor + 1, end)),
          });
          cursor = end + 1;
          continue;
        }
      }

      // Italic (_)
      if (text.startsWith("_", cursor)) {
        const prevChar = cursor > 0 ? text[cursor - 1] : " ";
        const isPrevAlpha = /[a-zA-Z0-9]/.test(prevChar);

        const end = text.indexOf("_", cursor + 1);
        if (end !== -1 && end < effectiveLength) {
          const isStartIntraword =
            isPrevAlpha && /[a-zA-Z0-9]/.test(text[cursor + 1]);

          if (!isStartIntraword) {
            inlines.push({
              kind: "emphasis",
              children: this.parseInlines(text.slice(cursor + 1, end)),
            });
            cursor = end + 1;
            continue;
          }
        }
      }

      // Icon
      if (text.startsWith("$(", cursor)) {
        const end = text.indexOf(")", cursor + 2);
        if (end !== -1 && end < effectiveLength) {
          const name = text.slice(cursor + 2, end);
          if (/^[a-z0-9-]+$/.test(name)) {
            inlines.push({ kind: "icon", name });
            cursor = end + 1;
            continue;
          }
        }
      }

      // Image (![...](...))
      if (text.startsWith("![", cursor)) {
        const closeBracket = this.findBalanced(text, cursor + 2, "[", "]");
        if (
          closeBracket !== -1 &&
          closeBracket + 1 < effectiveLength &&
          text[closeBracket + 1] === "("
        ) {
          const closeParen = this.findBalanced(
            text,
            closeBracket + 2,
            "(",
            ")",
          );
          if (closeParen !== -1) {
            const alt = text.slice(cursor + 2, closeBracket);
            const src = text.slice(closeBracket + 2, closeParen);
            inlines.push({ kind: "image", alt, src });
            cursor = closeParen + 1;
            continue;
          }
        }
      }

      // Link ([...](...))
      if (text.startsWith("[", cursor)) {
        const closeBracket = this.findBalanced(text, cursor + 1, "[", "]");
        if (
          closeBracket !== -1 &&
          closeBracket + 1 < effectiveLength &&
          text[closeBracket + 1] === "("
        ) {
          const closeParen = this.findBalanced(
            text,
            closeBracket + 2,
            "(",
            ")",
          );
          if (closeParen !== -1) {
            const content = text.slice(cursor + 1, closeBracket);
            const href = text.slice(closeBracket + 2, closeParen);
            inlines.push({
              kind: "link",
              href,
              children: this.parseInlines(content),
            });
            cursor = closeParen + 1;
            continue;
          }
        }
      }

      // Comment (Inline)
      if (text.startsWith("<!--", cursor)) {
        let nesting = 1;
        let current = cursor + 4;
        let foundEnd = false;

        while (current < effectiveLength) {
          if (text.startsWith("<!--", current)) {
            nesting++;
            current += 4;
          } else if (text.startsWith("-->", current)) {
            nesting--;
            current += 3;
            if (nesting === 0) {
              inlines.push({
                kind: "comment",
                value: text.slice(cursor + 4, current - 3),
              });
              cursor = current;
              foundEnd = true;
              break;
            }
          } else {
            current++;
          }
        }

        if (foundEnd) continue;

        // If unclosed at end of string, treat as text (or partial comment if we were streaming inlines?)
        // Since parseInlines is called on finalizeBlock, we assume we have the full text.
        // So unclosed = text.
      }

      // Text
      inlines.push({ kind: "text", value: text[cursor] });
      cursor++;
    }

    // Coalesce
    const coalesced: RTDInline[] = [];
    for (const inline of inlines) {
      if (
        inline.kind === "text" &&
        coalesced.length > 0 &&
        coalesced[coalesced.length - 1].kind === "text"
      ) {
        (coalesced[coalesced.length - 1] as any).value += inline.value;
      } else {
        coalesced.push(inline);
      }
    }
    return coalesced;
  }

  private parseAttributes(attrStr: string): Record<string, string> {
    const attrs: Record<string, string> = {};
    const regex = /([a-zA-Z]+)\s*=\s*(?:"((?:[^"]|\\")*)"|'((?:[^']|\\')*)')/g;
    let match;
    while ((match = regex.exec(attrStr)) !== null) {
      const key = match[1];
      if (match[2] !== undefined) {
        attrs[key] = match[2].replace(/\\"/g, '"');
      } else {
        attrs[key] = match[3].replace(/\\'/g, "'");
      }
    }
    return attrs;
  }

  private findBalanced(
    text: string,
    start: number,
    open: string,
    close: string,
  ): number {
    let depth = 1;
    let i = start;
    while (i < text.length) {
      if (text[i] === open) {
        depth++;
      } else if (text[i] === close) {
        depth--;
        if (depth === 0) {
          return i;
        }
      }
      i++;
    }
    return -1;
  }
}
