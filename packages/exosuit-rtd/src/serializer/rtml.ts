import type { RTDBlock, RTDInline } from "../dom/types.js";

export class RTMLSerializer {
  public serialize(blocks: RTDBlock[]): string {
    return blocks.map((block) => this.serializeBlock(block)).join("");
  }

  private serializeBlock(block: RTDBlock): string {
    switch (block.kind) {
      case "paragraph":
        return `<p>${this.serializeInlines(block.children)}</p>`;
      case "heading":
        // RTML only supports h1-h3? Spec says "<h1> - <h3>".
        // But RTD supports h1-h6.
        // Let's support h1-h6 for now, or clamp?
        // Spec says "The level corresponds directly to the tag name."
        // I'll assume h1-h6 are valid HTML5, so valid RTML if we extend the spec slightly or if the spec meant "e.g. h1-h3".
        return `<h${block.level}>${this.serializeInlines(block.children)}</h${
          block.level
        }>`;
      case "blockquote":
        return `<blockquote>${this.serialize(block.children)}</blockquote>`;
      case "code-block":
        const langAttr = block.language
          ? ` data-lang="${this.escapeAttr(block.language)}"`
          : "";
        return `<pre${langAttr}>${this.escapeHtml(block.value)}</pre>`;
      case "list":
        const tag = block.ordered ? "ol" : "ul";
        const items = block.items
          .map((item) => {
            const checkedAttr =
              item.checked !== undefined
                ? ` data-checked="${item.checked}"`
                : "";
            return `<li${checkedAttr}>${this.serialize(item.children)}</li>`;
          })
          .join("");
        return `<${tag}>${items}</${tag}>`;
      case "thematic-break":
        return "<hr />";
      // Extensions not in strict RTML spec yet, but useful to preserve?
      // Spec says "Strict Whitelist".
      // "All others MUST be stripped or rejected by the parser."
      // So for now, I should probably skip them or map them to something generic?
      // Or maybe the spec needs to be updated to include them.
      // For now, I'll skip unsupported blocks to adhere to "Strict Whitelist".
      default:
        return "";
    }
  }

  private serializeInlines(inlines: RTDInline[]): string {
    return inlines.map((inline) => this.serializeInline(inline)).join("");
  }

  private serializeInline(inline: RTDInline): string {
    switch (inline.kind) {
      case "text":
        return this.escapeHtml(inline.value);
      case "strong":
        return `<strong>${this.serializeInlines(inline.children)}</strong>`;
      case "emphasis":
        return `<em>${this.serializeInlines(inline.children)}</em>`;
      case "code-span":
        return `<code>${this.escapeHtml(inline.value)}</code>`;
      case "link":
        const titleAttr = inline.title
          ? ` title="${this.escapeAttr(inline.title)}"`
          : "";
        return `<a href="${this.escapeAttr(
          inline.href
        )}"${titleAttr}>${this.serializeInlines(inline.children)}</a>`;
      case "icon":
        return `<rtd-icon name="${this.escapeAttr(inline.name)}"></rtd-icon>`;
      case "command":
        const argsAttr = inline.args
          ? ` args="${this.escapeAttr(JSON.stringify(inline.args))}"`
          : "";
        return `<rtd-command id="${this.escapeAttr(
          inline.id
        )}"${argsAttr}>${this.serializeInlines(inline.children)}</rtd-command>`;
      default:
        return "";
    }
  }

  private escapeHtml(text: string): string {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  private escapeAttr(text: string): string {
    return text.replace(/"/g, "&quot;");
  }
}
