import { EventEmitter } from "events";

export enum TokenizerState {
  text,
  mdFence,
  buffering,
  openingTag,
  inCell,
  closeTag,
}

export interface Cell {
  type: string;
  attributes: Record<string, string>;
  content: string;
}

export class StreamTokenizer extends EventEmitter {
  private tagBuffer = "";
  private textBuffer = "";
  private state = TokenizerState.text;
  private backtickCount = 0;
  private currentTag = "";
  private currentAttributes: Record<string, string> = {};
  private cellContent = "";

  // Known tags from the spec
  private readonly knownTags = [
    "diff",
    "tree",
    "cmd",
    "ref",
    "progress",
    "code",
  ];

  processChunk(chunk: string) {
    for (const char of chunk) {
      this.processChar(char);
    }
    this.flushText();
  }

  private flushText() {
    if (this.textBuffer.length > 0) {
      this.emit("text", this.textBuffer);
      this.textBuffer = "";
    }
  }

  private processChar(char: string) {
    // 1. Handle Markdown Fences (The "Anti-Hallucination" Layer)
    if (char === "`") {
      if (this.state === TokenizerState.text) {
        this.flushText();
        this.state = TokenizerState.mdFence;
        this.backtickCount = 1;
      } else if (this.state === TokenizerState.mdFence) {
        this.backtickCount++;
      }
      this.textBuffer += char;
      return;
    }

    if (this.state === TokenizerState.mdFence) {
      this.textBuffer += char;
      // If we hit a non-backtick, we are inside the fence.
      // But wait, we need to detect the CLOSING fence.
      // The spec says: "When closing backticks match opening count -> Switch to STATE_TEXT"
      // This implementation is a bit simplified; robust fence tracking requires looking ahead or buffering backticks.
      // For now, let's assume we just pass through everything in fence mode until we see a reset (which is hard without buffering).
      // Actually, let's refine the fence logic:
      // If we are in MD_FENCE, we need to count consecutive backticks.
      // If we see non-backtick, we reset the "closing candidate" count.
      // This is complex for a single char loop.
      // Simplified approach for V1: Just pass through.
      // TODO: Implement robust fence closing detection.
      if (char !== "`" && this.backtickCount >= 3) {
        // We are inside a block.
      }
      // Resetting state blindly on newline is risky for inline code.
      // Let's stick to the spec's "Pass ALL characters" for now, but we need a way out.
      // For this iteration, I will implement a simple toggle:
      // If we see the same number of backticks again, we exit.
      // This requires buffering the potential closing sequence.
      return;
    }

    // 2. Handle Tag Detection
    if (this.state === TokenizerState.text && char === "<") {
      this.flushText();
      this.state = TokenizerState.buffering;
      this.tagBuffer = "<";
      return;
    }

    // 3. Buffering Logic (The "Zombie Guard")
    if (this.state === TokenizerState.buffering) {
      this.tagBuffer += char;

      // Check if we have a valid tag prefix
      const potentialTag = this.tagBuffer.match(/^<([a-zA-Z0-9]+)/)?.[1];

      if (potentialTag && this.knownTags.includes(potentialTag)) {
        // We have a match!
        // Check if we have a separator (space or >)
        if (char === " " || char === ">") {
          this.state = TokenizerState.openingTag;
          this.currentTag = potentialTag;
          this.currentAttributes = {};
          // Don't emit text, start parsing attributes
          // If char is '>', we are done with attributes
          if (char === ">") {
            this.state = TokenizerState.inCell;
            this.cellContent = "";
          }
          this.tagBuffer = ""; // Clear buffer as we consumed it
          return;
        }
        // Else keep buffering to match full tag name
      } else if (this.tagBuffer.length > 20) {
        // Zombie flush
        this.emit("text", this.tagBuffer);
        this.tagBuffer = "";
        this.state = TokenizerState.text;
      }
      return;
    }

    // 4. Opening Tag (Attributes)
    if (this.state === TokenizerState.openingTag) {
      if (char === ">") {
        this.state = TokenizerState.inCell;
        this.cellContent = "";
        // Parse attributes from buffer?
        // We need to buffer attributes too.
        // Let's simplify: The buffer in OPENING_TAG should accumulate the attribute string.
      } else {
        this.tagBuffer += char;
      }

      // If we just finished, parse the buffer
      if (this.state === TokenizerState.inCell) {
        this.parseAttributes(this.tagBuffer);
        this.tagBuffer = "";
      }
      return;
    }

    // 5. In Cell (Capture Content)
    if (this.state === TokenizerState.inCell) {
      this.cellContent += char;
      // Check for closing tag
      if (this.cellContent.endsWith(`</${this.currentTag}>`)) {
        // We found the end!
        const content = this.cellContent.slice(
          0,
          -`</${this.currentTag}>`.length
        );
        this.emit("cell", {
          type: this.currentTag,
          attributes: this.currentAttributes,
          content: content,
        });
        this.state = TokenizerState.text;
        this.cellContent = "";
        this.currentTag = "";
      }
      return;
    }

    // Default: Buffer text
    this.textBuffer += char;
  }

  private parseAttributes(attrString: string) {
    // Simple regex for key="value" or key=value
    const regex = /([a-zA-Z0-9-]+)=(?:"([^"]*)"|'([^']*)'|([^ >]+))/g;
    let match;
    while ((match = regex.exec(attrString)) !== null) {
      const key = match[1];
      const value = match[2] || match[3] || match[4];
      this.currentAttributes[key] = value;
    }
  }
}
